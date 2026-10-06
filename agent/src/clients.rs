//! Per-client controls, through the same calls as the stock web UI:
//!
//! - Names: `zwrt_router.api router_modify_lan_hostname {mac, hostname}`; the
//!   overrides are read back with `router_get_modified_lan_hostname`.
//! - Disconnect a Wi-Fi client: `zwrt_wlan kick_macs {macs}`.
//! - Block list: the Wi-Fi MAC filter in deny mode. `zwrt_wlan set` writes
//!   `denymaclist` on every access point and hostapd applies it live (no Wi-Fi
//!   restart). The firmware mirrors it into `maclist`, which is what
//!   `/lib/wifi/zteqcawifi.sh` builds the deny file from at Wi-Fi start; that
//!   copy can lag, and it is checked after each change. Blocking stops Wi-Fi
//!   association only; USB and Ethernet clients are not affected.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::handlers::AppState;
use crate::process::BoundedCommand;
use crate::ubus;
use crate::util::MutexExt;

const AP_SECTIONS: [&str; 4] = ["main_2g", "main_5g", "guest_2g", "guest_5g"];
const MAX_BLOCKED: usize = 32;
const MAX_NAME: usize = 32;
const MIRROR_WAIT: Duration = Duration::from_secs(6);

/// `aa-bb-…`/`aa:bb:…` in any case → `AA:BB:CC:DD:EE:FF`.
pub fn normalize_mac(raw: &str) -> Option<String> {
    let hex: String = raw
        .trim()
        .chars()
        .filter(|c| *c != ':' && *c != '-')
        .collect();
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let upper = hex.to_ascii_uppercase();
    Some(
        upper
            .as_bytes()
            .chunks(2)
            .map(|p| std::str::from_utf8(p).unwrap_or("00"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

fn parse_body(body: &[u8]) -> Result<Value, (u16, Value)> {
    serde_json::from_slice(body).map_err(|_| (400, json!({"ok": false, "error": "invalid JSON"})))
}

fn mac_field(parsed: &Value) -> Result<String, (u16, Value)> {
    parsed["mac"]
        .as_str()
        .and_then(normalize_mac)
        .ok_or_else(|| {
            (
                400,
                json!({"ok": false, "error": "mac must be a MAC address"}),
            )
        })
}

// ── Names ────────────────────────────────────────────────────────────────────

/// Custom names set here or in the stock UI, keyed by lower-case MAC (the key
/// the clients list uses). Empty when the firmware cannot be asked.
pub fn custom_names() -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(v) = ubus::call(
        "zwrt_router.api",
        "router_get_modified_lan_hostname",
        Some("{}"),
    ) {
        for d in v["devices"].as_array().into_iter().flatten() {
            if let (Some(mac), Some(name)) = (
                d["mac"].as_str().and_then(normalize_mac),
                d["hostname"].as_str(),
            ) {
                if !name.is_empty() {
                    out.insert(mac.to_lowercase(), name.to_string());
                }
            }
        }
    }
    out
}

fn valid_name(name: &str) -> bool {
    let len = name.chars().count();
    (1..=MAX_NAME).contains(&len)
        && name.trim() == name
        && !name.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '\'' | '"' | ';' | '$' | '`' | '\\' | '|' | '<' | '>' | '&'
                )
        })
}

/// PUT /api/network/clients/name — {mac, name}
pub fn name_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mac = match mac_field(&parsed) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let Some(name) = parsed["name"].as_str().filter(|n| valid_name(n)) else {
        return (
            400,
            json!({"ok": false, "error": "name must be 1-32 characters without quotes or shell symbols"}),
        );
    };
    let params = json!({"mac": mac, "hostname": name});
    if let Err(e) = ubus::call(
        "zwrt_router.api",
        "router_modify_lan_hostname",
        Some(&params.to_string()),
    ) {
        return (503, json!({"ok": false, "error": e}));
    }
    let saved = custom_names().get(&mac.to_lowercase()).cloned();
    if saved.as_deref() != Some(name) {
        return (
            503,
            json!({"ok": false, "error": "the router did not keep the new name"}),
        );
    }
    (200, json!({"ok": true, "data": {"mac": mac, "name": name}}))
}

