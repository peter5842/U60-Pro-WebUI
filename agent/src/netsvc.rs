//! Router network services, through the stock `zwrt_router.api` calls and
//! read back from uci like the stock web UI:
//!
//! - connection watchdog (`router_set_watchdog`; the firmware turns it back
//!   off when it cannot ping the address, so a save is confirmed after 10 s),
//! - UPnP, DMZ, remote management and WAN ping,
//! - port forwarding (a port range to the same ports on a LAN host) and port
//!   mapping (one external port to a different internal port),
//! - static DHCP (MAC-IP binding; the firmware applies it after a reboot),
//! - the clock status (read only: the clock holds local time, see AGENTS.md).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::handlers::AppState;
use crate::ubus;

const ROUTER: &str = "zwrt_router.api";
const MAX_PORT_RULES: usize = 20;
const MAX_BINDINGS: usize = 10;
const WATCHDOG_SETTLE: Duration = Duration::from_secs(10);

type Reply = (u16, Value);

fn ok(data: Value) -> Reply {
    (200, json!({"ok": true, "data": data}))
}
fn bad(msg: impl Into<String>) -> Reply {
    (400, json!({"ok": false, "error": msg.into()}))
}
fn unavailable(msg: impl Into<String>) -> Reply {
    (503, json!({"ok": false, "error": msg.into()}))
}

fn parse_body(body: &[u8]) -> Result<Map<String, Value>, Reply> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err(bad("expected a JSON object")),
    }
}

fn only_keys(obj: &Map<String, Value>, allowed: &[&str]) -> Result<(), Reply> {
    match obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(bad(format!("unknown field {k}"))),
        None => Ok(()),
    }
}

fn router(method: &str, params: Value) -> Result<Value, String> {
    ubus::call(ROUTER, method, Some(&params.to_string()))
}

/// `uci get` through ubus: one section's options.
fn uci_section(config: &str, section: &str) -> Result<Map<String, Value>, String> {
    let v = ubus::call(
        "uci",
        "get",
        Some(&json!({"config": config, "section": section}).to_string()),
    )?;
    Ok(v["values"].as_object().cloned().unwrap_or_default())
}

/// `uci get` through ubus: every section of a type, keyed by its real name
/// (the id the stock delete calls take).
fn uci_sections(config: &str, kind: &str) -> Result<BTreeMap<String, Value>, String> {
    let v = ubus::call(
        "uci",
        "get",
        Some(&json!({"config": config, "type": kind}).to_string()),
    )?;
    Ok(v["values"]
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default())
}

fn opt(m: &Map<String, Value>, key: &str) -> String {
    match m.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

fn flag(m: &Map<String, Value>, key: &str) -> bool {
    opt(m, key) == "1"
}

fn bool_field(obj: &Map<String, Value>, key: &str) -> Result<Option<bool>, Reply> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("{key} must be a boolean"))),
    }
}

fn uint_field(
    obj: &Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
) -> Result<Option<u64>, Reply> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| (min..=max).contains(n))
            .map(Some)
            .ok_or_else(|| bad(format!("{key} must be {min}-{max}"))),
    }
}

fn str_field<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, Reply> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(bad(format!("{key} must be a string"))),
    }
}

