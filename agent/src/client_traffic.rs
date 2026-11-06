//! Per-device internet traffic from conntrack accounting.
//!
//! The firmware has no per-client counters (`router_get_clients_traffic`
//! answers empty), but `nf_conntrack_acct` is on and IPA hardware offload
//! syncs its byte counts back into conntrack (checked 2026-10-06: a 38.6 MB
//! offloaded download showed 40.7 MB in the reply direction). Every 10 s the
//! table is read and per-flow growth is added to the client's MAC; closed
//! flows linger in conntrack (TCP TIME_WAIT, UDP timeouts), so their last
//! bytes are still seen. Only flows from a LAN device (a real source MAC) to
//! a non-LAN address count. Totals survive agent restarts via a state file.
//!
//! A flow missing from one read (the read is capped at `MAX_READ`, so a big
//! table loses its tail) keeps its last counters for a few reads, so it is
//! not counted again from zero when it comes back. Clients idle for 30 days
//! are dropped, and at most `MAX_CLIENTS` are kept, so randomized MACs do not
//! pile up in the state file.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::csv_utils::now_secs;
use crate::handlers::AppState;
use crate::storage::atomic_write;
use crate::ubus;
use crate::util::MutexExt;

const CONNTRACK: &str = "/proc/net/nf_conntrack";
const STATE_FILE: &str = "/data/local/tmp/client_traffic.json";
const SAMPLE: Duration = Duration::from_secs(10);
const SAVE_EVERY: Duration = Duration::from_secs(300);
const MAX_READ: u64 = 8 * 1024 * 1024;
/// A flow absent from this many reads in a row is forgotten. Until then a
/// flow that comes back is measured against its last counters.
const FLOW_MISSES: u64 = 3;
/// Clients that moved no bytes for this long are dropped from the totals.
const KEEP_IDLE_SECS: u64 = 30 * 24 * 3600;
/// At most this many clients are kept; the most recently active win.
const MAX_CLIENTS: usize = 256;
/// Wall-clock seconds below this (2025-01-01) mean the clock is not set yet,
/// so nothing is stamped or aged out by it.
const SANE_EPOCH: u64 = 1_735_689_600;
/// The LAN prefix is re-read every this many samples (10 min)…
const LAN_REFRESH_TICKS: u32 = 60;
/// …or after this many (1 min) when no client address falls inside it.
const LAN_RETRY_TICKS: u32 = 6;

/// First three octets of the router's LAN address: the /24 treated as local.
type Lan = [u8; 3];
const DEFAULT_LAN: Lan = [192, 168, 0];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct Totals {
    up: u64,
    down: u64,
    ip: String,
    /// Device-clock epoch seconds when this client last moved bytes. 0 means
    /// unknown (state files written before this field existed).
    last_seen: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    since: String,
    clients: HashMap<String, Totals>,
}

impl Saved {
    fn load(bytes: &[u8]) -> Option<Saved> {
        serde_json::from_slice(bytes).ok()
    }

    /// Drop clients idle for `KEEP_IDLE_SECS`, then cap the map at
    /// `MAX_CLIENTS`. Returns whether anything changed.
    fn prune(&mut self, wall: u64) -> bool {
        let before = self.clients.len();
        let mut stamped = false;
        if wall >= SANE_EPOCH {
            for t in self.clients.values_mut() {
                if t.last_seen == 0 {
                    // Age unknown (older state file): start its clock now.
                    t.last_seen = wall;
                    stamped = true;
                }
            }
            self.clients
                .retain(|_, t| wall.saturating_sub(t.last_seen) < KEEP_IDLE_SECS);
        }
        if self.clients.len() > MAX_CLIENTS {
            let mut order: Vec<(u64, u64, String)> = self
                .clients
                .iter()
                .map(|(mac, t)| (t.last_seen, t.up.saturating_add(t.down), mac.clone()))
                .collect();
            // Most recently active first, then the biggest.
            order.sort_unstable_by(|a, b| b.cmp(a));
            for (_, _, mac) in order.drain(MAX_CLIENTS..) {
                self.clients.remove(&mac);
            }
        }
        stamped || self.clients.len() != before
    }
}

