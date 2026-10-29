//! Work the PROXY page starts, off the drawing thread: switching a node and
//! delay tests against mihomo, and requests to zte-agent for what it owns.
//!
//! The agent reads `agent.request` from the session directory (root-only)
//! about every 400 ms while a session runs, does the one thing asked, and
//! answers in `agent.reply`. It accepts nothing else; see
//! agent/src/screen.rs.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};

use super::{call, error_text, refresh_soon, segment, HEALTH_URL};
use crate::pet::clock;

/// The session directory (`supervisor::RUNDIR`, which is Linux-only).
const RUNDIR: &str = "/tmp/zte-screen";
/// How long an agent request may take: a subscription download can be slow.
const AGENT_TIMEOUT_MS: i64 = 90_000;
const TOAST_MS: i64 = 4_000;
/// A test result stands in for mihomo's history until the next read has it.
const FRESH_MS: i64 = 8_000;

/// What the screen may ask of the agent.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentOp {
    /// rule, global or direct.
    Mode(&'static str),
    Restart,
    /// One subscription (id, name), or all of them.
    Update(Option<(String, String)>),
    /// The subscription list, without URLs.
    Subs,
}

impl AgentOp {
    fn body(&self, id: u64) -> Value {
        match self {
            AgentOp::Mode(m) => json!({"id": id, "op": "mode", "mode": m}),
            AgentOp::Restart => json!({"id": id, "op": "restart"}),
            AgentOp::Update(Some((sub, _))) => json!({"id": id, "op": "update", "sub": sub}),
            AgentOp::Update(None) => json!({"id": id, "op": "update"}),
            AgentOp::Subs => json!({"id": id, "op": "subs"}),
        }
    }

    /// What is happening, for the status line.
    pub fn doing(&self) -> String {
        match self {
            AgentOp::Mode(m) => format!("切换到{}模式…", mode_name(m)),
            AgentOp::Restart => "正在重启 mihomo…".into(),
            AgentOp::Update(Some((_, name))) => format!("正在更新「{name}」…"),
            AgentOp::Update(None) => "正在更新全部订阅…".into(),
            AgentOp::Subs => "正在读取订阅…".into(),
        }
    }

    fn done(&self) -> String {
        match self {
            AgentOp::Mode(m) => format!("已切换到{}模式", mode_name(m)),
            AgentOp::Restart => "mihomo 已重启".into(),
            AgentOp::Update(Some((_, name))) => format!("「{name}」已更新"),
            AgentOp::Update(None) => "订阅已全部更新".into(),
            AgentOp::Subs => String::new(),
        }
    }
}

pub fn mode_name(m: &str) -> &'static str {
    match m {
        "rule" => "规则",
        "global" => "全局",
        "direct" => "直连",
        _ => "未知",
    }
}

/// One subscription as the agent reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sub {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub nodes: Option<u64>,
    /// RFC 3339.
    pub updated: Option<String>,
    pub used: Option<u64>,
    pub total: Option<u64>,
    /// Unix seconds.
    pub expire: Option<u64>,
    pub error: Option<String>,
}

pub fn parse_subs(data: &Value) -> Vec<Sub> {
    data["subscriptions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let text = |v: &Value| v.as_str().map(str::to_string);
            let usage = &s["usage"];
            Sub {
                id: text(&s["id"]).unwrap_or_default(),
                name: text(&s["name"]).unwrap_or_default(),
                enabled: s["enabled"].as_bool().unwrap_or(false),
                nodes: s["node_count"].as_u64(),
                updated: text(&s["updated_at"]),
                used: usage["upload"]
                    .as_u64()
                    .zip(usage["download"].as_u64())
                    .map(|(u, d)| u + d),
                total: usage["total"].as_u64().filter(|t| *t > 0),
                expire: usage["expire"].as_u64().filter(|t| *t > 0),
                error: text(&s["error"]),
            }
        })
        .collect()
}

#[derive(Default)]
struct State {
    busy: Option<String>,
    /// Text, good news, shown until.
    toast: Option<(String, bool, i64)>,
    /// Results of the latest delay tests, and when they came in.
    delays: HashMap<String, (u32, i64)>,
    subs: Option<Result<Vec<Sub>, String>>,
    /// The agent request in flight: id, what, sent at.
    waiting: Option<(u64, AgentOp, i64)>,
    next_id: u64,
}