/// A uci section id as the firmware hands it out (`cfg0d92bd` or a name).
fn valid_section_id(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// ── LAN addressing ───────────────────────────────────────────────────────────

struct Lan {
    ip: Ipv4Addr,
    mask: Ipv4Addr,
}

fn lan() -> Result<Lan, String> {
    let m = uci_section("network", "lan")?;
    let ip = opt(&m, "ipaddr")
        .parse()
        .map_err(|_| "LAN address unreadable")?;
    let mask = opt(&m, "netmask")
        .parse()
        .map_err(|_| "LAN netmask unreadable")?;
    Ok(Lan { ip, mask })
}

/// A LAN host address the stock UI would accept: inside the LAN subnet, not
/// the router, the network or the broadcast address.
fn lan_host(text: &str, lan: &Lan) -> Result<Ipv4Addr, String> {
    let ip: Ipv4Addr = text
        .trim()
        .parse()
        .map_err(|_| "enter an IPv4 address such as 192.168.0.20".to_string())?;
    let (ip_n, lan_n, mask_n) = (u32::from(ip), u32::from(lan.ip), u32::from(lan.mask));
    let o = ip.octets();
    if ip_n & mask_n != lan_n & mask_n {
        return Err(format!("{ip} is not in the LAN ({}/{})", lan.ip, lan.mask));
    }
    if ip == lan.ip {
        return Err("that is the router's own address".into());
    }
    if ip_n & !mask_n == 0
        || ip_n & !mask_n == !mask_n
        || !(1..=223).contains(&o[0])
        || o[3] == 0
        || o[3] == 255
    {
        return Err(format!("{ip} cannot be used for a device"));
    }
    Ok(ip)
}

// ── Connection watchdog ──────────────────────────────────────────────────────

fn valid_ping_target(host: &str) -> bool {
    if host.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    // All-numeric names are mistyped addresses, not domains.
    if labels.iter().all(|l| l.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
}

fn watchdog_view() -> Result<Value, String> {
    let m = uci_section("zwrt_router", "watchdog")?;
    Ok(json!({
        "enabled": flag(&m, "enable"),
        "host": Some(opt(&m, "url")).filter(|s| !s.is_empty()),
        "interval_minutes": opt(&m, "time_gap").parse::<u64>().ok(),
        "failures": opt(&m, "ping_times").parse::<u64>().ok(),
    }))
}

/// GET /api/router/watchdog
pub fn watchdog_get(_state: &AppState) -> Reply {
    watchdog_view().map_or_else(unavailable, ok)
}

/// PUT /api/router/watchdog — {enabled, host?, interval_minutes?, failures?}
pub fn watchdog_set(_state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    if let Err(e) = only_keys(&obj, &["enabled", "host", "interval_minutes", "failures"]) {
        return e;
    }
    let enabled = match bool_field(&obj, "enabled") {
        Ok(Some(b)) => b,
        Ok(None) => return bad("enabled is required"),
        Err(e) => return e,
    };
    if !enabled {
        return match router("router_set_watchdog", json!({"enable": 0})) {
            Ok(_) => watchdog_get(_state),
            Err(e) => unavailable(e),
        };
    }
    let current = match watchdog_view() {
        Ok(v) => v,
        Err(e) => return unavailable(e),
    };
    let host = match str_field(&obj, "host") {
        Ok(Some(h)) => h.trim().to_string(),
        Ok(None) => current["host"].as_str().unwrap_or_default().to_string(),
        Err(e) => return e,
    };
    if !valid_ping_target(&host) {
        return bad("host must be an IPv4 address or a domain name");
    }
    let interval = match uint_field(&obj, "interval_minutes", 2, 1440) {
        Ok(v) => v
            .or(current["interval_minutes"].as_u64())
            .unwrap_or(5)
            .clamp(2, 1440),
        Err(e) => return e,
    };
    let failures = match uint_field(&obj, "failures", 1, 20) {
        Ok(v) => v.or(current["failures"].as_u64()).unwrap_or(3).clamp(1, 20),
        Err(e) => return e,
    };
    let params = json!({"enable": 1, "url": host, "ping_times": failures, "time_gap": interval});
    if let Err(e) = router("router_set_watchdog", params) {
        return unavailable(e);
    }
    // The firmware pings the address and switches the watchdog back off when
    // it gets no answer (stock UI: re-read after 10 s).
    std::thread::sleep(WATCHDOG_SETTLE);
    match watchdog_view() {
        Ok(v) if v["enabled"] == true => ok(v),
        Ok(_) => (
            409,
            json!({"ok": false, "error": format!("the router could not ping {host}, so the watchdog stayed off")}),
        ),
        Err(e) => unavailable(e),
    }
}

// ── UPnP, DMZ, remote management, WAN ping ───────────────────────────────────

fn firewall_view() -> Result<Value, String> {
    let fw = uci_section("zwrt_router", "firewall")?;
    let upnp = uci_section("upnpd", "config").unwrap_or_default();
    Ok(json!({
        "upnp": flag(&upnp, "enabled") && flag(&upnp, "enable_upnp"),
        "dmz_enabled": flag(&fw, "dmz_enable"),
        "dmz_ip": Some(opt(&fw, "dmz_ip")).filter(|s| !s.is_empty()),
        "remote_web_access": flag(&fw, "remote_web_access_enable"),
        "wan_ping": flag(&fw, "wan_ping_enable"),
    }))
}

/// GET /api/router/firewall
pub fn firewall_get(_state: &AppState) -> Reply {
    firewall_view().map_or_else(unavailable, ok)
}

/// PUT /api/router/firewall — any of {upnp, dmz_enabled, dmz_ip,
/// remote_web_access, wan_ping}
pub fn firewall_set(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    if let Err(e) = only_keys(
        &obj,
        &[
            "upnp",
            "dmz_enabled",
            "dmz_ip",
            "remote_web_access",
            "wan_ping",
        ],
    ) {
        return e;
    }
    if obj.is_empty() {
        return bad("nothing to change");
    }
    let current = match firewall_view() {
        Ok(v) => v,
        Err(e) => return unavailable(e),
    };
    let field = |k: &str| bool_field(&obj, k);
    let (upnp, dmz_on, remote, ping) = match (
        field("upnp"),
        field("dmz_enabled"),
        field("remote_web_access"),
        field("wan_ping"),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (_, _, _, Err(e)) => return e,
    };
    let dmz_ip = match str_field(&obj, "dmz_ip") {
        Ok(v) => v.map(str::to_string),
        Err(e) => return e,
    };

    // Validate everything before the first write.
    let mut dmz_call = None;
    if dmz_on.is_some() || dmz_ip.is_some() {
        let on = dmz_on.unwrap_or(current["dmz_enabled"] == true);
        if on {
            let ip_text = dmz_ip
                .or_else(|| current["dmz_ip"].as_str().map(str::to_string))
                .unwrap_or_default();
            let lan = match lan() {
                Ok(l) => l,
                Err(e) => return unavailable(e),
            };
            match lan_host(&ip_text, &lan) {
                Ok(ip) => dmz_call = Some(json!({"dmz_enable": 1, "dmz_ip": ip.to_string()})),
                Err(e) => return bad(format!("DMZ: {e}")),
            }
        } else {
            dmz_call = Some(json!({"dmz_enable": 0}));
        }
    }
    if let Some(u) = upnp {
        if let Err(e) = router(
            "router_set_upnp_switch",
            json!({"enable_upnp": i32::from(u)}),
        ) {
            return unavailable(e);
        }
    }
    if let Some(p) = dmz_call {
        if let Err(e) = router("router_set_dmz", p) {
            return unavailable(e);
        }
    }
    if remote.is_some() || ping.is_some() {
        let params = json!({
            "remote_web_access_enable": i32::from(remote.unwrap_or(current["remote_web_access"] == true)),
            "wan_ping_enable": i32::from(ping.unwrap_or(current["wan_ping"] == true)),
        });
        if let Err(e) = router("router_set_remote_acl", params) {
            return unavailable(e);
        }
    }
    firewall_get(state)
}

// ── Port forwarding and port mapping ─────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Forward,
    Mapping,
}

impl Kind {
    fn parse(v: Option<&Value>) -> Result<Self, Reply> {
        match v.and_then(Value::as_str) {
            Some("forward") => Ok(Kind::Forward),
            Some("mapping") => Ok(Kind::Mapping),
            _ => Err(bad("kind must be forward or mapping")),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Kind::Forward => "forward",
            Kind::Mapping => "mapping",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Kind::Forward => "router_set_portforward",
            Kind::Mapping => "router_set_portmapping",
        }
    }
}

/// "a-b" or "a" → (a, b)
fn port_range(text: &str) -> Option<(u16, u16)> {
    let (a, b) = text.split_once('-').unwrap_or((text, text));
    let a: u16 = a.trim().parse().ok()?;
    let b: u16 = b.trim().parse().ok()?;
    (a >= 1 && a <= b).then_some((a, b))
}

fn proto_name(text: &str) -> &'static str {
    let tcp = text.split_whitespace().any(|p| p == "tcp" || p == "tcpudp");
    let udp = text.split_whitespace().any(|p| p == "udp" || p == "tcpudp");
    match (tcp, udp) {
        (true, false) => "tcp",
        (false, true) => "udp",
        _ => "both",
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PortRule {
    id: String,
    kind: Kind,
    ip: String,
    /// External port range.
    external: (u16, u16),
    /// Mapping only: the internal port.
    internal: Option<u16>,
    proto: &'static str,
    comment: String,
}

impl PortRule {
    fn view(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind.name(),
            "ip": self.ip,
            "external_start": self.external.0,
            "external_end": self.external.1,
            "internal": self.internal,
            "proto": self.proto,
            "comment": self.comment,
        })
    }
    fn overlaps(&self, other: &PortRule) -> bool {
        let shares_proto =
            self.proto == "both" || other.proto == "both" || self.proto == other.proto;
        shares_proto && self.external.0 <= other.external.1 && other.external.0 <= self.external.1
    }
}