/// Counters of one flow at the last read that showed it.
#[derive(Clone, Copy, Debug)]
struct FlowState {
    up: u64,
    down: u64,
    /// `Tracker::tick` of that read.
    seen: u64,
}

#[derive(Default)]
struct Tracker {
    saved: Saved,
    flows: HashMap<FlowKey, FlowState>,
    /// mac → (up, down) bytes per second over the last sample
    rates: HashMap<String, (u64, u64)>,
    sampled: Option<Instant>,
    /// Number of snapshots ingested so far.
    tick: u64,
    /// Totals changed since the last save.
    dirty: bool,
}

static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);

/// Original-direction (protocol number, src, dst, sport, dport). ICMP has no
/// ports; its echo id stands in for the source port.
type FlowKey = (u8, IpAddr, IpAddr, u16, u16);

#[derive(Debug, PartialEq)]
struct Flow<'a> {
    key: FlowKey,
    mac: &'a str,
    src: &'a str,
    up: u64,
    down: u64,
}

/// One /proc/net/nf_conntrack line → the client flow, if it is one.
fn parse_line(line: &str) -> Option<Flow<'_>> {
    let mut toks = line.split_whitespace();
    // "ipv4 2 tcp 6 …": L3 name and number, then L4 name and number.
    let proto: u8 = toks.nth(3)?.parse().ok()?;
    let (mut src, mut dst, mut mac) = (None, None, None);
    let (mut sport, mut dport) = (None, None);
    let mut bytes = [0u64; 2];
    let mut nbytes = 0;
    for tok in toks {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "src" if src.is_none() => src = Some(v),
            "dst" if dst.is_none() => dst = Some(v),
            "sport" | "id" if sport.is_none() => sport = Some(v.parse::<u16>().ok()?),
            "dport" if dport.is_none() => dport = Some(v.parse::<u16>().ok()?),
            "src_mac" if mac.is_none() => mac = Some(v),
            "bytes" if nbytes < 2 => {
                bytes[nbytes] = v.parse().ok()?;
                nbytes += 1;
            }
            _ => {}
        }
    }
    let (src, dst, mac) = (src?, dst?, mac?);
    if mac == "00:00:00:00:00:00" || nbytes < 2 {
        return None;
    }
    Some(Flow {
        key: (
            proto,
            src.parse().ok()?,
            dst.parse().ok()?,
            sport.unwrap_or(0),
            dport.unwrap_or(0),
        ),
        mac,
        src,
        up: bytes[0],
        down: bytes[1],
    })
}

fn in_lan(ip: Ipv4Addr, lan: Lan) -> bool {
    ip.octets()[..3] == lan
}

/// Is `dst` inside the LAN (or the client's own /24) or not routable?
fn local_destination(src: IpAddr, dst: IpAddr, lan: Lan) -> bool {
    match dst {
        IpAddr::V4(d) => {
            in_lan(d, lan)
                || matches!(src, IpAddr::V4(s) if s.octets()[..3] == d.octets()[..3])
                || d.is_multicast()
                || d.is_broadcast()
        }
        IpAddr::V6(d) => d.is_multicast() || d.segments()[0] & 0xffc0 == 0xfe80,
    }
}

/// Byte growth of one client within one snapshot.
#[derive(Default)]
struct Delta<'a> {
    up: u64,
    down: u64,
    ip: Option<&'a str>,
}

