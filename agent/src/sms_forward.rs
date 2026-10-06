//! Forward incoming SMS to a push service (Bark, Server酱, WeCom group bot,
//! Telegram, or any JSON webhook).
//!
//! A background thread reads the newest messages every 20 s (the stock UI
//! polls the same way; there is no ubus event for new SMS) and sends every
//! received message with an id above `last_id`, oldest first. Enabling the
//! feature starts after the newest message already stored, so history is not
//! sent. Read flags are not touched. The key/URL is stored 0600 under
//! /data/local/tmp and never returned by the API.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::storage::atomic_write;
use crate::ubus;
use crate::util::MutexExt;

const CONFIG: &str = "/data/local/tmp/sms_forward.json";
const POLL: Duration = Duration::from_secs(20);
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(120);
const BATCH: u32 = 30;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Channel {
    #[default]
    Bark,
    Serverchan,
    Wecom,
    Telegram,
    Webhook,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    enabled: bool,
    channel: Channel,
    /// Bark/Server酱 key or URL, WeCom/webhook URL, or the Telegram bot token.
    target: String,
    /// Telegram only.
    chat_id: String,
    /// Send through the local mihomo proxy (for services blocked on the
    /// mainland network, such as Telegram).
    via_proxy: bool,
    /// Highest message id already handled.
    last_id: Option<u64>,
    forwarded: u64,
    last_sent: Option<String>,
    last_error: Option<String>,
}

static LOCK: Mutex<()> = Mutex::new(());

fn load() -> Config {
    std::fs::read(CONFIG)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(cfg: &Config) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    atomic_write(Path::new(CONFIG), &bytes).map_err(|e| format!("cannot save: {e}"))
}

// ── Messages ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Sms {
    id: u64,
    from: String,
    text: String,
    time: String,
}

fn ucs2_hex(s: &str) -> Option<String> {
    if s.is_empty() || s.len() % 4 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let units: Vec<u16> = (0..s.len())
        .step_by(4)
        .filter_map(|i| u16::from_str_radix(&s[i..i + 4], 16).ok())
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn text_field(v: &Value) -> String {
    let raw = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    ucs2_hex(&raw).unwrap_or(raw)
}

/// "26,09,07,14,52,19,+32" → "2026-09-07 14:52:19"
fn sms_time(raw: &str) -> String {
    let p: Vec<&str> = raw.split(',').collect();
    if p.len() >= 6
        && p[..6]
            .iter()
            .all(|x| x.len() == 2 && x.bytes().all(|b| b.is_ascii_digit()))
    {
        format!("20{}-{}-{} {}:{}:{}", p[0], p[1], p[2], p[3], p[4], p[5])
    } else {
        raw.to_string()
    }
}

/// Received messages (tag 0 read, 1 unread) among the newest, oldest first.
fn parse_received(data: &Value) -> Vec<Sms> {
    let mut out: Vec<Sms> = data["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"]
                .as_u64()
                .or_else(|| m["id"].as_str()?.parse().ok())?;
            let tag = m["tag"]
                .as_str()
                .map(str::to_string)
                .or_else(|| m["tag"].as_u64().map(|t| t.to_string()))?;
            if tag != "0" && tag != "1" {
                return None;
            }
            Some(Sms {
                id,
                from: text_field(&m["number"]),
                text: text_field(&m["content"]),
                time: sms_time(m["date"].as_str().unwrap_or_default()),
            })
        })
        .collect();
    out.sort_by_key(|m| m.id);
    out
}

fn newest_messages() -> Result<Vec<Sms>, String> {
    let params = json!({
        "page": 0,
        "data_per_page": BATCH,
        "mem_store": 1,
        "tags": 10,
        "order_by": "order by id desc",
    });
    ubus::call(
        "zwrt_wms",
        "zte_libwms_get_sms_data",
        Some(&params.to_string()),
    )
    .map(|v| parse_received(&v))
}