fn read_port_rules() -> Result<Vec<PortRule>, String> {
    let sections = uci_sections("firewall", "redirect")?;
    let mut out = Vec::new();
    for (id, v) in sections {
        let Some(m) = v.as_object() else { continue };
        let kind = match opt(m, "src").as_str() {
            "portforward" => Kind::Forward,
            "portmapping" => Kind::Mapping,
            _ => continue,
        };
        let Some(external) = port_range(&opt(m, "src_dport")) else {
            continue;
        };
        out.push(PortRule {
            id,
            kind,
            ip: opt(m, "dest_ip"),
            external: if kind == Kind::Mapping {
                (external.0, external.0)
            } else {
                external
            },
            internal: (kind == Kind::Mapping)
                .then(|| port_range(&opt(m, "dest_port")).map(|r| r.0))
                .flatten(),
            proto: proto_name(&opt(m, "proto")),
            comment: opt(m, "name"),
        });
    }
    Ok(out)
}

fn port_rules_view() -> Result<Value, String> {
    let fw = uci_section("zwrt_router", "firewall")?;
    let rules = read_port_rules()?;
    Ok(json!({
        "forward_enabled": flag(&fw, "portforward_enable"),
        "mapping_enabled": flag(&fw, "portmapping_enable"),
        "max_per_kind": MAX_PORT_RULES,
        "rules": rules.iter().map(PortRule::view).collect::<Vec<_>>(),
    }))
}