impl State {
    fn say(&mut self, text: String, ok: bool) {
        self.toast = Some((text, ok, clock() + TOAST_MS));
    }
}

#[derive(Clone, Default)]
pub struct Jobs(Arc<Mutex<State>>);

impl Jobs {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn spawn(&self, busy: String, work: impl FnOnce(&Jobs) + Send + 'static) -> bool {
        {
            let mut s = self.lock();
            if s.busy.is_some() {
                return false;
            }
            s.busy = Some(busy);
        }
        let me = self.clone();
        std::thread::spawn(move || {
            work(&me);
            me.lock().busy = None;
        });
        true
    }

    /// Choose `node` in the select group `group`. Takes effect at once.
    pub fn select(&self, group: &str, node: &str) {
        let (group, node) = (group.to_string(), node.to_string());
        self.spawn(format!("切换到 {node}…"), move |me| {
            let path = format!("/proxies/{}", segment(&group));
            let r = call(
                "PUT",
                &path,
                Some(&json!({ "name": node })),
                Duration::from_secs(5),
            );
            refresh_soon();
            let mut s = me.lock();
            match r {
                Ok((204 | 200, _)) => s.say(format!("{group} → {node}"), true),
                Ok((status, body)) => {
                    s.say(format!("切换失败：{}", error_text(status, &body)), false)
                }
                Err(e) => s.say(format!("切换失败：{e}"), false),
            }
        });
    }

    /// Test every member of `group` (GLOBAL tests everything).
    pub fn test(&self, group: &str) {
        let group = group.to_string();
        let label = if group == "GLOBAL" {
            "全部节点".to_string()
        } else {
            group.clone()
        };
        self.spawn(format!("测速中：{label}…"), move |me| {
            let url: String = HEALTH_URL
                .bytes()
                .map(|b| match b {
                    b':' => "%3A".to_string(),
                    b'/' => "%2F".to_string(),
                    _ => (b as char).to_string(),
                })
                .collect();
            let path = format!("/group/{}/delay?url={url}&timeout=5000", segment(&group));
            let r = call("GET", &path, None, Duration::from_secs(40));
            refresh_soon();
            let mut s = me.lock();
            match r {
                Ok((200, body)) => {
                    let map: HashMap<String, u32> = serde_json::from_slice::<Value>(&body)
                        .ok()
                        .and_then(|v| v.as_object().cloned())
                        .into_iter()
                        .flatten()
                        .filter_map(|(k, v)| Some((k, v.as_u64()? as u32)))
                        .collect();
                    let ok = map.values().filter(|d| **d > 0).count();
                    s.say(format!("测速完成：{ok}/{} 个可用", map.len()), true);
                    let now = clock();
                    s.delays.extend(map.into_iter().map(|(k, d)| (k, (d, now))));
                }
                Ok((status, body)) => {
                    s.say(format!("测速失败：{}", error_text(status, &body)), false)
                }
                Err(e) => s.say(format!("测速失败：{e}"), false),
            }
        });
    }

    /// Hand `op` to the agent. One request at a time.
    pub fn ask(&self, op: AgentOp) {
        let mut s = self.lock();
        if s.waiting.is_some() || s.busy.is_some() {
            s.say("还有操作没完成，稍等".into(), false);
            return;
        }
        s.next_id = s.next_id.wrapping_add(1).max(1);
        // Mixed with the clock so a reply left by an earlier panel never matches.
        let id = s.next_id ^ ((clock() as u64) << 16);
        let tmp = format!("{RUNDIR}/agent.request.tmp");
        let path = format!("{RUNDIR}/agent.request");
        let _ = fs::remove_file(format!("{RUNDIR}/agent.reply"));
        let sent = fs::write(&tmp, op.body(id).to_string()).and_then(|_| fs::rename(&tmp, &path));
        match sent {
            Ok(()) => {
                s.busy = Some(op.doing());
                s.waiting = Some((id, op, clock()));
            }
            Err(e) => s.say(format!("写请求失败：{e}"), false),
        }
    }

