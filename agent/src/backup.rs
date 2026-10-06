//! Settings backup and restore: one JSON document with the agent's own state
//! (proxy subscriptions and settings, SMS forwarding) and the router settings
//! this dashboard manages (sleep, scheduled reboot, watchdog, UPnP/DMZ/remote
//! access, port rules, fixed addresses, monthly limit, Wi-Fi block list and
//! device names). It holds subscription links and push keys, so the dashboard
//! treats the file as private.
//!
//! Restoring goes through the same handlers as the dashboard, so every value is
//! validated again, and each section succeeds or fails on its own. Rules and
//! bindings are added when missing, never deleted. Downloaded subscription
//! configs are not stored (the request limit is 1 MiB); they are fetched again.

use serde_json::{json, Map, Value};

use crate::handlers::AppState;
use crate::{clients, netsvc, schedule, sms_forward, ubus, wwan};

const FORMAT: &str = "u60-pro-webui-backup";
const VERSION: u64 = 1;

type Reply = (u16, Value);

fn data(reply: Reply) -> Value {
    if reply.0 < 300 {
        reply.1["data"].clone()
    } else {
        Value::Null
    }
}

fn result(reply: Reply) -> Result<Value, String> {
    if reply.0 < 300 {
        Ok(reply.1["data"].clone())
    } else {
        Err(reply.1["error"].as_str().unwrap_or("failed").to_string())
    }
}