impl Tracker {
    /// Fold one conntrack snapshot in. The first one only learns the flows
    /// that already exist. Returns true when IPv4 clients were seen but none
    /// of them is inside `lan`, i.e. the LAN prefix looks stale.
    fn ingest(&mut self, text: &str, lan: Lan, now: Instant, wall: u64) -> bool {
        let elapsed = self
            .sampled
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        let learn_only = self.sampled.is_none();
        self.tick += 1;
        let tick = self.tick;
        // A read cut off at MAX_READ ends in a partial line (possibly a short
        // `bytes=` value). Never parse it; that flow just misses this read.
        let text = text.rfind('\n').map_or("", |i| &text[..=i]);
        let (mut v4_clients, mut v4_in_lan) = (0usize, 0usize);
        let mut delta: HashMap<&str, Delta> = HashMap::new();
        for flow in text.lines().filter_map(parse_line) {
            let (_, src, dst, _, _) = flow.key;
            if let IpAddr::V4(s) = src {
                v4_clients += 1;
                v4_in_lan += usize::from(in_lan(s, lan));
            }
            if local_destination(src, dst, lan) {
                continue;
            }
            let (du, dd) = match self.flows.entry(flow.key) {
                Entry::Occupied(mut e) => {
                    let s = e.get_mut();
                    if s.seen == tick {
                        // The same tuple twice in one read: count it once.
                        continue;
                    }
                    let d = if flow.up < s.up || flow.down < s.down {
                        // Counters went backwards: a new flow reused the
                        // tuple. Count it from zero, never a negative delta.
                        (flow.up, flow.down)
                    } else {
                        (flow.up - s.up, flow.down - s.down)
                    };
                    *s = FlowState {
                        up: flow.up,
                        down: flow.down,
                        seen: tick,
                    };
                    d
                }
                Entry::Vacant(e) => {
                    e.insert(FlowState {
                        up: flow.up,
                        down: flow.down,
                        seen: tick,
                    });
                    (flow.up, flow.down)
                }
            };
            let d = delta.entry(flow.mac).or_default();
            d.up += du;
            d.down += dd;
            if src.is_ipv4() {
                d.ip = Some(flow.src);
            }
        }
        self.flows.retain(|_, s| tick - s.seen < FLOW_MISSES);
        self.rates.clear();
        if !learn_only {
            let per_sec = |b: u64| (b as f64 / elapsed) as u64;
            for (mac, d) in delta {
                let mac = mac.to_ascii_lowercase();
                let moved = d.up > 0 || d.down > 0;
                if !moved && !self.saved.clients.contains_key(&mac) {
                    continue;
                }
                if moved && elapsed > 0.5 {
                    self.rates
                        .insert(mac.clone(), (per_sec(d.up), per_sec(d.down)));
                }
                let t = self.saved.clients.entry(mac).or_default();
                if moved {
                    t.up += d.up;
                    t.down += d.down;
                    if wall >= SANE_EPOCH {
                        t.last_seen = wall;
                    }
                    self.dirty = true;
                }
                if let Some(ip) = d.ip.filter(|ip| t.ip != *ip) {
                    t.ip = ip.to_string();
                    self.dirty = true;
                }
            }
        }
        self.sampled = Some(now);
        v4_clients > 0 && v4_in_lan == 0
    }
}