    /// Pick up the agent's answer, or give up on it. Call about once a second.
    pub fn poll(&self) {
        let mut s = self.lock();
        let Some((id, op, sent)) = s.waiting.clone() else {
            return;
        };
        let path = format!("{RUNDIR}/agent.reply");
        let reply = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["id"].as_u64() == Some(id));
        let Some(reply) = reply else {
            if clock() - sent > AGENT_TIMEOUT_MS {
                let _ = fs::remove_file(format!("{RUNDIR}/agent.request"));
                s.waiting = None;
                s.busy = None;
                s.say("agent 没有回应".into(), false);
                if op == AgentOp::Subs {
                    s.subs = Some(Err("agent 没有回应".into()));
                }
            }
            return;
        };
        let _ = fs::remove_file(&path);
        s.waiting = None;
        s.busy = None;
        let ok = reply["ok"].as_bool() == Some(true);
        let error = reply["error"].as_str().unwrap_or("失败").to_string();
        match (&op, ok) {
            (AgentOp::Subs, true) => s.subs = Some(Ok(parse_subs(&reply["data"]))),
            (AgentOp::Subs, false) => s.subs = Some(Err(error)),
            (AgentOp::Update(_), true) => {
                // Per-subscription results: report the first failure.
                let failed = reply["data"]["results"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .find_map(|(_, r)| r["error"].as_str().map(str::to_string));
                match failed {
                    Some(e) => s.say(format!("更新失败：{e}"), false),
                    None => s.say(op.done(), true),
                }
            }
            (_, true) => s.say(op.done(), true),
            (_, false) => s.say(format!("失败：{error}"), false),
        }
        if matches!(op, AgentOp::Update(_) | AgentOp::Restart) {
            // The list's counts and times are stale now: read it again.
            s.subs = None;
        }
        refresh_soon();
    }

    /// A passing remark on the status line.
    pub fn note(&self, text: String) {
        self.lock().say(text, true);
    }

    pub fn busy(&self) -> Option<String> {
        self.lock().busy.clone()
    }

    pub fn toast(&self) -> Option<(String, bool)> {
        let s = self.lock();
        s.toast
            .as_ref()
            .filter(|t| clock() < t.2)
            .map(|(t, ok, _)| (t.clone(), *ok))
    }

    /// A delay from a test just now, newer than mihomo's last read.
    pub fn fresh_delay(&self, name: &str) -> Option<u32> {
        let s = self.lock();
        s.delays
            .get(name)
            .filter(|(_, at)| clock() - at < FRESH_MS)
            .map(|(d, _)| *d)
    }

    /// For `--preview`: as if the agent had answered.
    pub fn show(&self, subs: Vec<Sub>, toast: &str) {
        let mut s = self.lock();
        s.subs = Some(Ok(subs));
        if !toast.is_empty() {
            s.toast = Some((toast.to_string(), true, i64::MAX));
        }
    }

    pub fn subs(&self) -> Option<Result<Vec<Sub>, String>> {
        self.lock().subs.clone()
    }

    pub fn forget_subs(&self) {
        self.lock().subs = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_carry_only_what_the_agent_accepts() {
        assert_eq!(
            AgentOp::Mode("global").body(7),
            json!({"id": 7, "op": "mode", "mode": "global"})
        );
        assert_eq!(
            AgentOp::Update(Some(("abc".into(), "机场".into()))).body(8),
            json!({"id": 8, "op": "update", "sub": "abc"})
        );
        assert_eq!(AgentOp::Update(None).body(9)["op"], "update");
        assert!(AgentOp::Update(None).body(9).get("sub").is_none());
    }

    #[test]
    fn subscriptions_parse_usage_and_errors() {
        let subs = parse_subs(&json!({"subscriptions": [
            {"id": "a", "name": "机场", "enabled": true, "node_count": 31,
             "updated_at": "2026-10-29T08:00:00Z",
             "usage": {"upload": 1, "download": 2, "total": 100, "expire": 0}},
            {"id": "b", "name": "备用", "enabled": false, "error": "timeout"}
        ]}));
        assert_eq!(subs.len(), 2);
        assert_eq!(
            (subs[0].nodes, subs[0].used, subs[0].total),
            (Some(31), Some(3), Some(100))
        );
        assert_eq!(subs[0].expire, None, "0 means no expiry");
        assert_eq!(subs[1].error.as_deref(), Some("timeout"));
    }
}