fn body(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

/// GET /api/system/backup
pub fn export(state: &AppState) -> Reply {
    let names: Map<String, Value> = clients::custom_names()
        .into_iter()
        .map(|(mac, name)| (mac.to_uppercase(), json!(name)))
        .collect();
    let created = ubus::call("zwrt_sntp", "get_systime", Some("{}"))
        .ok()
        .and_then(|v| v["localtime"].as_str().map(str::to_string));
    let firmware = ubus::call("zwrt_zte_mdm.api", "get_zwrt_common_info", Some("{}"))
        .ok()
        .and_then(|v| v["wa_inner_version"].as_str().map(str::to_string));
    (
        200,
        json!({"ok": true, "data": {
            "format": FORMAT,
            "version": VERSION,
            "created": created,
            "firmware": firmware,
            "sections": {
                "proxy": state.mihomo.export_state(),
                "sms_forward": sms_forward::export(),
                "sleep": data(schedule::sleep_get(state)),
                "reboot_schedule": data(schedule::reboot_schedule_get(state)),
                "watchdog": data(netsvc::watchdog_get(state)),
                "firewall": data(netsvc::firewall_get(state)),
                "port_rules": data(netsvc::port_rules_get(state)),
                "dhcp_bindings": data(netsvc::bindings_get(state)),
                "data_limit": data(wwan::limit_get(state)),
                "blocklist": data(clients::blocklist_get(state)),
                "client_names": names,
            },
        }}),
    )
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    let mut m = Map::new();
    for k in keys {
        if let Some(x) = v.get(*k).filter(|x| !x.is_null()) {
            m.insert((*k).to_string(), x.clone());
        }
    }
    Value::Object(m)
}

fn restore_section(state: &AppState, name: &str, v: &Value) -> Result<Option<String>, String> {
    match name {
        "proxy" => state.mihomo.import_state(v),
        "sms_forward" => sms_forward::import(v).map(|()| None),
        "sleep" => result(schedule::sleep_set(state, &body(pick(v, &["minutes"])))).map(|_| None),
        "reboot_schedule" => result(schedule::reboot_schedule_set(
            state,
            &body(pick(
                v,
                &[
                    "enabled",
                    "mode",
                    "weekday",
                    "interval_days",
                    "hour",
                    "minute",
                    "window_hours",
                ],
            )),
        ))
        .map(|_| None),
        "watchdog" => {
            let req = if v["enabled"] == true {
                pick(v, &["enabled", "host", "interval_minutes", "failures"])
            } else {
                json!({"enabled": false})
            };
            result(netsvc::watchdog_set(state, &body(req))).map(|_| None)
        }
        "firewall" => {
            let mut req = pick(v, &["upnp", "remote_web_access", "wan_ping", "dmz_enabled"]);
            if v["dmz_enabled"] == true {
                req["dmz_ip"] = v["dmz_ip"].clone();
            }
            result(netsvc::firewall_set(state, &body(req))).map(|_| None)
        }
        "data_limit" => {
            if v["kind"] != "data" {
                return Ok(Some(
                    "a connection-time limit is set in the stock web UI".into(),
                ));
            }
            let req = if v["enabled"] == true {
                pick(v, &["enabled", "limit_bytes", "alert_percent"])
            } else {
                json!({"enabled": false})
            };
            result(wwan::limit_set(state, &body(req))).map(|_| None)
        }
        "port_rules" => {
            let current = result(netsvc::port_rules_get(state))?;
            let have = current["rules"].as_array().cloned().unwrap_or_default();
            let same = |a: &Value, b: &Value| {
                [
                    "kind",
                    "ip",
                    "external_start",
                    "external_end",
                    "internal",
                    "proto",
                ]
                .iter()
                .all(|k| a[*k] == b[*k])
            };
            let mut errors = Vec::new();
            for rule in v["rules"].as_array().into_iter().flatten() {
                if have.iter().any(|h| same(h, rule)) {
                    continue;
                }
                let mut req = pick(
                    rule,
                    &[
                        "kind",
                        "ip",
                        "proto",
                        "comment",
                        "external_start",
                        "internal",
                    ],
                );
                if rule["kind"] == "forward" {
                    req["external_end"] = rule["external_end"].clone();
                }
                if let Err(e) = result(netsvc::port_rule_add(state, &body(req))) {
                    errors.push(format!(
                        "{}: {e}",
                        rule["comment"].as_str().unwrap_or("rule")
                    ));
                }
            }
            result(netsvc::port_rules_switch(
                state,
                &body(pick(v, &["forward_enabled", "mapping_enabled"])),
            ))?;
            Ok((!errors.is_empty()).then(|| errors.join("; ")))
        }
        "dhcp_bindings" => {
            let current = result(netsvc::bindings_get(state))?;
            let have: Vec<String> = current["bindings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["mac"].as_str().map(str::to_uppercase))
                .collect();
            let mut errors = Vec::new();
            for b in v["bindings"].as_array().into_iter().flatten() {
                let mac = b["mac"].as_str().unwrap_or_default().to_uppercase();
                if have.contains(&mac) {
                    continue;
                }
                if let Err(e) = result(netsvc::binding_add(state, &body(pick(b, &["mac", "ip"])))) {
                    errors.push(format!("{mac}: {e}"));
                }
            }
            if let Some(on) = v["enabled"].as_bool() {
                result(netsvc::bindings_switch(
                    state,
                    &body(json!({"enabled": on})),
                ))?;
            }
            Ok((!errors.is_empty()).then(|| errors.join("; ")))
        }
        "blocklist" => {
            let current = result(clients::blocklist_get(state))?;
            let have: Vec<String> = current["blocked"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["mac"].as_str().map(str::to_uppercase))
                .collect();
            let mut errors = Vec::new();
            for b in v["blocked"].as_array().into_iter().flatten() {
                let mac = b["mac"].as_str().unwrap_or_default().to_uppercase();
                if mac.is_empty() || have.contains(&mac) {
                    continue;
                }
                if let Err(e) = result(clients::blocklist_set(
                    state,
                    &body(json!({"mac": mac, "blocked": true})),
                )) {
                    errors.push(format!("{mac}: {e}"));
                }
            }
            Ok((!errors.is_empty()).then(|| errors.join("; ")))
        }
        "client_names" => {
            let current = clients::custom_names();
            let mut errors = Vec::new();
            for (mac, name) in v.as_object().into_iter().flatten() {
                let Some(name) = name.as_str() else { continue };
                if current.get(&mac.to_lowercase()).map(String::as_str) == Some(name) {
                    continue;
                }
                if let Err(e) = result(clients::name_set(
                    state,
                    &body(json!({"mac": mac, "name": name})),
                )) {
                    errors.push(format!("{mac}: {e}"));
                }
            }
            Ok((!errors.is_empty()).then(|| errors.join("; ")))
        }
        _ => Err("unknown section".into()),
    }
}

/// Sections in the order they are restored (agent state first, the watchdog
/// last because it waits for the router to confirm the address).
const ORDER: [&str; 11] = [
    "proxy",
    "sms_forward",
    "client_names",
    "sleep",
    "reboot_schedule",
    "data_limit",
    "firewall",
    "dhcp_bindings",
    "port_rules",
    "blocklist",
    "watchdog",
];

/// POST /api/system/restore — {format, version, sections, only?: [names]}
pub fn restore(state: &AppState, body_bytes: &[u8]) -> Reply {
    let doc: Value = match serde_json::from_slice(body_bytes) {
        Ok(v) => v,
        Err(_) => {
            return (
                400,
                json!({"ok": false, "error": "the file is not valid JSON"}),
            )
        }
    };
    if doc["format"] != FORMAT {
        return (
            400,
            json!({"ok": false, "error": "this is not a U60-Pro-WebUI settings backup"}),
        );
    }
    if doc["version"].as_u64().is_none_or(|v| v > VERSION) {
        return (
            400,
            json!({"ok": false, "error": "the backup was made by a newer version of the dashboard"}),
        );
    }
    let only: Option<Vec<&str>> = doc["only"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect());
    let sections = &doc["sections"];
    let mut results = Map::new();
    for name in ORDER {
        let v = &sections[name];
        let wanted = only.as_ref().is_none_or(|o| o.contains(&name));
        let outcome = if !wanted {
            json!({"status": "skipped"})
        } else if v.is_null() {
            json!({"status": "missing"})
        } else {
            match restore_section(state, name, v) {
                Ok(None) => json!({"status": "ok"}),
                Ok(Some(note)) => json!({"status": "partial", "message": note}),
                Err(e) => json!({"status": "failed", "message": e}),
            }
        };
        results.insert(name.to_string(), outcome);
    }
    (200, json!({"ok": true, "data": {"results": results}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_keeps_present_non_null_keys() {
        let v = json!({"a": 1, "b": null, "c": "x"});
        assert_eq!(pick(&v, &["a", "b", "d"]), json!({"a": 1}));
    }

    #[test]
    fn restore_order_covers_every_exported_section() {
        let exported = [
            "proxy",
            "sms_forward",
            "sleep",
            "reboot_schedule",
            "watchdog",
            "firewall",
            "port_rules",
            "dhcp_bindings",
            "data_limit",
            "blocklist",
            "client_names",
        ];
        for s in exported {
            assert!(ORDER.contains(&s), "{s}");
        }
        assert_eq!(ORDER.len(), exported.len());
    }
}
