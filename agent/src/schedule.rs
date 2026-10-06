//! Device sleep timer and scheduled reboot, through the same ubus calls as the
//! stock web UI.
//!
//! - Sleep: read uci `zwrt_sleep.ztmp_time.SysIdTime`, write
//!   `zwrt_zte_sleep_faw.wakelock set_ufi_sleep {ufiSleepTime}` (minutes as a
//!   string, `-1` = never).
//! - Reboot schedule: read uci `zwrt_zte_mc.reboot_schedule`, write
//!   `zwrt_mc.device.manager set_device_info {deviceInfoList:{…}}` with every
//!   value as a string. Mode 1 is weekly (`reboot_dow`, slot 1); mode 2 is every
//!   N days since boot (`reboot_dod`, slot 2). The firmware reboots at a random
//!   moment within `timeframe` hours after the set time. The device clock holds
//!   local time, so the hour is local.

use serde_json::{json, Map, Value};

use crate::handlers::AppState;
use crate::ubus;

const SLEEP_MINUTES: [i64; 7] = [-1, 5, 10, 20, 30, 60, 120];

/// GET /api/device/sleep
pub fn sleep_get(_state: &AppState) -> (u16, Value) {
    match ubus::uci_get("zwrt_sleep.ztmp_time.SysIdTime") {
        Ok(v) => match v.trim().parse::<i64>() {
            Ok(minutes) => (
                200,
                json!({"ok": true, "data": {"minutes": minutes, "options": SLEEP_MINUTES}}),
            ),
            Err(_) => (
                503,
                json!({"ok": false, "error": format!("unexpected sleep time {v:?}")}),
            ),
        },
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// PUT /api/device/sleep — {minutes: -1 | 5 | 10 | 20 | 30 | 60 | 120}
pub fn sleep_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let Some(minutes) = parsed["minutes"]
        .as_i64()
        .filter(|m| SLEEP_MINUTES.contains(m))
    else {
        return (
            400,
            json!({"ok": false, "error": "minutes must be -1 (never), 5, 10, 20, 30, 60 or 120"}),
        );
    };
    let params = json!({"ufiSleepTime": minutes.to_string()});
    if let Err(e) = ubus::call(
        "zwrt_zte_sleep_faw.wakelock",
        "set_ufi_sleep",
        Some(&params.to_string()),
    ) {
        return (503, json!({"ok": false, "error": e}));
    }
    sleep_get(state)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    hour: u64,
    minute: u64,
    window_hours: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct RebootSchedule {
    enabled: bool,
    weekly: bool,
    weekday: u64,
    interval_days: u64,
    weekly_slot: Slot,
    interval_slot: Slot,
}

fn field(cfg: &std::collections::HashMap<String, String>, key: &str) -> Option<u64> {
    cfg.get(&format!("reboot_schedule.{key}"))
        .and_then(|v| v.trim().parse().ok())
}

fn read_schedule() -> Result<RebootSchedule, String> {
    let cfg = ubus::uci_show("zwrt_zte_mc");
    let slot = |n: u8| -> Option<Slot> {
        Some(Slot {
            hour: field(&cfg, &format!("reboot_hour{n}"))?,
            minute: field(&cfg, &format!("reboot_min{n}"))?,
            window_hours: field(&cfg, &format!("reboot_timeframe_hours{n}"))?,
        })
    };
    (|| {
        Some(RebootSchedule {
            enabled: field(&cfg, "reboot_schedule_enable")? == 1,
            weekly: field(&cfg, "reboot_schedule_mode")? != 2,
            weekday: field(&cfg, "reboot_dow")?,
            interval_days: field(&cfg, "reboot_dod")?,
            weekly_slot: slot(1)?,
            interval_slot: slot(2)?,
        })
    })()
    .ok_or_else(|| "the reboot schedule could not be read".to_string())
}

fn schedule_view(s: &RebootSchedule) -> Value {
    let slot = if s.weekly {
        s.weekly_slot
    } else {
        s.interval_slot
    };
    json!({
        "enabled": s.enabled,
        "mode": if s.weekly { "weekly" } else { "interval" },
        "weekday": s.weekday,
        "interval_days": s.interval_days,
        "hour": slot.hour,
        "minute": slot.minute,
        "window_hours": slot.window_hours,
    })
}

/// Apply a request onto the current schedule. Fields left out keep their
/// value; only the active mode's time slot changes.
fn plan_schedule(current: &RebootSchedule, req: &Value) -> Result<RebootSchedule, String> {
    let obj = req.as_object().ok_or("expected a JSON object")?;
    const KNOWN: [&str; 7] = [
        "enabled",
        "mode",
        "weekday",
        "interval_days",
        "hour",
        "minute",
        "window_hours",
    ];
    if let Some(k) = obj.keys().find(|k| !KNOWN.contains(&k.as_str())) {
        return Err(format!("unknown field {k}"));
    }
    let num = |key: &str, max: u64, min: u64| -> Result<Option<u64>, String> {
        match obj.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .filter(|n| (min..=max).contains(n))
                .map(Some)
                .ok_or_else(|| format!("{key} must be {min}-{max}")),
        }
    };
    let mut next = current.clone();
    if let Some(v) = obj.get("enabled") {
        next.enabled = v.as_bool().ok_or("enabled must be a boolean")?;
    }
    if let Some(v) = obj.get("mode") {
        next.weekly = match v.as_str() {
            Some("weekly") => true,
            Some("interval") => false,
            _ => return Err("mode must be weekly or interval".into()),
        };
    }
    if let Some(d) = num("weekday", 6, 0)? {
        next.weekday = d;
    }
    if let Some(d) = num("interval_days", 30, 1)? {
        next.interval_days = d;
    }
    let slot = if next.weekly {
        &mut next.weekly_slot
    } else {
        &mut next.interval_slot
    };
    if let Some(h) = num("hour", 23, 0)? {
        slot.hour = h;
    }
    if let Some(m) = num("minute", 59, 0)? {
        slot.minute = m;
    }
    if let Some(w) = num("window_hours", 6, 0)? {
        slot.window_hours = w;
    }
    Ok(next)
}

fn device_info_list(s: &RebootSchedule) -> Value {
    let mut m = Map::new();
    let mut put = |k: &str, v: u64| {
        m.insert(k.into(), json!(v.to_string()));
    };
    put("reboot_schedule_enable", u64::from(s.enabled));
    put("reboot_schedule_mode", if s.weekly { 1 } else { 2 });
    put("reboot_dow", s.weekday);
    put("reboot_hour1", s.weekly_slot.hour);
    put("reboot_min1", s.weekly_slot.minute);
    put("reboot_timeframe_hours1", s.weekly_slot.window_hours);
    put("reboot_dod", s.interval_days);
    put("reboot_hour2", s.interval_slot.hour);
    put("reboot_min2", s.interval_slot.minute);
    put("reboot_timeframe_hours2", s.interval_slot.window_hours);
    json!({"deviceInfoList": m})
}

/// GET /api/device/reboot-schedule
pub fn reboot_schedule_get(_state: &AppState) -> (u16, Value) {
    match read_schedule() {
        Ok(s) => (200, json!({"ok": true, "data": schedule_view(&s)})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// PUT /api/device/reboot-schedule — any of {enabled, mode, weekday,
/// interval_days, hour, minute, window_hours}.
pub fn reboot_schedule_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let current = match read_schedule() {
        Ok(s) => s,
        Err(e) => return (503, json!({"ok": false, "error": e})),
    };
    let next = match plan_schedule(&current, &parsed) {
        Ok(s) => s,
        Err(e) => return (400, json!({"ok": false, "error": e})),
    };
    if next != current {
        if let Err(e) = ubus::call(
            "zwrt_mc.device.manager",
            "set_device_info",
            Some(&device_info_list(&next).to_string()),
        ) {
            return (503, json!({"ok": false, "error": e}));
        }
    }
    reboot_schedule_get(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> RebootSchedule {
        RebootSchedule {
            enabled: false,
            weekly: true,
            weekday: 2,
            interval_days: 1,
            weekly_slot: Slot {
                hour: 2,
                minute: 0,
                window_hours: 2,
            },
            interval_slot: Slot {
                hour: 3,
                minute: 30,
                window_hours: 1,
            },
        }
    }

    #[test]
    fn view_shows_the_active_mode_slot() {
        let mut s = current();
        assert_eq!(schedule_view(&s)["hour"], 2);
        s.weekly = false;
        let v = schedule_view(&s);
        assert_eq!(v["mode"], "interval");
        assert_eq!(v["hour"], 3);
        assert_eq!(v["minute"], 30);
    }

    #[test]
    fn plan_changes_only_the_active_slot_and_validates() {
        let next = plan_schedule(
            &current(),
            &json!({"enabled": true, "mode": "interval", "interval_days": 7, "hour": 4}),
        )
        .unwrap();
        assert!(next.enabled);
        assert!(!next.weekly);
        assert_eq!(next.interval_days, 7);
        assert_eq!(next.interval_slot.hour, 4);
        assert_eq!(next.weekly_slot, current().weekly_slot);
        assert!(plan_schedule(&current(), &json!({"hour": 24})).is_err());
        assert!(plan_schedule(&current(), &json!({"window_hours": 7})).is_err());
        assert!(plan_schedule(&current(), &json!({"interval_days": 0})).is_err());
        assert!(plan_schedule(&current(), &json!({"weekday": 7})).is_err());
        assert!(plan_schedule(&current(), &json!({"mode": "daily"})).is_err());
        assert!(plan_schedule(&current(), &json!({"reboot_dow": "1"})).is_err());
    }

    #[test]
    fn device_info_values_are_strings() {
        let list = device_info_list(&current());
        let m = &list["deviceInfoList"];
        assert_eq!(m["reboot_schedule_enable"], "0");
        assert_eq!(m["reboot_schedule_mode"], "1");
        assert_eq!(m["reboot_min2"], "30");
        assert_eq!(m.as_object().unwrap().len(), 10);
    }
}