/// GET /api/router/port-forwards
pub fn port_rules_get(_state: &AppState) -> Reply {
    port_rules_view().map_or_else(unavailable, ok)
}

/// PUT /api/router/port-forwards — {forward_enabled?, mapping_enabled?}
pub fn port_rules_switch(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    if let Err(e) = only_keys(&obj, &["forward_enabled", "mapping_enabled"]) {
        return e;
    }
    for (key, method, param) in [
        (
            "forward_enabled",
            "router_set_portforward_switch",
            "portforward_enable",
        ),
        (
            "mapping_enabled",
            "router_set_portmapping_switch",
            "portmapping_enable",
        ),
    ] {
        match bool_field(&obj, key) {
            Ok(Some(on)) => {
                if let Err(e) = router(method, json!({ param: i32::from(on) })) {
                    return unavailable(e);
                }
            }
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    port_rules_get(state)
}

fn valid_comment(c: &str) -> bool {
    (1..=32).contains(&c.len())
        && c.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "!#()+-./%=?@^_{|}~".contains(ch))
}

fn plan_port_rule(
    obj: &Map<String, Value>,
    lan: &Lan,
    existing: &[PortRule],
) -> Result<(Kind, Value), Reply> {
    only_keys(
        obj,
        &[
            "kind",
            "ip",
            "proto",
            "comment",
            "external_start",
            "external_end",
            "internal",
        ],
    )?;
    let kind = Kind::parse(obj.get("kind"))?;
    let ip = lan_host(str_field(obj, "ip")?.unwrap_or_default(), lan).map_err(bad)?;
    let proto = match str_field(obj, "proto")? {
        Some("tcp") => "tcp",
        Some("udp") => "udp",
        Some("both") | None => "both",
        Some(_) => return Err(bad("proto must be tcp, udp or both")),
    };
    let comment = str_field(obj, "comment")?
        .unwrap_or_default()
        .trim()
        .to_string();
    if !valid_comment(&comment) {
        return Err(bad(
            "comment must be 1-32 characters: letters, digits and !#()+-./%=?@^_{|}~",
        ));
    }
    let (max_port, reserved) = match kind {
        Kind::Forward => (65535, false),
        Kind::Mapping => (65000, true),
    };
    let start = uint_field(obj, "external_start", 1, max_port)?
        .ok_or_else(|| bad("external_start is required"))? as u16;
    let end = match kind {
        Kind::Forward => {
            uint_field(obj, "external_end", 1, max_port)?.unwrap_or(u64::from(start)) as u16
        }
        Kind::Mapping => start,
    };
    if end < start {
        return Err(bad("the port range ends before it starts"));
    }
    let internal = match kind {
        Kind::Forward => None,
        Kind::Mapping => Some(
            uint_field(obj, "internal", 1, max_port)?.ok_or_else(|| bad("internal is required"))?
                as u16,
        ),
    };
    if reserved {
        for p in [Some(start), internal].into_iter().flatten() {
            if (32000..=32007).contains(&p) {
                return Err(bad("ports 32000-32007 are reserved by the firmware"));
            }
        }
    }
    if existing.iter().filter(|r| r.kind == kind).count() >= MAX_PORT_RULES {
        return Err(bad(format!("at most {MAX_PORT_RULES} rules of this kind")));
    }
    let candidate = PortRule {
        id: String::new(),
        kind,
        ip: ip.to_string(),
        external: (start, end),
        internal,
        proto,
        comment: comment.clone(),
    };
    if let Some(clash) = existing.iter().find(|r| r.overlaps(&candidate)) {
        return Err((
            409,
            json!({"ok": false, "error": format!(
                "external port {}-{} overlaps the rule \"{}\"",
                clash.external.0, clash.external.1, clash.comment
            )}),
        ));
    }
    let wire_proto = match proto {
        "both" => "tcp udp",
        p => p,
    };
    let params = match kind {
        Kind::Forward => json!({
            "action": "add",
            "dest_ip": ip.to_string(),
            "src_dport": format!("{start}-{end}"),
            "proto": wire_proto,
            "comment": comment,
            "enabled": 1,
        }),
        Kind::Mapping => {
            let internal = internal.unwrap_or(start);
            json!({
                "action": "add",
                "src_dport": format!("{start}-{start}"),
                "dest_port": format!("{internal}-{internal}"),
                "dest_ip": ip.to_string(),
                "proto": wire_proto,
                "comment": comment,
                "enabled": 1,
            })
        }
    };
    Ok((kind, params))
}

/// POST /api/router/port-forwards — add one rule.
pub fn port_rule_add(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (lan, existing) = match (lan(), read_port_rules()) {
        (Ok(l), Ok(r)) => (l, r),
        (Err(e), _) | (_, Err(e)) => return unavailable(e),
    };
    let (kind, params) = match plan_port_rule(&obj, &lan, &existing) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Err(e) = router(kind.method(), params) {
        return unavailable(e);
    }
    port_rules_get(state)
}

/// POST /api/router/port-forwards/delete — {kind, id}
pub fn port_rule_delete(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    if let Err(e) = only_keys(&obj, &["kind", "id"]) {
        return e;
    }
    let kind = match Kind::parse(obj.get("kind")) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let Some(id) = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_section_id(id))
    else {
        return bad("id must be a rule id");
    };
    match read_port_rules() {
        Ok(rules) if rules.iter().any(|r| r.id == id && r.kind == kind) => {}
        Ok(_) => return (404, json!({"ok": false, "error": "no such rule"})),
        Err(e) => return unavailable(e),
    }
    if let Err(e) = router(
        kind.method(),
        json!({"action": "delete", "section_id": [id]}),
    ) {
        return unavailable(e);
    }
    port_rules_get(state)
}