// ── Sending ──────────────────────────────────────────────────────────────────

/// URL and JSON body for one message.
fn request(cfg: &Config, title: &str, body: &str) -> Result<(String, Value), String> {
    let t = cfg.target.trim();
    let is_url = t.starts_with("https://") || t.starts_with("http://");
    Ok(match cfg.channel {
        Channel::Bark => (
            if is_url {
                t.trim_end_matches('/').to_string()
            } else {
                format!("https://api.day.app/{t}")
            },
            json!({"title": title, "body": body, "group": "SMS"}),
        ),
        Channel::Serverchan => (
            if is_url {
                t.to_string()
            } else {
                format!("https://sctapi.ftqq.com/{t}.send")
            },
            json!({"title": title, "desp": body}),
        ),
        Channel::Wecom => (
            t.to_string(),
            json!({"msgtype": "text", "text": {"content": format!("{title}\n{body}")}}),
        ),
        Channel::Telegram => (
            format!("https://api.telegram.org/bot{t}/sendMessage"),
            json!({"chat_id": cfg.chat_id.trim(), "text": format!("{title}\n{body}")}),
        ),
        Channel::Webhook => (
            t.to_string(),
            json!({"title": title, "text": body, "device": "U60 Pro"}),
        ),
    })
}

/// Services answer 200 with an error code in the body; read it.
fn check_reply(channel: Channel, reply: &str) -> Result<(), String> {
    let v: Value = match serde_json::from_str(reply) {
        Ok(v) => v,
        Err(_) => return Ok(()), // plain-text webhooks: HTTP status already checked
    };
    let ok = match channel {
        Channel::Bark => v["code"].as_i64().is_none_or(|c| c == 200),
        Channel::Serverchan => v["code"].as_i64().is_none_or(|c| c == 0),
        Channel::Wecom => v["errcode"].as_i64().is_none_or(|c| c == 0),
        Channel::Telegram => v["ok"].as_bool().unwrap_or(true),
        Channel::Webhook => true,
    };
    if ok {
        Ok(())
    } else {
        let msg = ["message", "errmsg", "description", "info"]
            .iter()
            .find_map(|k| v[*k].as_str())
            .unwrap_or("rejected");
        Err(format!("the service answered: {msg}"))
    }
}

fn mask_secret(text: &str, secret: &str) -> String {
    if secret.len() >= 4 {
        text.replace(secret, "…")
    } else {
        text.to_string()
    }
}

fn send(cfg: &Config, proxy: Option<&str>, title: &str, body: &str) -> Result<(), String> {
    let (url, payload) = request(cfg, title, body)?;
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail",
        "--connect-timeout",
        "8",
        "--max-time",
        "15",
        "--header",
        "Content-Type: application/json; charset=utf-8",
        "--data-binary",
        "@-",
    ]);
    if let Some(p) = proxy {
        cmd.args(["--proxy", p]);
    }
    cmd.args(["--", &url]);
    let out = process_runner::output(
        &mut cmd,
        Some(payload.to_string().as_bytes()),
        Duration::from_secs(20),
        64 * 1024,
    )
    .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = mask_secret(err.trim().trim_start_matches("curl: "), cfg.target.trim());
        return Err(if err.is_empty() {
            "sending failed".into()
        } else {
            err
        });
    }
    check_reply(cfg.channel, &String::from_utf8_lossy(&out.stdout))
}

fn proxy_for(cfg: &Config, state: &AppState) -> Result<Option<String>, String> {
    if !cfg.via_proxy {
        return Ok(None);
    }
    state.mihomo.local_proxy().map(Some).ok_or_else(|| {
        "the proxy is not running; start it or turn off \"send through the proxy\"".to_string()
    })
}

fn now_text() -> String {
    ubus::call("zwrt_sntp", "get_systime", Some("{}"))
        .ok()
        .and_then(|v| v["localtime"].as_str().map(str::to_string))
        .unwrap_or_default()
}

