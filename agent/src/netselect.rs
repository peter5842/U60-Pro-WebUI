//! Manual carrier (PLMN) selection, through the stock zte_nwinfo_api flow:
//! `nwinfo_manual_scan` starts a scan that the stock UI polls with
//! `nwinfo_m_netselect_status` ("manual_selecting" while it runs) and reads with
//! `nwinfo_m_netselect_contents` ("state,name,mccmnc,rat;…");
//! `nwinfo_manual_register` then picks one and `nwinfo_m_netselect_result`
//! reports "manual_success"/"manual_fail". Going back to automatic re-applies
//! the current network mode with `nwinfo_set_netselect`, the only write the
//! stock page has, and is confirmed from `net_select_mode`.
//!
//! A scan can pause mobile data for a minute or more; the dashboard says so.

use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::handlers::AppState;
use crate::ubus;

const NWINFO: &str = "zte_nwinfo_api";
const RATS: [&str; 8] = ["0", "2", "7", "9", "11", "12", "13", "14"];

fn nwinfo(method: &str, params: Option<Value>) -> Result<Value, String> {
    ubus::call(
        NWINFO,
        method,
        Some(&params.unwrap_or_else(|| json!({})).to_string()),
    )
}

fn rat_label(rat: &str) -> &'static str {
    match rat {
        "0" => "2G",
        "2" => "3G",
        "7" | "13" => "4G",
        "9" | "11" | "12" | "14" => "5G",
        _ => "?",
    }
}

/// "1,\"CMCC\",46000,7;2,China Mobile,46000,12;" → rows
fn parse_contents(text: &str) -> Vec<Value> {
    text.split(';')
        .filter_map(|row| {
            let f: Vec<&str> = row.split(',').map(|s| s.trim().trim_matches('"')).collect();
            if f.len() < 4 || f[2].is_empty() || !f[2].bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let state = match f[0] {
                "1" => "available",
                "2" => "current",
                "3" => "forbidden",
                _ => "unknown",
            };
            Some(json!({
                "state": state,
                "name": f[1],
                "mccmnc": f[2],
                "rat": f[3],
                "rat_label": rat_label(f[3]),
            }))
        })
        .collect()
}

fn field(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn view() -> Result<Value, String> {
    let info = nwinfo("nwinfo_get_netinfo", None)?;
    let status = field(
        &nwinfo("nwinfo_m_netselect_status", None)?,
        "m_netselect_status",
    );
    let contents = field(
        &nwinfo("nwinfo_m_netselect_contents", None)?,
        "m_netselect_contents",
    );
    let result = field(
        &nwinfo("nwinfo_m_netselect_result", None)?,
        "m_netselect_result",
    );
    let networks = parse_contents(&contents);
    let scan = match status.as_str() {
        "manual_selecting" => "scanning",
        "manual_search_fail" => "failed",
        "" if networks.is_empty() => "idle",
        _ => "done",
    };
    let register = match result.as_str() {
        "manual_success" => "success",
        "manual_fail" => "failed",
        "" => "idle",
        _ => "registering",
    };
    let mut m = Map::new();
    m.insert(
        "select_mode".into(),
        json!(if field(&info, "net_select_mode") == "manual_select" {
            "manual"
        } else {
            "auto"
        }),
    );
    m.insert("network_mode".into(), json!(field(&info, "net_select")));
    m.insert(
        "current".into(),
        json!({
            "name": Some(field(&info, "network_provider_fullname")).filter(|s| !s.is_empty())
                .or_else(|| Some(field(&info, "network_provider")).filter(|s| !s.is_empty())),
            "mcc": field(&info, "rmcc"),
            "mnc": field(&info, "rmnc"),
        }),
    );
    m.insert("scan".into(), json!(scan));
    m.insert("networks".into(), json!(networks));
    m.insert("register".into(), json!(register));
    Ok(Value::Object(m))
}

/// GET /api/cell/operators
pub fn get(_state: &AppState) -> (u16, Value) {
    match view() {
        Ok(v) => (200, json!({"ok": true, "data": v})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/cell/operators/scan — starts a scan; poll GET for progress.
pub fn scan(state: &AppState) -> (u16, Value) {
    if let Ok(v) = view() {
        if v["scan"] == "scanning" {
            return (
                409,
                json!({"ok": false, "error": "a scan is already running"}),
            );
        }
    }
    match nwinfo("nwinfo_manual_scan", None) {
        Ok(_) => get(state),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/cell/operators/select — {mccmnc, rat}; poll GET for the result.
pub fn select(state: &AppState, body: &[u8]) -> (u16, Value) {
    let obj = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => return (400, json!({"ok": false, "error": "expected a JSON object"})),
    };
    let mccmnc = obj
        .get("mccmnc")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let rat = obj.get("rat").and_then(Value::as_str).unwrap_or_default();
    if !(5..=6).contains(&mccmnc.len()) || !mccmnc.bytes().all(|b| b.is_ascii_digit()) {
        return (
            400,
            json!({"ok": false, "error": "mccmnc must be 5 or 6 digits"}),
        );
    }
    if !RATS.contains(&rat) {
        return (
            400,
            json!({"ok": false, "error": "rat is not a known access technology"}),
        );
    }
    match nwinfo(
        "nwinfo_manual_register",
        Some(json!({"m_mcc_mnc": mccmnc, "m_rat": rat})),
    ) {
        Ok(_) => get(state),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/cell/operators/auto — back to automatic selection.
pub fn auto(_state: &AppState) -> (u16, Value) {
    let mode = match nwinfo("nwinfo_get_netinfo", None) {
        Ok(v) => field(&v, "net_select"),
        Err(e) => return (503, json!({"ok": false, "error": e})),
    };
    if mode.is_empty() {
        return (
            503,
            json!({"ok": false, "error": "the current network mode could not be read"}),
        );
    }
    if let Err(e) = nwinfo("nwinfo_set_netselect", Some(json!({"net_select": mode}))) {
        return (503, json!({"ok": false, "error": e}));
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(v) = view() {
            if v["select_mode"] == "auto" {
                return (200, json!({"ok": true, "data": v}));
            }
        }
    }
    (
        503,
        json!({"ok": false, "error": "the modem still reports manual selection; try again or restart the router"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scan_rows() {
        let rows = parse_contents(
            "2,\"CHINA MOBILE\",46000,12;1,CHN-UNICOM,46001,7;3,CHN-CT,46011,7;bad;",
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["state"], "current");
        assert_eq!(rows[0]["name"], "CHINA MOBILE");
        assert_eq!(rows[0]["rat_label"], "5G");
        assert_eq!(rows[1]["rat_label"], "4G");
        assert_eq!(rows[2]["state"], "forbidden");
        assert!(parse_contents("").is_empty());
    }
}