// ── Static DHCP (MAC-IP binding) ─────────────────────────────────────────────

fn read_bindings() -> Result<Vec<Value>, String> {
    Ok(uci_sections("dhcp", "host")?
        .into_iter()
        .filter_map(|(id, v)| {
            let m = v.as_object()?;
            let mac = crate::clients::normalize_mac(&opt(m, "mac"))?;
            Some(json!({"id": id, "mac": mac, "ip": opt(m, "ip"), "name": Some(opt(m, "name")).filter(|s| !s.is_empty())}))
        })
        .collect())
}

fn bindings_view() -> Result<Value, String> {
    let net = uci_section("zwrt_router", "network")?;
    let lan = lan()?;
    Ok(json!({
        "enabled": flag(&net, "mac_ip_bond_enable"),
        "max": MAX_BINDINGS,
        "lan_ip": lan.ip.to_string(),
        "netmask": lan.mask.to_string(),
        "bindings": read_bindings()?,
    }))
}

/// GET /api/router/dhcp-bindings
pub fn bindings_get(_state: &AppState) -> Reply {
    bindings_view().map_or_else(unavailable, ok)
}

/// PUT /api/router/dhcp-bindings — {enabled}
pub fn bindings_switch(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    if let Err(e) = only_keys(&obj, &["enabled"]) {
        return e;
    }
    let on = match bool_field(&obj, "enabled") {
        Ok(Some(b)) => b,
        Ok(None) => return bad("enabled is required"),
        Err(e) => return e,
    };
    if let Err(e) = router(
        "router_set_mac_ip_bond_switch",
        json!({"mac_ip_bond_enable": i32::from(on)}),
    ) {
        return unavailable(e);
    }
    bindings_get(state)
}

