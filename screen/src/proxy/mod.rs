//! The PROXY page: mihomo's groups, nodes, connections and subscriptions,
//! with the few controls the owner asked for.
//!
//! Reads, node choices and delay tests go straight to mihomo's root-only
//! controller socket (`store-selected` keeps a choice across restarts). The
//! mode, restarts and subscription updates belong to zte-agent, which owns
//! mihomo's config, so they are handed to it as a request file
//! (`jobs.rs`, served by agent/src/screen.rs).

mod jobs;
mod page;

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::stats::push;

pub use jobs::{AgentOp, Jobs, Sub};
pub use page::View;

const SOCKET: &str = "/data/mihomo/mihomo.sock";
/// The URL the agent's health checks use too.
pub const HEALTH_URL: &str = "https://www.gstatic.com/generate_204";

/// Set after a switch or a test so the collector re-reads groups at once.
static REFRESH: AtomicBool = AtomicBool::new(false);

pub fn refresh_soon() {
    REFRESH.store(true, Ordering::SeqCst);
}

/// One HTTP/1.0 exchange with mihomo's controller (no secret needed; the
/// socket is root-only on the device). HTTP/1.0 keeps the reply unchunked.
pub fn call(
    method: &str,
    path: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    use std::os::unix::net::UnixStream;
    let mut s = UnixStream::connect(SOCKET).map_err(|e| format!("mihomo: {e}"))?;
    s.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.0\r\nHost: mihomo\r\n");
    if !body.is_empty() {
        req += &format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        );
    }
    req += "\r\n";
    req += &body;
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.take(4 * 1024 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| format!("mihomo: {e}"))?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("mihomo: short reply")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .strip_prefix("HTTP/1.")
        .and_then(|h| h.get(2..5))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("mihomo: bad status line")?;
    Ok((status, raw[split + 4..].to_vec()))
}

/// GET a JSON document; `None` unless mihomo answered 200.
pub fn get(path: &str) -> Option<Value> {
    match call("GET", path, None, Duration::from_millis(1500)) {
        Ok((200, body)) => serde_json::from_slice(&body).ok(),
        _ => None,
    }
}

/// A group or node name as one URL path segment.
pub fn segment(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// mihomo's error text from a reply body, or the status.
pub fn error_text(status: u16, body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("HTTP {status}"))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: String,
    /// Selector, URLTest, Fallback, LoadBalance …
    pub kind: String,
    pub now: String,
    pub all: Vec<String>,
}

