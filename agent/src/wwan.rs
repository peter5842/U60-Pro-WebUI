//! Mobile data connection and the monthly data limit (`zwrt_data`).
//!
//! Mirrors the stock web UI: `set_wwaniface {cid:1, enable:1|0}` is a
//! connect/disconnect command (the `enable` read back is not the link state;
//! `connect_status` is), and `set_wwandst_monthlimit` stores the limit in bytes
//! with an alert percentage.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;

const SOURCE: &str = "web";
const SETTLE: Duration = Duration::from_secs(8);
/// `type` of a data (byte) limit; 1 is a connection-time limit.
const LIMIT_TYPE_DATA: u64 = 2;
const MAX_LIMIT_BYTES: u64 = 1 << 50; // 1 PiB: anything above is a typo

fn read_iface() -> Result<Value, String> {
    ubus::call(
        "zwrt_data",
        "get_wwaniface",
        Some(&json!({"source_module": SOURCE, "cid": 1}).to_string()),
    )
}

fn connected(status: &str) -> bool {
    status.contains("connected") && !status.contains("disconnected")
}

fn data_view(v: &Value) -> Value {
    let status = v["connect_status"].as_str().unwrap_or("");
    json!({
        "connected": connected(status),
        "connect_status": status,
        "auto_connect": v["connect_mode"].as_i64().map(|m| m == 1),
        "roaming_allowed": v["roam_enable"].as_i64().map(|r| r == 1),
        "ipv4": v["ipv4_address"].as_str().filter(|s| !s.is_empty()),
        "ipv6": v["ipv6_address"].as_str().filter(|s| !s.is_empty()),
    })
}

/// GET /api/modem/data
pub fn data_get(_state: &AppState) -> (u16, Value) {
    match read_iface() {
        Ok(v) => (200, json!({"ok": true, "data": data_view(&v)})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// PUT /api/modem/data — {connect: bool}. Waits (bounded) for the link to
/// reach the requested state and returns what it actually is.
pub fn data_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let Some(connect) = parsed["connect"].as_bool() else {
        return (
            400,
            json!({"ok": false, "error": "connect must be a boolean"}),
        );
    };
    let params = json!({"source_module": SOURCE, "cid": 1, "enable": i32::from(connect)});
    if let Err(e) = ubus::call("zwrt_data", "set_wwaniface", Some(&params.to_string())) {
        return (503, json!({"ok": false, "error": e}));
    }
    state.dash.invalidate_usage();
    let deadline = Instant::now() + SETTLE;
    let mut last = Value::Null;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(v) = read_iface() {
            let view = data_view(&v);
            let done = view["connected"].as_bool() == Some(connect);
            last = view;
            if done {
                break;
            }
        }
    }
    if last.is_null() {
        return (
            503,
            json!({"ok": false, "error": "the modem did not report its data state"}),
        );
    }
    (200, json!({"ok": true, "data": last}))
}

fn limit_view(v: &Value) -> Value {
    let enabled = v["enable"].as_i64() == Some(1);
    let is_data = v["type"].as_u64() == Some(LIMIT_TYPE_DATA);
    let bytes = v["value"]
        .as_str()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| v["value"].as_u64());
    json!({
        "enabled": enabled,
        // A time-based limit set elsewhere is reported but not editable here.
        "kind": if is_data { "data" } else { "time" },
        "limit_bytes": if is_data { bytes } else { None },
        "alert_percent": v["ratio"].as_u64().filter(|r| (1..=100).contains(r)),
    })
}

/// GET /api/data-usage/limit
pub fn limit_get(_state: &AppState) -> (u16, Value) {
    match ubus::call(
        "zwrt_data",
        "get_wwandst_monthlimit",
        Some(&json!({"source_module": SOURCE, "cid": 1}).to_string()),
    ) {
        Ok(v) => (200, json!({"ok": true, "data": limit_view(&v)})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// Validate a limit request; returns the ubus parameters.
fn limit_params(parsed: &Value) -> Result<Value, String> {
    let enabled = parsed["enabled"]
        .as_bool()
        .ok_or("enabled must be a boolean")?;
    if !enabled {
        return Ok(json!({"source_module": SOURCE, "cid": 1, "enable": 0}));
    }
    let bytes = parsed["limit_bytes"]
        .as_u64()
        .filter(|b| (1..=MAX_LIMIT_BYTES).contains(b))
        .ok_or("limit_bytes must be a positive number of bytes")?;
    let alert = parsed["alert_percent"]
        .as_u64()
        .filter(|p| (1..=99).contains(p))
        .ok_or("alert_percent must be 1-99")?;
    Ok(json!({
        "source_module": SOURCE,
        "cid": 1,
        "enable": 1,
        "type": LIMIT_TYPE_DATA,
        "value": bytes.to_string(),
        "ratio": alert,
    }))
}

/// PUT /api/data-usage/limit — {enabled, limit_bytes?, alert_percent?}
pub fn limit_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let params = match limit_params(&parsed) {
        Ok(p) => p,
        Err(e) => return (400, json!({"ok": false, "error": e})),
    };
    if let Err(e) = ubus::call(
        "zwrt_data",
        "set_wwandst_monthlimit",
        Some(&params.to_string()),
    ) {
        return (503, json!({"ok": false, "error": e}));
    }
    state.dash.invalidate_usage();
    limit_get(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_state_comes_from_connect_status_not_enable() {
        let v = json!({"enable": 0, "connect_mode": 1, "roam_enable": 1, "connect_status": "ipv4_ipv6_connected"});
        let d = data_view(&v);
        assert_eq!(d["connected"], true);
        assert_eq!(d["auto_connect"], true);
        assert_eq!(d["roaming_allowed"], true);
        assert_eq!(
            data_view(&json!({"connect_status": "disconnected"}))["connected"],
            false
        );
        assert_eq!(data_view(&json!({}))["connected"], false);
    }

    #[test]
    fn limit_view_reads_bytes_and_flags_time_limits() {
        let v = json!({"enable": 0, "type": 2, "value": "322122547200", "ratio": 80});
        let l = limit_view(&v);
        assert_eq!(l["enabled"], false);
        assert_eq!(l["kind"], "data");
        assert_eq!(l["limit_bytes"], 322_122_547_200u64);
        assert_eq!(l["alert_percent"], 80);
        let t = limit_view(&json!({"enable": 1, "type": 1, "value": "36000", "ratio": 90}));
        assert_eq!(t["kind"], "time");
        assert!(t["limit_bytes"].is_null());
    }

    #[test]
    fn limit_params_validate() {
        assert_eq!(
            limit_params(&json!({"enabled": false})).unwrap()["enable"],
            0
        );
        let p = limit_params(
            &json!({"enabled": true, "limit_bytes": 1_073_741_824u64, "alert_percent": 80}),
        )
        .unwrap();
        assert_eq!(p["type"], 2);
        assert_eq!(p["value"], "1073741824");
        assert_eq!(p["ratio"], 80);
        assert!(
            limit_params(&json!({"enabled": true, "limit_bytes": 0, "alert_percent": 80})).is_err()
        );
        assert!(
            limit_params(&json!({"enabled": true, "limit_bytes": 10, "alert_percent": 100}))
                .is_err()
        );
        assert!(limit_params(&json!({"enabled": "yes"})).is_err());
    }
}