fn plan_binding(obj: &Map<String, Value>, lan: &Lan, existing: &[Value]) -> Result<Value, Reply> {
    only_keys(obj, &["mac", "ip"])?;
    let mac = str_field(obj, "mac")?
        .and_then(crate::clients::normalize_mac)
        .ok_or_else(|| bad("mac must be a MAC address"))?;
    let first = u8::from_str_radix(&mac[..2], 16).unwrap_or(1);
    if first & 1 == 1 || mac == "00:00:00:00:00:00" {
        return Err(bad("that is not a device (unicast) MAC address"));
    }
    let ip = lan_host(str_field(obj, "ip")?.unwrap_or_default(), lan).map_err(bad)?;
    if existing.len() >= MAX_BINDINGS {
        return Err(bad(format!("at most {MAX_BINDINGS} bindings")));
    }
    if existing.iter().any(|b| b["mac"] == mac.as_str()) {
        return Err((
            409,
            json!({"ok": false, "error": "that device already has a fixed address"}),
        ));
    }
    if existing.iter().any(|b| b["ip"] == ip.to_string().as_str()) {
        return Err((
            409,
            json!({"ok": false, "error": format!("{ip} is already bound to another device")}),
        ));
    }
    Ok(json!({"action": "add", "mac": mac, "ip": ip.to_string(), "name": "", "enable": 1}))
}