// ── Disconnect ───────────────────────────────────────────────────────────────

fn kick(mac: &str) -> Result<(), String> {
    ubus::call(
        "zwrt_wlan",
        "kick_macs",
        Some(&json!({"macs": mac}).to_string()),
    )
    .map(|_| ())
}

/// POST /api/network/clients/kick — {mac}. The device may rejoin at once
/// unless it is also blocked.
pub fn kick_post(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mac = match mac_field(&parsed) {
        Ok(m) => m,
        Err(e) => return e,
    };
    match kick(&mac) {
        Ok(()) => (200, json!({"ok": true, "data": {"mac": mac}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

// ── Block list ───────────────────────────────────────────────────────────────

/// `uci show` renders a list as `'a' 'b'`; `ubus::uci_show` strips the outer
/// quotes, leaving `a' 'b`.
fn uci_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| v.split("' '").filter_map(normalize_mac).collect::<Vec<_>>())
        .unwrap_or_default()
}

struct Filter {
    /// Access-point sections present on this device.
    sections: Vec<&'static str>,
    blocked: BTreeSet<String>,
    /// A section in allow-list mode: the block list does not apply there.
    allow_mode: bool,
}

fn read_filter(cfg: &HashMap<String, String>) -> Filter {
    let mut sections = Vec::new();
    let mut blocked = BTreeSet::new();
    let mut allow_mode = false;
    for sec in AP_SECTIONS {
        if !cfg.contains_key(sec) {
            continue;
        }
        sections.push(sec);
        if cfg.get(&format!("{sec}.macfilter")).map(String::as_str) == Some("allow") {
            allow_mode = true;
        }
        blocked.extend(uci_list(cfg.get(&format!("{sec}.denymaclist"))));
    }
    Filter {
        sections,
        blocked,
        allow_mode,
    }
}

fn blocklist_view(filter: &Filter) -> Value {
    let names = custom_names();
    let entries: Vec<Value> = filter
        .blocked
        .iter()
        .map(|mac| json!({"mac": mac, "name": names.get(&mac.to_lowercase())}))
        .collect();
    json!({
        "blocked": entries,
        "max": MAX_BLOCKED,
        "available": !filter.allow_mode && !filter.sections.is_empty(),
    })
}

/// GET /api/network/blocklist
pub fn blocklist_get(_state: &AppState) -> (u16, Value) {
    let cfg = ubus::uci_show("wireless");
    if cfg.is_empty() {
        return (
            503,
            json!({"ok": false, "error": "the Wi-Fi configuration could not be read"}),
        );
    }
    (
        200,
        json!({"ok": true, "data": blocklist_view(&read_filter(&cfg))}),
    )
}

fn set_payload(sections: &[&str], list: &BTreeSet<String>) -> Value {
    let mut m = Map::new();
    for sec in sections {
        m.insert(
            (*sec).into(),
            json!({"macfilter": "deny", "denymaclist": list.iter().collect::<Vec<_>>()}),
        );
    }
    Value::Object(m)
}

/// Wait for the firmware's `maclist` copy to match; when the list was emptied
/// and the copy lingers, remove it so a later Wi-Fi start does not re-block.
fn settle_mirror(sections: &[&str], list: &BTreeSet<String>) -> Result<(), String> {
    let deadline = Instant::now() + MIRROR_WAIT;
    loop {
        let cfg = ubus::uci_show("wireless");
        let stale: Vec<&str> = sections
            .iter()
            .copied()
            .filter(|sec| {
                let mirror: BTreeSet<String> = uci_list(cfg.get(&format!("{sec}.maclist")))
                    .into_iter()
                    .collect();
                &mirror != list
            })
            .collect();
        if stale.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            if !list.is_empty() {
                return Err("the Wi-Fi block list was saved but not yet applied".into());
            }
            for sec in &stale {
                let _ = std::process::Command::new("uci")
                    .args(["-q", "delete", &format!("wireless.{sec}.maclist")])
                    .bounded_output();
            }
            return ubus::uci_commit("wireless");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// PUT /api/network/blocklist — {mac, blocked: bool}
pub fn blocklist_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mac = match mac_field(&parsed) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let Some(block) = parsed["blocked"].as_bool() else {
        return (
            400,
            json!({"ok": false, "error": "blocked must be a boolean"}),
        );
    };
    // Shares the Wi-Fi settings lock: both rewrite the wireless config.
    let _guard = crate::uci_transaction::WIFI_CHANGE.safe_lock();
    let cfg = ubus::uci_show("wireless");
    let filter = read_filter(&cfg);
    if filter.sections.is_empty() {
        return (
            503,
            json!({"ok": false, "error": "the Wi-Fi configuration could not be read"}),
        );
    }
    if filter.allow_mode {
        return (
            409,
            json!({"ok": false, "error": "the Wi-Fi MAC filter is in allow-list mode; change it in the stock web UI"}),
        );
    }
    let mut list = filter.blocked.clone();
    let changed = if block {
        list.insert(mac.clone())
    } else {
        list.remove(&mac)
    };
    if list.len() > MAX_BLOCKED {
        return (
            400,
            json!({"ok": false, "error": format!("at most {MAX_BLOCKED} devices can be blocked")}),
        );
    }
    if changed {
        let payload = set_payload(&filter.sections, &list);
        if let Err(e) = ubus::call("zwrt_wlan", "set", Some(&payload.to_string())) {
            return (503, json!({"ok": false, "error": e}));
        }
        if let Err(e) = settle_mirror(&filter.sections, &list) {
            return (503, json!({"ok": false, "error": e}));
        }
    }
    if block {
        // Already-associated stations are not dropped by the ACL alone.
        let _ = kick(&mac);
    }
    blocklist_get(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs_are_normalized() {
        assert_eq!(
            normalize_mac("aa-bb-cc-dd-ee-0f").as_deref(),
            Some("AA:BB:CC:DD:EE:0F")
        );
        assert_eq!(
            normalize_mac(" 02:00:00:00:00:01 ").as_deref(),
            Some("02:00:00:00:00:01")
        );
        assert!(normalize_mac("02:00:00:00:00").is_none());
        assert!(normalize_mac("zz:00:00:00:00:01").is_none());
    }

    #[test]
    fn names_reject_shell_symbols_and_padding() {
        assert!(valid_name("Living room TV"));
        assert!(valid_name("客厅电视"));
        assert!(!valid_name(""));
        assert!(!valid_name(" padded"));
        assert!(!valid_name("a;b"));
        assert!(!valid_name(&"x".repeat(33)));
    }

    #[test]
    fn filter_reads_lists_and_allow_mode() {
        let mut cfg = HashMap::new();
        cfg.insert("main_2g".to_string(), "wifi-iface".to_string());
        cfg.insert("main_2g.macfilter".to_string(), "deny".to_string());
        cfg.insert(
            "main_2g.denymaclist".to_string(),
            "02:00:00:00:00:01' '02:00:00:00:00:02".to_string(),
        );
        cfg.insert("guest_2g".to_string(), "wifi-iface".to_string());
        cfg.insert("guest_2g.macfilter".to_string(), "deny".to_string());
        let f = read_filter(&cfg);
        assert_eq!(f.sections, vec!["main_2g", "guest_2g"]);
        assert_eq!(f.blocked.len(), 2);
        assert!(!f.allow_mode);
        cfg.insert("guest_2g.macfilter".to_string(), "allow".to_string());
        assert!(read_filter(&cfg).allow_mode);
    }

    #[test]
    fn payload_writes_every_section() {
        let list: BTreeSet<String> = ["02:00:00:00:00:01".to_string()].into();
        let p = set_payload(&["main_2g", "main_5g"], &list);
        assert_eq!(p["main_5g"]["macfilter"], "deny");
        assert_eq!(p["main_2g"]["denymaclist"][0], "02:00:00:00:00:01");
        let empty = set_payload(&["main_2g"], &BTreeSet::new());
        assert_eq!(empty["main_2g"]["denymaclist"], json!([]));
    }
}