// ── Background loop ──────────────────────────────────────────────────────────

pub fn start(state: Arc<AppState>) {
    std::thread::spawn(move || {
        let mut retry_at: Option<Instant> = None;
        loop {
            std::thread::sleep(POLL);
            if retry_at.is_some_and(|t| Instant::now() < t) {
                continue;
            }
            retry_at = tick(&state)
                .err()
                .map(|_| Instant::now() + RETRY_AFTER_ERROR);
        }
    });
}

/// One pass. Err means a send failed and the next pass should wait.
fn tick(state: &AppState) -> Result<(), ()> {
    let _guard = LOCK.safe_lock();
    let mut cfg = load();
    if !cfg.enabled || cfg.target.trim().is_empty() {
        return Ok(());
    }
    let Ok(messages) = newest_messages() else {
        return Ok(());
    };
    let Some(last) = cfg.last_id else {
        cfg.last_id = Some(messages.iter().map(|m| m.id).max().unwrap_or(0));
        let _ = save(&cfg);
        return Ok(());
    };
    for m in messages.into_iter().filter(|m| m.id > last) {
        let proxy = match proxy_for(&cfg, state) {
            Ok(p) => p,
            Err(e) => {
                cfg.last_error = Some(e);
                let _ = save(&cfg);
                return Err(());
            }
        };
        let body = format!("{}\n{}", m.text, m.time);
        match send(&cfg, proxy.as_deref(), &m.from, &body) {
            Ok(()) => {
                cfg.last_id = Some(m.id);
                cfg.forwarded += 1;
                cfg.last_sent = Some(now_text());
                cfg.last_error = None;
                let _ = save(&cfg);
            }
            Err(e) => {
                eprintln!("[sms-forward] message {} not sent: {e}", m.id);
                cfg.last_error = Some(e);
                let _ = save(&cfg);
                return Err(());
            }
        }
    }
    Ok(())
}

// ── Handlers ─────────────────────────────────────────────────────────────────

fn hint(cfg: &Config) -> Option<String> {
    let t = cfg.target.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
    {
        let host = rest.split('/').next().unwrap_or_default();
        return Some(format!("{host}/…"));
    }
    let keep: String = t.chars().take(4).collect();
    Some(format!("{keep}…"))
}

fn view(cfg: &Config) -> Value {
    json!({
        "enabled": cfg.enabled,
        "channel": cfg.channel,
        "configured": !cfg.target.trim().is_empty()
            && (cfg.channel != Channel::Telegram || !cfg.chat_id.trim().is_empty()),
        "target_hint": hint(cfg),
        "chat_id": Some(cfg.chat_id.clone()).filter(|c| !c.is_empty()),
        "via_proxy": cfg.via_proxy,
        "forwarded": cfg.forwarded,
        "last_sent": cfg.last_sent,
        "last_error": cfg.last_error,
    })
}

/// GET /api/sms/forward
pub fn get(_state: &AppState) -> (u16, Value) {
    let _guard = LOCK.safe_lock();
    (200, json!({"ok": true, "data": view(&load())}))
}