/// POST /api/router/dhcp-bindings — {mac, ip}
pub fn binding_add(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (lan, existing) = match (lan(), read_bindings()) {
        (Ok(l), Ok(b)) => (l, b),
        (Err(e), _) | (_, Err(e)) => return unavailable(e),
    };
    let params = match plan_binding(&obj, &lan, &existing) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Err(e) = router("router_set_mac_ip_bind", params) {
        return unavailable(e);
    }
    bindings_get(state)
}

/// POST /api/router/dhcp-bindings/delete — {id}
pub fn binding_delete(state: &AppState, body: &[u8]) -> Reply {
    let obj = match parse_body(body) {
        Ok(o) => o,
        Err(e) => return e,
    };
    if let Err(e) = only_keys(&obj, &["id"]) {
        return e;
    }
    let Some(id) = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_section_id(id))
    else {
        return bad("id must be a binding id");
    };
    match read_bindings() {
        Ok(b) if b.iter().any(|x| x["id"] == id) => {}
        Ok(_) => return (404, json!({"ok": false, "error": "no such binding"})),
        Err(e) => return unavailable(e),
    }
    if let Err(e) = router(
        "router_set_mac_ip_bind",
        json!({"action": "delete", "section_id": [id]}),
    ) {
        return unavailable(e);
    }
    bindings_get(state)
}

// ── Clock status (read only) ─────────────────────────────────────────────────