/// The router's LAN /24 from uci, or None when ubus could not answer.
fn lan_prefix() -> Option<Lan> {
    let v = ubus::call(
        "uci",
        "get",
        Some(r#"{"config":"network","section":"lan","option":"ipaddr"}"#),
    )
    .ok()?;
    let ip: Ipv4Addr = v["value"].as_str()?.split('/').next()?.parse().ok()?;
    let o = ip.octets();
    Some([o[0], o[1], o[2]])
}

fn now_text() -> String {
    ubus::call("zwrt_sntp", "get_systime", Some("{}"))
        .ok()
        .and_then(|v| v["localtime"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Read the conntrack table into `buf` (reused across samples).
fn read_conntrack(buf: &mut String) -> bool {
    use std::io::Read;
    buf.clear();
    std::fs::File::open(CONNTRACK)
        .and_then(|f| f.take(MAX_READ).read_to_string(buf))
        .is_ok()
}

fn save(saved: &Saved) {
    if let Ok(bytes) = serde_json::to_vec(saved) {
        let _ = atomic_write(Path::new(STATE_FILE), &bytes);
    }
}

pub fn start() {
    let mut saved = std::fs::read(STATE_FILE)
        .ok()
        .and_then(|b| Saved::load(&b))
        .unwrap_or_else(|| Saved {
            since: now_text(),
            ..Saved::default()
        });
    let dirty = saved.prune(now_secs());
    *TRACKER.safe_lock() = Some(Tracker {
        saved,
        dirty,
        ..Tracker::default()
    });
    std::thread::spawn(|| {
        let first = lan_prefix();
        let mut lan_known = first.is_some();
        let mut lan = first.unwrap_or(DEFAULT_LAN);
        let mut lan_age = 0u32;
        let mut saved_at = Instant::now();
        let mut buf = String::new();
        let mut warned = false;
        loop {
            let mut lan_stale = !lan_known;
            if read_conntrack(&mut buf) {
                if !warned && buf.len() as u64 >= MAX_READ {
                    eprintln!(
                        "[client-traffic] conntrack table exceeds {MAX_READ} bytes; flows past the cut are not counted"
                    );
                    warned = true;
                }
                let mut guard = TRACKER.safe_lock();
                if let Some(t) = guard.as_mut() {
                    lan_stale |= t.ingest(&buf, lan, Instant::now(), now_secs());
                    if saved_at.elapsed() >= SAVE_EVERY {
                        if t.saved.prune(now_secs()) {
                            t.dirty = true;
                        }
                        if t.dirty {
                            save(&t.saved);
                            t.dirty = false;
                        }
                        saved_at = Instant::now();
                    }
                }
            }
            // ubus forks, so the prefix is re-read rarely and outside the lock.
            lan_age += 1;
            if lan_age >= LAN_REFRESH_TICKS || (lan_stale && lan_age >= LAN_RETRY_TICKS) {
                lan_age = 0;
                match lan_prefix() {
                    Some(p) => {
                        lan = p;
                        lan_known = true;
                    }
                    None => lan_known = false,
                }
            }
            std::thread::sleep(SAMPLE);
        }
    });
}

/// GET /api/network/clients/traffic
pub fn get(_state: &AppState) -> (u16, Value) {
    let guard = TRACKER.safe_lock();
    let Some(t) = guard.as_ref() else {
        return (
            503,
            json!({"ok": false, "error": "traffic counting has not started"}),
        );
    };
    let mut clients: Vec<Value> = t
        .saved
        .clients
        .iter()
        .map(|(mac, tot)| {
            let (ur, dr) = t.rates.get(mac).copied().unwrap_or((0, 0));
            json!({
                "mac": mac.to_uppercase(),
                "ip": Some(&tot.ip).filter(|s| !s.is_empty()),
                "up_bytes": tot.up,
                "down_bytes": tot.down,
                "up_rate": ur,
                "down_rate": dr,
            })
        })
        .collect();
    clients.sort_by(|a, b| b["down_bytes"].as_u64().cmp(&a["down_bytes"].as_u64()));
    (
        200,
        json!({"ok": true, "data": {
            "since": Some(&t.saved.since).filter(|s| !s.is_empty()),
            "sampled_secs_ago": t.sampled.map(|s| s.elapsed().as_secs()),
            "clients": clients,
        }}),
    )
}

/// POST /api/network/clients/traffic/reset
pub fn reset(state: &AppState) -> (u16, Value) {
    {
        let mut guard = TRACKER.safe_lock();
        if let Some(t) = guard.as_mut() {
            t.saved = Saved {
                since: now_text(),
                ..Saved::default()
            };
            t.rates.clear();
            save(&t.saved);
            t.dirty = false;
        }
    }
    get(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "ipv4     2 tcp      6 9 CLOSE src=192.168.0.149 dst=39.130.176.204 sport=60570 dport=443 src_mac=5c:4d:bf:ee:e1:66 dst_mac=00:00:00:00:00:00 packets=12162 bytes=649403 src=39.130.176.204 dst=10.66.9.85 sport=443 dport=60570 src_mac=00:00:00:00:00:00 dst_mac=00:00:00:00:00:00 packets=28979 bytes=40732680 [ASSURED] mark=0 use=2";
    const MAC: &str = "5c:4d:bf:ee:e1:66";
    const LAN: Lan = [192, 168, 0];
    const WALL: u64 = 1_800_000_000;

    /// A complete conntrack line for a client TCP flow to `dst`.
    fn flow(src: &str, dst: &str, sport: u16, up: u64, down: u64) -> String {
        format!(
            "ipv4     2 tcp      6 7440 ESTABLISHED src={src} dst={dst} sport={sport} dport=443 src_mac={MAC} dst_mac=00:00:00:00:00:00 packets=10 bytes={up} src={dst} dst=10.66.9.85 sport=443 dport={sport} src_mac=00:00:00:00:00:00 dst_mac=00:00:00:00:00:00 packets=20 bytes={down} [ASSURED] mark=0 use=2\n"
        )
    }

    fn a(up: u64, down: u64) -> String {
        flow("192.168.0.149", "39.130.176.204", 1000, up, down)
    }

    fn b(up: u64, down: u64) -> String {
        flow("192.168.0.149", "39.130.176.205", 1001, up, down)
    }

    /// Feeds snapshots 10 s apart; the first one only learns.
    struct Run {
        t: Tracker,
        t0: Instant,
    }

    impl Run {
        fn new(first: &str) -> Self {
            let mut r = Run {
                t: Tracker::default(),
                t0: Instant::now(),
            };
            r.feed(first);
            r
        }

        fn feed(&mut self, text: &str) -> bool {
            let now = self.t0 + SAMPLE * self.t.tick as u32;
            self.t.ingest(text, LAN, now, WALL)
        }

        fn totals(&self) -> (u64, u64) {
            self.t
                .saved
                .clients
                .get(MAC)
                .map_or((0, 0), |t| (t.up, t.down))
        }
    }

    #[test]
    fn parses_client_flows_only() {
        let f = parse_line(LINE).unwrap();
        assert_eq!(f.mac, MAC);
        assert_eq!(f.src, "192.168.0.149");
        assert_eq!((f.up, f.down), (649_403, 40_732_680));
        assert_eq!((f.key.0, f.key.3, f.key.4), (6, 60570, 443));
        let router_own = LINE.replace("src_mac=5c:4d:bf:ee:e1:66", "src_mac=00:00:00:00:00:00");
        assert!(parse_line(&router_own).is_none());
        assert!(parse_line("garbage").is_none());
        let icmp = "ipv4     2 icmp     1 29 src=192.168.0.5 dst=8.8.8.8 type=8 code=0 id=77 src_mac=5c:4d:bf:ee:e1:66 dst_mac=00:00:00:00:00:00 packets=1 bytes=84 src=8.8.8.8 dst=10.66.9.85 type=0 code=0 id=77 src_mac=00:00:00:00:00:00 dst_mac=00:00:00:00:00:00 packets=1 bytes=84 mark=0 use=1";
        assert_eq!(parse_line(icmp).unwrap().key.3, 77);
    }

    #[test]
    fn accumulates_growth_and_skips_lan_traffic() {
        let mut r = Run::new(&format!("{LINE}\n"));
        assert!(r.t.saved.clients.is_empty(), "first pass only learns");
        let grown = LINE.replace("bytes=40732680", "bytes=41732680");
        r.feed(&format!("{grown}\n"));
        assert_eq!(r.totals(), (0, 1_000_000));
        assert_eq!(r.t.saved.clients[MAC].ip, "192.168.0.149");
        assert_eq!(r.t.saved.clients[MAC].last_seen, WALL);
        assert_eq!(r.t.rates[MAC].1, 100_000);

        let router = flow("192.168.0.149", "192.168.0.1", 1002, 0, 0);
        let mut l = Run::new(&router);
        l.feed(&flow("192.168.0.149", "192.168.0.1", 1002, 500, 9000));
        assert!(l.t.saved.clients.is_empty());
    }

    #[test]
    fn partial_last_line_is_never_parsed() {
        let mut r = Run::new(&(a(100, 1000) + &b(100, 40_000_000)));
        // A read cut at MAX_READ inside b's reply `bytes=` field.
        let full = b(200, 41_000_000);
        let cut = &full[..full.rfind("bytes=41").unwrap() + "bytes=41".len()];
        r.feed(&(a(100, 2000) + cut));
        assert_eq!(r.totals(), (0, 1000), "only a's growth");
        r.feed(&(a(100, 2000) + &b(200, 41_000_000)));
        assert_eq!(r.totals(), (100, 1000 + 1_000_000), "b measured from 40 MB");
    }

    #[test]
    fn missing_flow_is_not_counted_twice() {
        let mut r = Run::new(&(a(100, 1000) + &b(500, 5_000_000)));
        r.feed(&a(100, 1000));
        r.feed(&a(100, 1000));
        assert_eq!(r.t.flows.len(), 2, "b kept while it misses < 3 reads");
        r.feed(&(a(100, 1000) + &b(600, 5_100_000)));
        assert_eq!(r.totals(), (100, 100_000));
        for _ in 0..FLOW_MISSES {
            r.feed(&a(100, 1000));
        }
        assert_eq!(r.t.flows.len(), 1, "b forgotten after 3 misses");
    }

    #[test]
    fn counter_reset_restarts_from_new_value() {
        let mut r = Run::new(&a(5000, 90_000));
        r.feed(&a(300, 1200));
        assert_eq!(r.totals(), (300, 1200));
        r.feed(&a(400, 1500));
        assert_eq!(r.totals(), (400, 1500));
    }

    #[test]
    fn dirty_only_when_totals_change() {
        let mut r = Run::new(&a(100, 1000));
        assert!(!r.t.dirty, "learning changes nothing");
        r.feed(&a(100, 1000));
        assert!(!r.t.dirty, "no growth, nothing to save");
        r.feed(&a(100, 1500));
        assert!(r.t.dirty);
        r.t.dirty = false;
        r.feed(&a(100, 1500));
        assert!(!r.t.dirty);
    }

    #[test]
    fn learn_pass_keeps_restored_totals() {
        let mut t = Tracker::default();
        t.saved.clients.insert(
            MAC.into(),
            Totals {
                up: 7,
                down: 9,
                ..Totals::default()
            },
        );
        t.ingest(&a(100, 1000), LAN, Instant::now(), WALL);
        assert_eq!(t.saved.clients[MAC].down, 9);
        assert!(t.rates.is_empty() && !t.dirty);
    }

    #[test]
    fn same_subnet_traffic_is_local_and_flags_a_stale_prefix() {
        // The LAN moved to 192.168.1.0/24 but the agent still has 192.168.0.
        let lan_to_lan = flow("192.168.1.20", "192.168.1.30", 1003, 0, 0);
        let mut r = Run::new(&lan_to_lan);
        let stale = r.feed(&flow("192.168.1.20", "192.168.1.30", 1003, 800, 800));
        assert!(stale);
        assert!(r.t.saved.clients.is_empty());
        assert!(!r.feed(&a(100, 1000)), "a client inside the prefix");
    }

    #[test]
    fn prune_drops_idle_clients_and_caps_the_map() {
        let mut s = Saved::default();
        let day = 24 * 3600;
        for (mac, last_seen) in [
            ("old", WALL - 31 * day),
            ("legacy", 0),
            ("recent", WALL - day),
        ] {
            s.clients.insert(
                mac.into(),
                Totals {
                    last_seen,
                    ..Totals::default()
                },
            );
        }
        assert!(!s.clone().prune(1000), "no aging with an unset clock");
        assert!(s.prune(WALL));
        let mut kept: Vec<_> = s.clients.keys().cloned().collect();
        kept.sort();
        assert_eq!(kept, ["legacy", "recent"]);
        assert_eq!(s.clients["legacy"].last_seen, WALL);
        assert!(!s.prune(WALL), "nothing left to change");

        let mut s = Saved::default();
        for i in 0..300u64 {
            s.clients.insert(
                format!("m{i}"),
                Totals {
                    last_seen: WALL - i,
                    ..Totals::default()
                },
            );
        }
        assert!(s.prune(WALL));
        assert_eq!(s.clients.len(), MAX_CLIENTS);
        assert!(s.clients.contains_key("m0") && s.clients.contains_key("m255"));
        assert!(!s.clients.contains_key("m256"));
    }

    #[test]
    fn old_state_file_loads() {
        let old = br#"{"since":"2026-10-01 09:00:00","clients":{"5c:4d:bf:ee:e1:66":{"up":12,"down":34,"ip":"192.168.0.149"}},"extra":1}"#;
        let s = Saved::load(old).unwrap();
        assert_eq!(s.since, "2026-10-01 09:00:00");
        let t = &s.clients[MAC];
        assert_eq!(
            (t.up, t.down, t.ip.as_str(), t.last_seen),
            (12, 34, "192.168.0.149", 0)
        );
        assert!(Saved::load(b"{}").unwrap().clients.is_empty());
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains(r#""last_seen":0"#));
    }
}