fn no_control(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn validate_target(channel: Channel, target: &str, chat_id: &str) -> Result<(), String> {
    let t = target.trim();
    let url = (t.starts_with("https://") || t.starts_with("http://"))
        && t.len() <= 512
        && no_control(t)
        && t.len() > 10;
    let key = |min: usize| {
        (min..=128).contains(&t.len())
            && t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let ok = match channel {
        Channel::Bark => url || key(10),
        Channel::Serverchan => url || key(10),
        Channel::Wecom | Channel::Webhook => url,
        Channel::Telegram => {
            let token_ok = t.split_once(':').is_some_and(|(id, secret)| {
                !id.is_empty()
                    && id.bytes().all(|b| b.is_ascii_digit())
                    && secret.len() >= 20
                    && secret
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            });
            let c = chat_id.trim();
            let chat_ok =
                (c.starts_with('-') && c.len() > 1 && c[1..].bytes().all(|b| b.is_ascii_digit()))
                    || (!c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()))
                    || (c.starts_with('@') && (6..=33).contains(&c.len()));
            if !token_ok {
                return Err("enter the bot token from @BotFather (digits:letters)".into());
            }
            if !chat_ok {
                return Err("enter the chat id (a number, or @channelname)".into());
            }
            true
        }
    };
    if ok {
        Ok(())
    } else {
        Err(match channel {
            Channel::Wecom | Channel::Webhook => "enter the full https:// URL".into(),
            _ => "enter the key, or the full https:// URL".into(),
        })
    }
}

/// PUT /api/sms/forward — any of {enabled, channel, target, chat_id, via_proxy}
pub fn set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let obj = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => return (400, json!({"ok": false, "error": "expected a JSON object"})),
    };
    if let Some(k) = obj
        .keys()
        .find(|k| !["enabled", "channel", "target", "chat_id", "via_proxy"].contains(&k.as_str()))
    {
        return (
            400,
            json!({"ok": false, "error": format!("unknown field {k}")}),
        );
    }
    let _guard = LOCK.safe_lock();
    let mut cfg = load();
    let was_enabled = cfg.enabled;
    if let Some(c) = obj.get("channel") {
        match serde_json::from_value::<Channel>(c.clone()) {
            Ok(c) => cfg.channel = c,
            Err(_) => {
                return (
                    400,
                    json!({"ok": false, "error": "channel must be bark, serverchan, wecom, telegram or webhook"}),
                )
            }
        }
    }
    for (key, slot) in [("target", &mut cfg.target), ("chat_id", &mut cfg.chat_id)] {
        match obj.get(key) {
            None => {}
            Some(Value::String(s)) => *slot = s.trim().to_string(),
            Some(_) => {
                return (
                    400,
                    json!({"ok": false, "error": format!("{key} must be a string")}),
                )
            }
        }
    }
    for (key, slot) in [
        ("enabled", &mut cfg.enabled),
        ("via_proxy", &mut cfg.via_proxy),
    ] {
        match obj.get(key) {
            None => {}
            Some(Value::Bool(b)) => *slot = *b,
            Some(_) => {
                return (
                    400,
                    json!({"ok": false, "error": format!("{key} must be a boolean")}),
                )
            }
        }
    }
    let touched_target =
        obj.contains_key("target") || obj.contains_key("chat_id") || obj.contains_key("channel");
    // Validate when forwarding is on, or when a new destination is entered.
    if cfg.enabled || (touched_target && !cfg.target.is_empty()) {
        if let Err(e) = validate_target(cfg.channel, &cfg.target, &cfg.chat_id) {
            return (400, json!({"ok": false, "error": e}));
        }
    }
    if cfg.enabled && !was_enabled {
        // Start after the newest stored message: history is not sent.
        match newest_messages() {
            Ok(m) => cfg.last_id = Some(m.iter().map(|m| m.id).max().unwrap_or(0)),
            Err(e) => {
                return (
                    503,
                    json!({"ok": false, "error": format!("SMS could not be read: {e}")}),
                )
            }
        }
        cfg.last_error = None;
    }
    if let Err(e) = save(&cfg) {
        return (503, json!({"ok": false, "error": e}));
    }
    (200, json!({"ok": true, "data": view(&cfg)}))
}