/// GET /api/system/time
pub fn time_get(_state: &AppState) -> Reply {
    let settings = uci_section("zwrt_zte_sntp", "settings").unwrap_or_default();
    let status = uci_section("zwrt_zte_sntp", "status").unwrap_or_default();
    let local = ubus::call("zwrt_sntp", "get_systime", Some("{}"))
        .ok()
        .and_then(|v| v["localtime"].as_str().map(str::to_string));
    let servers: Vec<String> = (0..3)
        .map(|i| opt(&settings, &format!("server{i}")))
        .filter(|s| !s.is_empty() && s != "Other")
        .collect();
    let source = Some(opt(&status, "systime_mode")).filter(|s| !s.is_empty());
    ok(json!({
        "local_time": local,
        "utc_offset_hours": opt(&settings, "time_from_utc").parse::<f64>().ok(),
        "mode": Some(opt(&settings, "time_set_mode")).filter(|s| !s.is_empty()),
        "source": source,
        "sntp_synced": flag(&status, "syn_done"),
        "servers": servers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lan() -> Lan {
        Lan {
            ip: Ipv4Addr::new(192, 168, 0, 1),
            mask: Ipv4Addr::new(255, 255, 255, 0),
        }
    }

    #[test]
    fn lan_hosts() {
        assert!(lan_host("192.168.0.20", &lan()).is_ok());
        assert!(lan_host("192.168.0.1", &lan()).is_err());
        assert!(lan_host("192.168.0.255", &lan()).is_err());
        assert!(lan_host("192.168.0.0", &lan()).is_err());
        assert!(lan_host("192.168.1.20", &lan()).is_err());
        assert!(lan_host("garbage", &lan()).is_err());
    }

    #[test]
    fn ping_targets() {
        assert!(valid_ping_target("223.5.5.5"));
        assert!(valid_ping_target("www.baidu.com"));
        assert!(!valid_ping_target("localhost"));
        assert!(!valid_ping_target("a b.com"));
        assert!(!valid_ping_target("-x.com"));
        assert!(!valid_ping_target("999.1.1.1"));
    }

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn port_rules_are_validated_and_rendered_like_the_stock_ui() {
        let (kind, p) = plan_port_rule(
            &obj(json!({"kind": "forward", "ip": "192.168.0.20", "external_start": 8000, "external_end": 8010, "comment": "nas"})),
            &lan(),
            &[],
        )
        .unwrap();
        assert_eq!(kind, Kind::Forward);
        assert_eq!(p["src_dport"], "8000-8010");
        assert_eq!(p["proto"], "tcp udp");
        let (_, m) = plan_port_rule(
            &obj(json!({"kind": "mapping", "ip": "192.168.0.20", "external_start": 2222, "internal": 22, "proto": "tcp", "comment": "ssh"})),
            &lan(),
            &[],
        )
        .unwrap();
        assert_eq!(m["src_dport"], "2222-2222");
        assert_eq!(m["dest_port"], "22-22");
        assert_eq!(m["proto"], "tcp");
        for bad_rule in [
            json!({"kind": "forward", "ip": "192.168.0.20", "external_start": 10, "external_end": 5, "comment": "x"}),
            json!({"kind": "mapping", "ip": "192.168.0.20", "external_start": 32001, "internal": 22, "comment": "x"}),
            json!({"kind": "forward", "ip": "192.168.0.20", "external_start": 80, "comment": "has space"}),
            json!({"kind": "forward", "ip": "10.0.0.2", "external_start": 80, "comment": "x"}),
            json!({"kind": "other", "ip": "192.168.0.20", "external_start": 80, "comment": "x"}),
        ] {
            assert!(
                plan_port_rule(&obj(bad_rule.clone()), &lan(), &[]).is_err(),
                "{bad_rule}"
            );
        }
        let existing = PortRule {
            id: "cfg1".into(),
            kind: Kind::Mapping,
            ip: "192.168.0.30".into(),
            external: (8005, 8005),
            internal: Some(80),
            proto: "tcp",
            comment: "web".into(),
        };
        let clash = plan_port_rule(
            &obj(
                json!({"kind": "forward", "ip": "192.168.0.20", "external_start": 8000, "external_end": 8010, "proto": "tcp", "comment": "nas"}),
            ),
            &lan(),
            &[existing.clone()],
        );
        assert_eq!(clash.unwrap_err().0, 409);
        // Different protocols do not clash.
        assert!(plan_port_rule(
            &obj(json!({"kind": "forward", "ip": "192.168.0.20", "external_start": 8005, "proto": "udp", "comment": "game"})),
            &lan(),
            &[existing],
        )
        .is_ok());
    }

    #[test]
    fn port_helpers() {
        assert_eq!(port_range("100-200"), Some((100, 200)));
        assert_eq!(port_range("80"), Some((80, 80)));
        assert_eq!(port_range("200-100"), None);
        assert_eq!(proto_name("tcp udp"), "both");
        assert_eq!(proto_name("udp"), "udp");
        assert_eq!(proto_name("tcpudp"), "both");
    }

    #[test]
    fn bindings_are_validated() {
        let existing =
            vec![json!({"id": "cfg1", "mac": "02:00:00:00:00:01", "ip": "192.168.0.50"})];
        let p = plan_binding(
            &obj(json!({"mac": "02-00-00-00-00-02", "ip": "192.168.0.51"})),
            &lan(),
            &existing,
        )
        .unwrap();
        assert_eq!(p["mac"], "02:00:00:00:00:02");
        assert_eq!(p["action"], "add");
        assert!(plan_binding(
            &obj(json!({"mac": "01:00:5E:00:00:01", "ip": "192.168.0.52"})),
            &lan(),
            &existing
        )
        .is_err());
        assert_eq!(
            plan_binding(
                &obj(json!({"mac": "02:00:00:00:00:01", "ip": "192.168.0.52"})),
                &lan(),
                &existing
            )
            .unwrap_err()
            .0,
            409
        );
        assert_eq!(
            plan_binding(
                &obj(json!({"mac": "02:00:00:00:00:03", "ip": "192.168.0.50"})),
                &lan(),
                &existing
            )
            .unwrap_err()
            .0,
            409
        );
        assert!(plan_binding(
            &obj(json!({"mac": "02:00:00:00:00:03", "ip": "192.168.0.1"})),
            &lan(),
            &existing
        )
        .is_err());
    }
}
