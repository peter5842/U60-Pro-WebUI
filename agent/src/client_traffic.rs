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

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::storage::atomic_write;
use crate::ubus;
use crate::util::MutexExt;

const CONNTRACK: &str = "/proc/net/nf_conntrack";
const STATE_FILE: &str = "/data/local/tmp/client_traffic.json";
const SAMPLE: Duration = Duration::from_secs(10);
const SAVE_EVERY: Duration = Duration::from_secs(300);
const MAX_READ: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct Totals {
    up: u64,
    down: u64,
    ip: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    since: String,
    clients: HashMap<String, Totals>,
}

#[derive(Default)]
struct Tracker {
    saved: Saved,
    /// flow key → (orig bytes, reply bytes) at the last sample
    flows: HashMap<String, (u64, u64)>,
    /// mac → (up, down) bytes per second over the last sample
    rates: HashMap<String, (u64, u64)>,
    sampled: Option<Instant>,
    dirty: bool,
}

static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);

#[derive(Debug, PartialEq)]
struct Flow {
    key: String,
    mac: String,
    src: String,
    dst: String,
    up: u64,
    down: u64,
}

/// One /proc/net/nf_conntrack line → the client flow, if it is one.
fn parse_line(line: &str) -> Option<Flow> {
    let mut src = None;
    let mut dst = None;
    let mut sport = "";
    let mut dport = "";
    let mut mac = None;
    let mut bytes = Vec::with_capacity(2);
    let proto = line.split_whitespace().nth(2)?;
    for tok in line.split_whitespace() {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "src" if src.is_none() => src = Some(v),
            "dst" if dst.is_none() => dst = Some(v),
            "sport" if sport.is_empty() => sport = v,
            "dport" if dport.is_empty() => dport = v,
            "src_mac" if mac.is_none() => mac = Some(v),
            "bytes" => bytes.push(v.parse::<u64>().ok()?),
            _ => {}
        }
    }
    let (src, dst, mac) = (src?, dst?, mac?);
    if mac == "00:00:00:00:00:00" || bytes.len() < 2 {
        return None;
    }
    Some(Flow {
        key: format!("{proto}|{src}|{dst}|{sport}|{dport}"),
        mac: mac.to_lowercase(),
        src: src.to_string(),
        dst: dst.to_string(),
        up: bytes[0],
        down: bytes[1],
    })
}

/// Is `ip` inside the LAN (`prefix` like "192.168.0.") or not routable?
fn local_destination(ip: &str, lan_prefix: &str) -> bool {
    ip.starts_with(lan_prefix)
        || ip.starts_with("224.")
        || ip.starts_with("239.")
        || ip == "255.255.255.255"
        || ip.starts_with("fe80:")
        || ip.starts_with("ff")
}

impl Tracker {
    /// Fold one conntrack snapshot in. Returns the seconds since the last one.
    fn ingest(&mut self, text: &str, lan_prefix: &str, now: Instant) {
        let elapsed = self
            .sampled
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        let mut seen: HashMap<String, (u64, u64)> = HashMap::new();
        let mut delta: HashMap<String, (u64, u64)> = HashMap::new();
        for flow in text.lines().filter_map(parse_line) {
            if local_destination(&flow.dst, lan_prefix) {
                continue;
            }
            let (pu, pd) = self.flows.get(&flow.key).copied().unwrap_or((0, 0));
            // A shrinking counter is a new flow reusing the tuple.
            let du = if flow.up >= pu { flow.up - pu } else { flow.up };
            let dd = if flow.down >= pd {
                flow.down - pd
            } else {
                flow.down
            };
            let d = delta.entry(flow.mac.clone()).or_default();
            d.0 += du;
            d.1 += dd;
            let t = self.saved.clients.entry(flow.mac.clone()).or_default();
            t.up += du;
            t.down += dd;
            if flow.src.contains('.') {
                t.ip = flow.src.clone();
            }
            seen.insert(flow.key, (flow.up, flow.down));
        }
        self.flows = seen;
        self.rates = if elapsed > 0.5 {
            delta
                .into_iter()
                .map(|(mac, (u, d))| {
                    (
                        mac,
                        ((u as f64 / elapsed) as u64, (d as f64 / elapsed) as u64),
                    )
                })
                .collect()
        } else {
            HashMap::new()
        };
        self.sampled = Some(now);
        self.dirty = true;
    }
}

fn lan_prefix() -> String {
    ubus::call(
        "uci",
        "get",
        Some(r#"{"config":"network","section":"lan","option":"ipaddr"}"#),
    )
    .ok()
    .and_then(|v| v["value"].as_str().map(str::to_string))
    .and_then(|ip| ip.rsplit_once('.').map(|(net, _)| format!("{net}.")))
    .unwrap_or_else(|| "192.168.0.".into())
}

fn now_text() -> String {
    ubus::call("zwrt_sntp", "get_systime", Some("{}"))
        .ok()
        .and_then(|v| v["localtime"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn read_conntrack() -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(CONNTRACK)
        .ok()?
        .take(MAX_READ)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn save(saved: &Saved) {
    if let Ok(bytes) = serde_json::to_vec(saved) {
        let _ = atomic_write(Path::new(STATE_FILE), &bytes);
    }
}

pub fn start() {
    let saved: Saved = std::fs::read(STATE_FILE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| Saved {
            since: now_text(),
            ..Saved::default()
        });
    *TRACKER.safe_lock() = Some(Tracker {
        saved,
        ..Tracker::default()
    });
    std::thread::spawn(|| {
        let prefix = lan_prefix();
        let mut saved_at = Instant::now();
        loop {
            if let Some(text) = read_conntrack() {
                let mut guard = TRACKER.safe_lock();
                if let Some(t) = guard.as_mut() {
                    // The first pass only learns the flows that already exist.
                    let first = t.sampled.is_none();
                    let before = t.saved.clients.clone();
                    t.ingest(&text, &prefix, Instant::now());
                    if first {
                        t.saved.clients = before;
                        t.rates.clear();
                    }
                    if t.dirty && saved_at.elapsed() >= SAVE_EVERY {
                        save(&t.saved);
                        t.dirty = false;
                        saved_at = Instant::now();
                    }
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

    #[test]
    fn parses_client_flows_only() {
        let f = parse_line(LINE).unwrap();
        assert_eq!(f.mac, "5c:4d:bf:ee:e1:66");
        assert_eq!((f.up, f.down), (649_403, 40_732_680));
        let router_own = LINE.replace("src_mac=5c:4d:bf:ee:e1:66", "src_mac=00:00:00:00:00:00");
        assert!(parse_line(&router_own).is_none());
        assert!(parse_line("garbage").is_none());
    }

    #[test]
    fn accumulates_growth_and_skips_lan_traffic() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.ingest(LINE, "192.168.0.", t0);
        let grown = LINE.replace("bytes=40732680", "bytes=41732680");
        t.ingest(&grown, "192.168.0.", t0 + Duration::from_secs(10));
        let tot = &t.saved.clients["5c:4d:bf:ee:e1:66"];
        assert_eq!(tot.down, 41_732_680);
        assert_eq!(tot.ip, "192.168.0.149");
        assert_eq!(t.rates["5c:4d:bf:ee:e1:66"].1, 100_000);
        let lan = LINE.replace(
            "dst=39.130.176.204 sport=60570",
            "dst=192.168.0.1 sport=60570",
        );
        let mut l = Tracker::default();
        l.ingest(&lan, "192.168.0.", t0);
        assert!(l.saved.clients.is_empty());
    }
}