/// POST /api/sms/forward/test — send one test message with the saved settings.
pub fn test(state: &AppState) -> (u16, Value) {
    let cfg = {
        let _guard = LOCK.safe_lock();
        load()
    };
    if let Err(e) = validate_target(cfg.channel, &cfg.target, &cfg.chat_id) {
        return (400, json!({"ok": false, "error": e}));
    }
    let result = proxy_for(&cfg, state).and_then(|proxy| {
        send(
            &cfg,
            proxy.as_deref(),
            "U60 Pro",
            &format!("SMS forwarding test {}", now_text()),
        )
    });
    match result {
        Ok(()) => (200, json!({"ok": true, "data": {"sent": true}})),
        Err(e) => (502, json!({"ok": false, "error": e})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_stock_messages_oldest_first() {
        let data = json!({"messages": [
            {"id": 12, "tag": "1", "number": "10086", "content": "4F60597D", "date": "26,09,07,14,52,19,+32"},
            {"id": 11, "tag": "2", "number": "x", "content": "sent", "date": ""},
            {"id": 10, "tag": "0", "number": "002B00380036", "content": "plain text", "date": "26,09,07,14,41,31,+32"},
        ]});
        let m = parse_received(&data);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, 10);
        assert_eq!(m[0].from, "+86");
        assert_eq!(m[0].text, "plain text");
        assert_eq!(m[1].text, "你好");
        assert_eq!(m[1].time, "2026-09-07 14:52:19");
    }

    #[test]
    fn requests_per_channel() {
        let mut cfg = Config {
            channel: Channel::Bark,
            target: "abcdefghijk".into(),
            ..Config::default()
        };
        let (url, body) = request(&cfg, "10086", "hi").unwrap();
        assert_eq!(url, "https://api.day.app/abcdefghijk");
        assert_eq!(body["body"], "hi");
        cfg.channel = Channel::Telegram;
        cfg.target = "123:abc".into();
        cfg.chat_id = "-100".into();
        let (url, body) = request(&cfg, "10086", "hi").unwrap();
        assert!(url.ends_with("/bot123:abc/sendMessage"));
        assert_eq!(body["chat_id"], "-100");
        cfg.channel = Channel::Wecom;
        cfg.target = "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x".into();
        assert_eq!(request(&cfg, "a", "b").unwrap().1["msgtype"], "text");
    }

    #[test]
    fn targets_are_validated() {
        assert!(validate_target(Channel::Bark, "AbCdEfGhIjK", "").is_ok());
        assert!(validate_target(Channel::Bark, "short", "").is_err());
        assert!(validate_target(Channel::Wecom, "not a url", "").is_err());
        assert!(validate_target(Channel::Webhook, "https://example.com/hook", "").is_ok());
        assert!(validate_target(
            Channel::Telegram,
            "12345:AAbbCCddEEffGGhhIIjjKK",
            "-1001234"
        )
        .is_ok());
        assert!(validate_target(Channel::Telegram, "12345:AAbbCCddEEffGGhhIIjjKK", "").is_err());
        assert!(validate_target(Channel::Telegram, "nope", "1").is_err());
    }

    #[test]
    fn replies_with_error_codes_fail() {
        assert!(check_reply(Channel::Bark, r#"{"code":200,"message":"success"}"#).is_ok());
        assert!(check_reply(
            Channel::Bark,
            r#"{"code":400,"message":"failed to get device token"}"#
        )
        .is_err());
        assert!(check_reply(
            Channel::Wecom,
            r#"{"errcode":93000,"errmsg":"invalid webhook url"}"#
        )
        .is_err());
        assert!(check_reply(
            Channel::Telegram,
            r#"{"ok":false,"description":"chat not found"}"#
        )
        .is_err());
        assert!(check_reply(Channel::Webhook, "OK").is_ok());
    }

    #[test]
    fn secrets_are_not_shown() {
        let cfg = Config {
            target: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=secret".into(),
            ..Config::default()
        };
        assert_eq!(hint(&cfg).unwrap(), "qyapi.weixin.qq.com/…");
        let key = Config {
            target: "SCT12345abcdef".into(),
            ..Config::default()
        };
        assert_eq!(hint(&key).unwrap(), "SCT1…");
        assert_eq!(
            mask_secret("bad key SCT12345abcdef", "SCT12345abcdef"),
            "bad key …"
        );
    }
}