impl Group {
    /// Only select groups take a manual choice.
    pub fn manual(&self) -> bool {
        self.kind == "Selector"
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conn {
    pub host: String,
    /// The outbound node, and the group the rule sent it to.
    pub node: String,
    pub group: String,
    pub rule: String,
    pub down_rate: f64,
    pub up_rate: f64,
}

/// What the collector keeps of mihomo while the page is open.
#[derive(Clone, Debug, Default)]
pub struct Live {
    /// mihomo answered the last read.
    pub up: bool,
    pub version: String,
    pub mode: String,
    /// In config order, without GLOBAL and hidden groups.
    pub groups: Vec<Group>,
    /// What GLOBAL points at: everything goes there in global mode.
    pub global_now: String,
    /// Last delay of every proxy and group, in ms; 0 is a timeout.
    pub delays: HashMap<String, u32>,
    pub connections: usize,
    /// Busiest first.
    pub conns: Vec<Conn>,
    pub down_rate: f64,
    pub up_rate: f64,
    pub down_hist: VecDeque<f32>,
    pub up_hist: VecDeque<f32>,
}

impl Live {
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// The node a group ends up on, following groups inside groups.
    pub fn leaf<'a>(&'a self, name: &'a str) -> &'a str {
        let mut at = name;
        for _ in 0..8 {
            match self.group(at) {
                Some(g) if !g.now.is_empty() => at = &g.now,
                _ => break,
            }
        }
        at
    }
}

/// Groups in config order (GLOBAL lists them that way), every last delay,
/// and GLOBAL's choice.
pub fn parse_proxies(v: &Value) -> (Vec<Group>, HashMap<String, u32>, String) {
    let Some(map) = v.get("proxies").and_then(Value::as_object) else {
        return (Vec::new(), HashMap::new(), String::new());
    };
    let global_now = map
        .get("GLOBAL")
        .and_then(|g| g["now"].as_str())
        .unwrap_or("")
        .to_string();
    let mut order: Vec<&str> = map
        .get("GLOBAL")
        .and_then(|g| g.get("all"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut rest: Vec<&str> = map
        .keys()
        .map(String::as_str)
        .filter(|k| !order.contains(k))
        .collect();
    rest.sort_unstable();
    order.extend(rest);
    let mut groups = Vec::new();
    for name in order {
        let Some(p) = map.get(name) else { continue };
        let Some(all) = p.get("all").and_then(Value::as_array) else {
            continue;
        };
        if name == "GLOBAL" || p.get("hidden").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        groups.push(Group {
            name: name.to_string(),
            kind: p["type"].as_str().unwrap_or("").to_string(),
            now: p["now"].as_str().unwrap_or("").to_string(),
            all: all
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        });
    }
    let delays = map
        .iter()
        .filter_map(|(name, p)| {
            let last = p.get("history")?.as_array()?.last()?;
            Some((name.clone(), last.get("delay")?.as_u64()? as u32))
        })
        .collect();
    (groups, delays, global_now)
}

fn conn_of(c: &Value) -> (String, Conn, u64, u64) {
    let md = &c["metadata"];
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let host = [&md["host"], &md["sniffHost"]]
        .into_iter()
        .map(text)
        .find(|h| !h.is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                text(&md["destinationIP"]),
                text(&md["destinationPort"])
            )
        });
    let chains: Vec<String> = c["chains"]
        .as_array()
        .map(|a| a.iter().map(text).collect())
        .unwrap_or_default();
    let payload = text(&c["rulePayload"]);
    let rule = if payload.is_empty() {
        text(&c["rule"])
    } else {
        format!("{}({payload})", text(&c["rule"]))
    };
    (
        text(&c["id"]),
        Conn {
            host,
            node: chains.first().cloned().unwrap_or_default(),
            group: chains.last().cloned().unwrap_or_default(),
            rule,
            ..Conn::default()
        },
        c["download"].as_u64().unwrap_or(0),
        c["upload"].as_u64().unwrap_or(0),
    )
}

/// Reads mihomo for the page once a second: connections and traffic every
/// time, groups every few seconds or right after a change, the mode now and
/// then.
#[derive(Default)]
pub struct Poller {
    n: u64,
    totals: Option<(u64, u64, i64)>,
    seen: HashMap<String, (u64, u64)>,
}

impl Poller {
    /// `fresh`: the page just opened, so read everything now.
    pub fn tick(&mut self, live: &mut Live, now_ms: i64, fresh: bool) {
        let n = self.n;
        self.n += 1;
        let Some(c) = get("/connections") else {
            live.up = false;
            live.conns.clear();
            self.totals = None;
            return;
        };
        live.up = true;
        let (down, up) = (
            c["downloadTotal"].as_u64().unwrap_or(0),
            c["uploadTotal"].as_u64().unwrap_or(0),
        );
        let dt = self
            .totals
            .map(|(_, _, t)| (now_ms - t) as f64 / 1000.0)
            .filter(|dt| *dt > 0.2 && *dt < 10.0);
        if let (Some(dt), Some((d0, u0, _))) = (dt, self.totals) {
            live.down_rate = down.saturating_sub(d0) as f64 / dt;
            live.up_rate = up.saturating_sub(u0) as f64 / dt;
            push(&mut live.down_hist, live.down_rate as f32);
            push(&mut live.up_hist, live.up_rate as f32);
        }
        self.totals = Some((down, up, now_ms));
        let mut seen = HashMap::new();
        let mut conns = Vec::new();
        for raw in c["connections"].as_array().into_iter().flatten() {
            let (id, mut conn, d, u) = conn_of(raw);
            if let (Some(dt), Some((d0, u0))) = (dt, self.seen.get(&id)) {
                conn.down_rate = d.saturating_sub(*d0) as f64 / dt;
                conn.up_rate = u.saturating_sub(*u0) as f64 / dt;
            }
            seen.insert(id, (d, u));
            conns.push(conn);
        }
        self.seen = seen;
        live.connections = conns.len();
        conns.sort_by(|a, b| (b.down_rate + b.up_rate).total_cmp(&(a.down_rate + a.up_rate)));
        conns.truncate(40);
        live.conns = conns;

        if fresh || n % 3 == 0 || REFRESH.swap(false, Ordering::SeqCst) {
            if let Some(p) = get("/proxies") {
                (live.groups, live.delays, live.global_now) = parse_proxies(&p);
            }
        }
        if fresh || n % 5 == 0 {
            if let Some(cfg) = get("/configs") {
                live.mode = cfg["mode"].as_str().unwrap_or("").to_string();
            }
        }
        if live.version.is_empty() || fresh {
            if let Some(v) = get("/version") {
                live.version = v["version"].as_str().unwrap_or("").to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({"proxies": {
            "GLOBAL": {"type": "Selector", "now": "DIRECT", "all": ["节点选择", "香港节点", "Claude", "DIRECT"]},
            "节点选择": {"type": "Selector", "now": "香港节点", "all": ["香港节点", "HK-1", "DIRECT"]},
            "香港节点": {"type": "URLTest", "now": "HK-1", "all": ["HK-1", "HK-2"],
                         "history": [{"delay": 90}]},
            "Claude": {"type": "Selector", "now": "US-1", "all": ["US-1"], "hidden": true},
            "HK-1": {"type": "Vless", "history": [{"delay": 120}, {"delay": 81}]},
            "HK-2": {"type": "Vless", "history": [{"delay": 0}]},
            "US-1": {"type": "Shadowsocks", "history": []},
            "DIRECT": {"type": "Direct", "history": []}
        }})
    }

    #[test]
    fn groups_keep_config_order_and_skip_global_and_hidden() {
        let (groups, delays, global) = parse_proxies(&sample());
        assert_eq!(global, "DIRECT");
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["节点选择", "香港节点"]);
        assert!(groups[0].manual() && !groups[1].manual());
        assert_eq!(delays.get("HK-1"), Some(&81), "the latest test counts");
        assert_eq!(delays.get("HK-2"), Some(&0));
        assert_eq!(delays.get("US-1"), None);
        let live = Live {
            groups,
            ..Live::default()
        };
        assert_eq!(live.leaf("节点选择"), "HK-1");
        assert_eq!(live.leaf("HK-2"), "HK-2");
    }

    #[test]
    fn connections_name_host_node_group_and_rule() {
        let (id, c, d, u) = conn_of(&json!({
            "id": "a", "download": 10, "upload": 4,
            "metadata": {"host": "", "sniffHost": "api.example.com", "destinationIP": "1.2.3.4", "destinationPort": "443"},
            "chains": ["HK-1", "香港节点", "节点选择"], "rule": "RuleSet", "rulePayload": "proxy"
        }));
        assert_eq!((id.as_str(), d, u), ("a", 10, 4));
        assert_eq!(c.host, "api.example.com");
        assert_eq!((c.node.as_str(), c.group.as_str()), ("HK-1", "节点选择"));
        assert_eq!(c.rule, "RuleSet(proxy)");
        let (_, c, _, _) = conn_of(
            &json!({"metadata": {"destinationIP": "1.2.3.4", "destinationPort": "53"}, "chains": ["DIRECT"], "rule": "Match"}),
        );
        assert_eq!((c.host.as_str(), c.rule.as_str()), ("1.2.3.4:53", "Match"));
    }

    #[test]
    fn path_segments_are_percent_encoded() {
        assert_eq!(segment("HK-1"), "HK-1");
        assert_eq!(segment("节点 1/2"), "%E8%8A%82%E7%82%B9%201%2F2");
    }
}
