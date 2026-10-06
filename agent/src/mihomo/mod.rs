//! mihomo proxy manager: subscriptions, node selection, service control and
//! the optional LAN TUN, behind `/api/proxy/*`.
//!
//! Two config sources:
//! - managed: subscriptions become node providers, rules come from a preset;
//! - profile: one subscription's own full config (groups, rules, rule sets)
//!   with the router-specific keys replaced (`config::sanitize_profile`).
//!
//! State lives in /data/mihomo/manager.json (0600: it holds subscription URLs
//! and the controller secret). Every change follows render → `mihomo -t` →
//! atomic replace → hot reload, and is rolled back when the reload fails.

mod config;
mod controller;
mod profile;
mod service;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::storage::atomic_write;
use crate::util::MutexExt;
use config::{Mode, Preset, State, Subscription};
use controller::{segment, Controller};
use service::Process;

const STATE_FILE: &str = "/data/mihomo/manager.json";
const PROVIDERS_DIR: &str = "/data/mihomo/providers";
const TICK: Duration = Duration::from_secs(5);
const FIREWALL_CHECK_EVERY: Duration = Duration::from_secs(15);
const CRASH_WINDOW: Duration = Duration::from_secs(600);
const TUN_CRASH_LIMIT: usize = 3;
const BACKOFF_MAX: Duration = Duration::from_secs(300);
/// Minimum gap between automatic profile downloads after a failure.
const PROFILE_RETRY: Duration = Duration::from_secs(1800);
const GROUP_TYPES: &[&str] = &["Selector", "URLTest", "Fallback", "LoadBalance", "Relay"];

pub struct Manager {
    inner: Mutex<Inner>,
}

struct Inner {
    state: State,
    process: Option<Process>,
    version: Option<String>,
    restarts: u32,
    crashes: VecDeque<Instant>,
    backoff: Duration,
    retry_at: Option<Instant>,
    last_error: Option<String>,
    notice: Option<String>,
    sub_errors: HashMap<String, String>,
    traffic: Option<(Instant, u64, u64)>,
    firewall_checked: Option<Instant>,
    profile_attempt: Option<Instant>,
}

fn bad(msg: impl Into<String>) -> (u16, Value) {
    (400, json!({"ok": false, "error": msg.into()}))
}

fn conflict(msg: impl Into<String>) -> (u16, Value) {
    (409, json!({"ok": false, "error": msg.into()}))
}

fn not_found() -> (u16, Value) {
    (404, json!({"ok": false, "error": "subscription not found"}))
}

fn parse(body: &[u8]) -> Result<Value, (u16, Value)> {
    let v: Value = serde_json::from_slice(body).map_err(|_| bad("invalid JSON"))?;
    if !v.is_object() {
        return Err(bad("expected a JSON object"));
    }
    Ok(v)
}

fn lan_ip() -> Result<String, String> {
    crate::ubus::uci_get("zwrt_router.network.lan_ipaddr")
        .ok()
        .filter(|ip| {
            ip.parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.is_private())
        })
        .ok_or_else(|| "could not read the router's LAN address".to_string())
}

fn random_hex(bytes: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; bytes];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn load_state() -> State {
    let mut state = fs::read(STATE_FILE)
        .ok()
        .and_then(|b| serde_json::from_slice::<State>(&b).ok())
        .unwrap_or_default();
    if state.secret.is_empty() {
        state.secret = random_hex(16);
    }
    state
}

fn save_state(state: &State) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    atomic_write(Path::new(STATE_FILE), &bytes)
        .map_err(|e| format!("cannot save {STATE_FILE}: {e}"))
}

/// Unix seconds → `YYYY-MM-DDTHH:MM:SSZ` (civil-from-days, no date crate).
fn iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn usage_json(u: &config::Usage) -> Value {
    json!({"upload": u.upload, "download": u.download, "total": u.total, "expire": u.expire})
}

impl Manager {
    pub fn new() -> Self {
        let process = service::adopt();
        if let Some(p) = &process {
            eprintln!("[mihomo] adopted running mihomo (pid {})", p.pid);
        }
        Self {
            inner: Mutex::new(Inner {
                state: load_state(),
                process,
                version: None,
                restarts: 0,
                crashes: VecDeque::new(),
                backoff: Duration::from_secs(5),
                retry_at: None,
                last_error: None,
                notice: None,
                sub_errors: HashMap::new(),
                traffic: None,
                firewall_checked: None,
                profile_attempt: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.safe_lock()
    }

    /// Background supervisor: (re)starts mihomo when it should run, keeps the
    /// TUN firewall accepts in place across fw3/QCMAP reloads, refreshes the
    /// profile subscription on its interval, caps the log.
    pub fn start_watchdog(self: &Arc<Self>) {
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            // Give the LAN bridge and the WAN time to come up after boot.
            std::thread::sleep(Duration::from_secs(10));
            let mut ticks: u64 = 0;
            loop {
                me.lock().tick();
                if let Some(sub) = me.lock().profile_due() {
                    if let Err(e) = me.refresh_profile(&sub) {
                        eprintln!("[mihomo] scheduled profile update failed: {e}");
                    }
                }
                ticks += 1;
                if ticks % 12 == 0 {
                    service::truncate_log_if_large();
                }
                std::thread::sleep(TICK);
            }
        });
    }

    /// Download `sub` (without holding the lock) and, if it is still the
    /// same subscription, store the result and re-apply the config.
    fn refresh_profile(&self, sub: &Subscription) -> Result<(), String> {
        let fetched = profile::fetch(sub);
        let mut inner = self.lock();
        let Some(idx) = inner
            .find_sub(&sub.id)
            .filter(|&i| inner.state.subscriptions[i].url == sub.url)
        else {
            return Err("the subscription changed while it was downloading".into());
        };
        let fetched = match fetched {
            Ok(f) => f,
            Err(e) => {
                inner.sub_errors.insert(sub.id.clone(), e.clone());
                return Err(e);
            }
        };
        let mut next = inner.state.clone();
        next.subscriptions[idx].fetched = Some(fetched);
        let result = inner.apply(next);
        match &result {
            Ok(()) => inner.sub_errors.remove(&sub.id),
            Err(e) => inner.sub_errors.insert(sub.id.clone(), e.clone()),
        };
        result
    }
}

impl Inner {
    fn secret(&self) -> String {
        self.state.secret.clone()
    }

    fn alive(&mut self) -> bool {
        self.process.as_mut().is_some_and(Process::alive)
    }

    fn find_sub(&self, id: &str) -> Option<usize> {
        self.state.subscriptions.iter().position(|s| s.id == id)
    }

    fn profile_sub(&self) -> Option<&Subscription> {
        let id = self.state.settings.profile.as_deref()?;
        self.state.subscriptions.iter().find(|s| s.id == id)
    }

    /// The profile subscription when its auto-update interval has elapsed.
    fn profile_due(&mut self) -> Option<Subscription> {
        let sub = self.profile_sub()?.clone();
        if sub.interval_hours == 0 || !self.state.settings.enabled {
            return None;
        }
        let last = sub.fetched.as_ref().map_or(0, |f| f.at);
        let due = last + u64::from(sub.interval_hours) * 3600 <= profile::now_secs();
        if !due
            || self
                .profile_attempt
                .is_some_and(|t| t.elapsed() < PROFILE_RETRY)
        {
            return None;
        }
        self.profile_attempt = Some(Instant::now());
        Some(sub)
    }

    fn tick(&mut self) {
        // Reap an exited process and remember why it died.
        if self.process.is_some() && !self.alive() {
            self.process = None;
            self.crashes.push_back(Instant::now());
            self.last_error =
                service::log_tail_error().or(Some("mihomo exited unexpectedly".into()));
            eprintln!(
                "[mihomo] exited unexpectedly: {}",
                self.last_error.as_deref().unwrap_or("")
            );
            service::firewall_remove();
            service::cleanup_routing();
        }
        while self
            .crashes
            .front()
            .is_some_and(|t| t.elapsed() > CRASH_WINDOW)
        {
            self.crashes.pop_front();
        }

        if self.state.settings.enabled && self.process.is_none() {
            if self.retry_at.is_some_and(|t| Instant::now() < t) || !service::installed() {
                return;
            }
            if self.state.settings.tun && self.crashes.len() >= TUN_CRASH_LIMIT {
                let mut next = self.state.clone();
                next.settings.tun = false;
                if self.write_config(&next).is_ok() && save_state(&next).is_ok() {
                    self.state = next;
                    self.notice = Some(format!(
                        "TUN was turned off after mihomo stopped {TUN_CRASH_LIMIT} times within 10 minutes. \
                         The proxy port still works; re-enable TUN once the cause is fixed."
                    ));
                    eprintln!("[mihomo] disabled TUN after repeated failures");
                }
            }
            match self.start() {
                Ok(()) => {
                    self.restarts += 1;
                    self.backoff = Duration::from_secs(5);
                    self.retry_at = None;
                }
                Err(e) => {
                    self.last_error = Some(e);
                    self.retry_at = Some(Instant::now() + self.backoff);
                    self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
                }
            }
            return;
        }

        if self.state.settings.tun
            && self.alive()
            && self
                .firewall_checked
                .is_none_or(|t| t.elapsed() >= FIREWALL_CHECK_EVERY)
        {
            self.firewall_checked = Some(Instant::now());
            if !service::firewall_present() {
                eprintln!("[mihomo] TUN firewall accepts were missing (fw3 reload?) — restoring");
                if let Err(e) = service::firewall_add() {
                    self.last_error = Some(e);
                }
            }
        }
    }

    /// Render `next`, validate it with `mihomo -t`, then atomically make it the
    /// live config file. Returns the previous file contents for rollback.
    fn write_config(&self, next: &State) -> Result<Option<Vec<u8>>, String> {
        let ip = lan_ip()?;
        let base = match &next.settings.profile {
            Some(id) => Some(profile::load(id)?),
            None => None,
        };
        let rendered = config::render(next, &ip, base.as_ref())?;
        let bytes = serde_json::to_vec_pretty(&rendered).map_err(|e| e.to_string())?;
        atomic_write(Path::new(service::STAGED), &bytes)
            .map_err(|e| format!("cannot write config: {e}"))?;
        let tested = service::test_config(service::STAGED);
        let previous = fs::read(service::CONFIG).ok();
        let result = tested.and_then(|_| {
            fs::rename(service::STAGED, service::CONFIG)
                .map_err(|e| format!("cannot install config: {e}"))
        });
        let _ = fs::remove_file(service::STAGED);
        result.map(|_| previous)
    }

    fn start(&mut self) -> Result<(), String> {
        if !service::installed() {
            return Err(format!(
                "mihomo is not installed (expected {})",
                service::BINARY
            ));
        }
        let ip = lan_ip()?;
        if !service::address_ready(&ip) {
            return Err(format!("LAN address {ip} is not up yet"));
        }
        // The LAN address may have changed since the file was written.
        let state = self.state.clone();
        self.write_config(&state)?;
        service::cleanup_routing();
        if self.state.settings.tun {
            service::firewall_add()?;
        }
        let mut process = service::spawn()?;
        // Catch an immediate failure (bad port, TUN error) while the user waits.
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(200));
            if !process.alive() {
                service::firewall_remove();
                service::cleanup_routing();
                return Err(service::log_tail_error()
                    .unwrap_or_else(|| "mihomo exited during start".into()));
            }
        }
        eprintln!(
            "[mihomo] started (pid {}, tun {})",
            process.pid, self.state.settings.tun
        );
        self.process = Some(process);
        self.traffic = None;
        self.last_error = None;
        if self.version.is_none() {
            self.version = service::version();
        }
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(process) = self.process.take() {
            let forced = process.terminate();
            eprintln!("[mihomo] stopped{}", if forced { " (killed)" } else { "" });
        }
        service::firewall_remove();
        service::cleanup_routing();
        self.traffic = None;
    }

    fn restart(&mut self) -> Result<(), String> {
        self.stop();
        self.start()
    }

    /// Make `next` the desired state and bring the running process in line.
    fn apply(&mut self, next: State) -> Result<(), String> {
        let previous_state = self.state.clone();
        let previous_file = self.write_config(&next)?;
        save_state(&next)?;
        self.state = next;

        let running = self.alive();
        let needs_restart = previous_state.settings.tun != self.state.settings.tun
            || previous_state.settings.mixed_port != self.state.settings.mixed_port;

        let outcome = match (self.state.settings.enabled, running) {
            (true, true) if needs_restart => self.restart(),
            (true, true) => self.reload(),
            (true, false) => self.start(),
            (false, true) => {
                self.stop();
                Ok(())
            }
            (false, false) => Ok(()),
        };
        if let Err(e) = outcome {
            // Put the previous config and state back, and the process with them.
            if let Some(bytes) = previous_file {
                let _ = atomic_write(Path::new(service::CONFIG), &bytes);
            }
            let _ = save_state(&previous_state);
            self.state = previous_state;
            if self.alive() {
                let _ = self.reload();
            } else if self.state.settings.enabled {
                let _ = self.start();
            }
            return Err(e);
        }
        self.notice = None;
        Ok(())
    }

    fn reload(&self) -> Result<(), String> {
        let secret = self.secret();
        let reply = Controller { secret: &secret }.request(
            "PUT",
            "/configs?force=true",
            Some(&json!({"path": service::CONFIG})),
            Duration::from_secs(30),
        )?;
        if reply.ok() {
            Ok(())
        } else {
            Err(format!(
                "mihomo rejected the reload: {}",
                reply.error_message()
            ))
        }
    }

    fn status(&mut self) -> Value {
        let running = self.alive();
        let s = self.state.settings.clone();
        let ip = lan_ip().ok();
        let mut traffic = Value::Null;
        let mut route = Value::Null;
        if running {
            let secret = self.secret();
            let ctl = Controller { secret: &secret };
            if let Ok(c) = ctl.get("/connections") {
                let up = c["uploadTotal"].as_u64().unwrap_or(0);
                let down = c["downloadTotal"].as_u64().unwrap_or(0);
                let now = Instant::now();
                let (up_rate, down_rate) = match self.traffic {
                    Some((t, u, d)) if now > t && up >= u && down >= d => {
                        let secs = (now - t).as_secs_f64().max(0.001);
                        (
                            json!(((up - u) as f64 / secs) as u64),
                            json!(((down - d) as f64 / secs) as u64),
                        )
                    }
                    _ => (Value::Null, Value::Null),
                };
                self.traffic = Some((now, up, down));
                traffic = json!({
                    "up_total": up, "down_total": down,
                    "up_rate": up_rate, "down_rate": down_rate,
                    "connections": c["connections"].as_array().map_or(0, Vec::len),
                });
            }
            if let Ok(p) = ctl.get("/proxies") {
                route = main_route(&p["proxies"]);
            }
        }
        let process = self.process.as_ref();
        let profile = self
            .profile_sub()
            .map(|p| json!({"id": p.id, "name": p.name}));
        json!({
            "installed": service::installed(),
            "version": self.version,
            "running": running,
            "pid": process.map(|p| p.pid),
            "uptime_secs": process.filter(|_| running).map(|p| p.started.elapsed().as_secs()),
            "rss_bytes": process.filter(|_| running).and_then(Process::rss_bytes),
            "enabled": s.enabled,
            "mode": s.mode,
            "preset": s.preset,
            "profile": profile,
            "tun": s.tun,
            "tun_active": service::tun_active(),
            "mixed_port": s.mixed_port,
            "lan_ip": ip,
            "proxy_address": ip.as_ref().map(|ip| format!("{ip}:{}", s.mixed_port)),
            "pac_url": ip.as_ref().map(|ip| format!("http://{ip}:9090/proxy.pac")),
            "subscriptions": self.state.subscriptions.len(),
            "traffic": traffic,
            "route": route,
            "restarts": self.restarts,
            "last_error": self.last_error,
            "notice": self.notice,
        })
    }
}

fn is_group(p: &Value) -> bool {
    p["type"].as_str().is_some_and(|t| GROUP_TYPES.contains(&t))
}

/// Groups and proxies in config order (mihomo's GLOBAL group lists them so).
fn ordered_names(proxies: &Value) -> Vec<String> {
    let map = match proxies.as_object() {
        Some(m) => m,
        None => return Vec::new(),
    };
    let mut names: Vec<String> = proxies["GLOBAL"]["all"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .filter(|n| map.contains_key(n))
        .collect();
    let seen: HashSet<String> = names.iter().cloned().collect();
    let mut rest: Vec<String> = map
        .keys()
        .filter(|k| !seen.contains(*k) && *k != "GLOBAL")
        .cloned()
        .collect();
    rest.sort();
    names.extend(rest);
    names
}

/// The first selectable group in config order (e.g. PROXY or 节点选择) and the
/// chain its choice resolves through, e.g. [节点选择, 自动选择, HK 01].
fn main_route(proxies: &Value) -> Value {
    let Some(main) = ordered_names(proxies)
        .into_iter()
        .find(|n| proxies[n.as_str()]["type"] == "Selector")
    else {
        return Value::Null;
    };
    let mut chain = vec![main.clone()];
    let mut current = main.clone();
    for _ in 0..8 {
        let Some(next) = proxies[current.as_str()]["now"]
            .as_str()
            .filter(|n| !n.is_empty())
        else {
            break;
        };
        if chain.iter().any(|c| c == next) {
            break;
        }
        chain.push(next.to_string());
        if !is_group(&proxies[next]) {
            break;
        }
        current = next.to_string();
    }
    json!({"group": main, "chain": chain})
}

// ── Handlers ─────────────────────────────────────────────────────────────────

impl Manager {
    /// GET /api/proxy/status
    pub fn status(&self) -> (u16, Value) {
        let mut inner = self.lock();
        if inner.version.is_none() && service::installed() {
            inner.version = service::version();
        }
        (200, json!({"ok": true, "data": inner.status()}))
    }

    /// PUT /api/proxy/settings — {mode?, preset?, tun?, mixed_port?}
    pub fn settings_set(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut inner = self.lock();
        let mut next = inner.state.clone();
        for key in v.as_object().unwrap().keys() {
            if !["mode", "preset", "tun", "mixed_port"].contains(&key.as_str()) {
                return bad(format!("unknown setting '{key}'"));
            }
        }
        if let Some(m) = v.get("mode") {
            next.settings.mode = match serde_json::from_value::<Mode>(m.clone()) {
                Ok(m) => m,
                Err(_) => return bad("mode must be rule, global or direct"),
            };
        }
        if let Some(p) = v.get("preset") {
            next.settings.preset = match serde_json::from_value::<Preset>(p.clone()) {
                Ok(p) => p,
                Err(_) => return bad("preset must be bypass_cn, gfw or proxy_all"),
            };
        }
        if let Some(t) = v.get("tun") {
            match t.as_bool() {
                Some(t) => next.settings.tun = t,
                None => return bad("tun must be a boolean"),
            }
        }
        if let Some(p) = v.get("mixed_port") {
            match p
                .as_u64()
                .ok_or("mixed_port must be a number".to_string())
                .and_then(config::validate_port)
            {
                Ok(p) => next.settings.mixed_port = p,
                Err(e) => return bad(e),
            }
        }
        if next == inner.state {
            return (200, json!({"ok": true, "data": inner.status()}));
        }
        match inner.apply(next) {
            Ok(()) => (200, json!({"ok": true, "data": inner.status()})),
            Err(e) => conflict(e),
        }
    }

    /// POST /api/proxy/service — {action: start|stop|restart}
    pub fn service(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut inner = self.lock();
        let result = match v["action"].as_str() {
            Some("start") => {
                let mut next = inner.state.clone();
                next.settings.enabled = true;
                inner.retry_at = None;
                inner.apply(next)
            }
            Some("stop") => {
                let mut next = inner.state.clone();
                next.settings.enabled = false;
                inner.apply(next)
            }
            Some("restart") => {
                if inner.state.settings.enabled {
                    inner.restart()
                } else {
                    Err("mihomo is not enabled; start it first".into())
                }
            }
            _ => return bad("action must be start, stop or restart"),
        };
        match result {
            Ok(()) => (200, json!({"ok": true, "data": inner.status()})),
            Err(e) => {
                inner.last_error = Some(e.clone());
                conflict(e)
            }
        }
    }

    /// GET /api/proxy/subscriptions
    pub fn subscriptions(&self) -> (u16, Value) {
        let mut inner = self.lock();
        let providers = if inner.alive() {
            let secret = inner.secret();
            Controller { secret: &secret }
                .get("/providers/proxies")
                .ok()
                .and_then(|v| v.get("providers").cloned())
        } else {
            None
        };
        let profile_id = inner.state.settings.profile.clone();
        let list: Vec<Value> = inner
            .state
            .subscriptions
            .iter()
            .map(|s| {
                let use_config = profile_id.as_deref() == Some(s.id.as_str());
                let (nodes, updated, usage) = if use_config {
                    let f = s.fetched.as_ref();
                    (
                        f.map(|f| json!(f.proxies)),
                        f.map(|f| json!(iso8601(f.at))),
                        f.and_then(|f| f.usage.as_ref()).map(usage_json),
                    )
                } else {
                    let p = providers.as_ref().and_then(|p| p.get(s.provider()));
                    let nodes = p.and_then(|p| p["proxies"].as_array()).map(|a| {
                        json!(a
                            .iter()
                            .filter(|x| x["type"].as_str() != Some("Compatible"))
                            .count())
                    });
                    let updated = p
                        .and_then(|p| p["updatedAt"].as_str())
                        .filter(|t| !t.starts_with("0001-"))
                        .map(|t| json!(t));
                    let usage = p
                        .map(|p| &p["subscriptionInfo"])
                        .filter(|i| i.is_object())
                        .map(|i| {
                            json!({
                                "upload": i["Upload"].as_u64(), "download": i["Download"].as_u64(),
                                "total": i["Total"].as_u64(), "expire": i["Expire"].as_u64(),
                            })
                        });
                    (nodes, updated, usage)
                };
                json!({
                    "id": s.id,
                    "name": s.name,
                    "url_masked": config::mask_url(&s.url),
                    "enabled": s.enabled,
                    "interval_hours": s.interval_hours,
                    "use_config": use_config,
                    "full_config": s.fetched.as_ref().map(|f| f.full),
                    "groups": s.fetched.as_ref().filter(|_| use_config).map(|f| f.groups),
                    "node_count": nodes,
                    "updated_at": updated,
                    "usage": usage,
                    "error": inner.sub_errors.get(&s.id),
                })
            })
            .collect();
        (
            200,
            json!({"ok": true, "data": {"subscriptions": list, "running": inner.alive()}}),
        )
    }

    /// POST /api/proxy/subscriptions — {name, url, interval_hours?, use_config?}
    ///
    /// With `use_config` the subscription is downloaded first; if it is a full
    /// config it becomes the profile, otherwise it is added as a node source
    /// and the reply carries a `warning`.
    pub fn subscription_add(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let name = match config::validate_name(v["name"].as_str().unwrap_or("")) {
            Ok(n) => n,
            Err(e) => return bad(e),
        };
        let url = match config::validate_url(v["url"].as_str().unwrap_or("")) {
            Ok(u) => u,
            Err(e) => return bad(e),
        };
        let interval = match config::validate_interval(v["interval_hours"].as_u64().unwrap_or(24)) {
            Ok(i) => i,
            Err(e) => return bad(e),
        };
        let use_config = v["use_config"].as_bool().unwrap_or(false);
        let id = {
            let inner = self.lock();
            if let Err(e) = config::can_add_subscription(&inner.state) {
                return bad(e);
            }
            if inner.state.subscriptions.iter().any(|s| s.url == url) {
                return bad("this subscription is already added");
            }
            let mut id = random_hex(4);
            while inner.find_sub(&id).is_some() {
                id = random_hex(4);
            }
            id
        };
        let mut sub = Subscription {
            id: id.clone(),
            name,
            url,
            enabled: true,
            interval_hours: interval,
            fetched: None,
        };
        let mut warning = None;
        if use_config {
            match profile::fetch(&sub) {
                Ok(f) if f.full => sub.fetched = Some(f),
                Ok(f) => {
                    sub.fetched = Some(f);
                    profile::remove(&id);
                    warning = Some(
                        "the subscription has no groups or rules; it was added as a node source",
                    );
                }
                Err(e) => return conflict(e),
            }
        }
        let mut inner = self.lock();
        if inner.state.subscriptions.iter().any(|s| s.url == sub.url) {
            return bad("this subscription is already added");
        }
        let mut next = inner.state.clone();
        if sub.fetched.as_ref().is_some_and(|f| f.full) {
            next.settings.profile = Some(id.clone());
        }
        next.subscriptions.push(sub);
        match inner.apply(next) {
            Ok(()) => (
                200,
                json!({"ok": true, "data": {"id": id, "warning": warning}}),
            ),
            Err(e) => {
                profile::remove(&id);
                conflict(e)
            }
        }
    }

    /// PUT /api/proxy/subscriptions — {id, name?, url?, enabled?, interval_hours?, use_config?}
    pub fn subscription_edit(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let id = v["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            return bad("id is required");
        }
        let (mut next, idx) = {
            let inner = self.lock();
            let Some(idx) = inner.find_sub(&id) else {
                return not_found();
            };
            (inner.state.clone(), idx)
        };
        let old_url = next.subscriptions[idx].url.clone();
        {
            let sub = &mut next.subscriptions[idx];
            if let Some(n) = v.get("name") {
                match config::validate_name(n.as_str().unwrap_or("")) {
                    Ok(n) => sub.name = n,
                    Err(e) => return bad(e),
                }
            }
            if let Some(u) = v
                .get("url")
                .and_then(Value::as_str)
                .filter(|u| !u.trim().is_empty())
            {
                match config::validate_url(u) {
                    Ok(u) => sub.url = u,
                    Err(e) => return bad(e),
                }
            }
            if let Some(e) = v.get("enabled") {
                match e.as_bool() {
                    Some(e) => sub.enabled = e,
                    None => return bad("enabled must be a boolean"),
                }
            }
            if let Some(i) = v.get("interval_hours") {
                match i
                    .as_u64()
                    .ok_or("interval_hours must be a number".to_string())
                    .and_then(config::validate_interval)
                {
                    Ok(i) => sub.interval_hours = i,
                    Err(e) => return bad(e),
                }
            }
        }
        let url_changed = next.subscriptions[idx].url != old_url;
        let was_profile = next.settings.profile.as_deref() == Some(id.as_str());
        let want_profile = v["use_config"].as_bool().unwrap_or(was_profile);
        if !want_profile && was_profile {
            next.settings.profile = None;
        }
        if want_profile && (!was_profile || url_changed) {
            // Download outside the lock; the subscription is re-checked below.
            match profile::fetch(&next.subscriptions[idx]) {
                Ok(f) if f.full => {
                    next.subscriptions[idx].fetched = Some(f);
                    next.subscriptions[idx].enabled = true;
                    next.settings.profile = Some(id.clone());
                }
                Ok(_) => {
                    profile::remove(&id);
                    return bad(
                        "the subscription has no groups or rules, so its own config cannot be used",
                    );
                }
                Err(e) => return conflict(e),
            }
        }
        if url_changed && !want_profile {
            next.subscriptions[idx].fetched = None;
        }
        let mut inner = self.lock();
        if inner.find_sub(&id) != Some(idx) || inner.state.subscriptions[idx].url != old_url {
            return conflict("the subscription changed meanwhile; try again");
        }
        match inner.apply(next) {
            Ok(()) => {
                if url_changed {
                    let _ = fs::remove_file(format!("{PROVIDERS_DIR}/sub-{id}.yaml"));
                    inner.sub_errors.remove(&id);
                }
                if !want_profile {
                    profile::remove(&id);
                }
                (200, json!({"ok": true, "data": {}}))
            }
            Err(e) => conflict(e),
        }
    }

    /// POST /api/proxy/subscriptions/delete — {id}  (X-Confirm)
    pub fn subscription_delete(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let id = v["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            return bad("id is required");
        }
        let mut inner = self.lock();
        let Some(idx) = inner.find_sub(&id) else {
            return not_found();
        };
        let mut next = inner.state.clone();
        next.subscriptions.remove(idx);
        if next.settings.profile.as_deref() == Some(id.as_str()) {
            next.settings.profile = None;
        }
        match inner.apply(next) {
            Ok(()) => {
                let _ = fs::remove_file(format!("{PROVIDERS_DIR}/sub-{id}.yaml"));
                profile::remove(&id);
                inner.sub_errors.remove(&id);
                (200, json!({"ok": true, "data": {}}))
            }
            Err(e) => conflict(e),
        }
    }

    /// POST /api/proxy/subscriptions/update — {id?}: fetch now (one or all).
    pub fn subscription_refresh(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let (secret, running, profile_id, targets) = {
            let mut inner = self.lock();
            let targets: Vec<Subscription> = match v["id"].as_str() {
                Some(id) => match inner.find_sub(id) {
                    Some(i) => vec![inner.state.subscriptions[i].clone()],
                    None => return not_found(),
                },
                None => inner
                    .state
                    .subscriptions
                    .iter()
                    .filter(|s| s.enabled)
                    .cloned()
                    .collect(),
            };
            (
                inner.secret(),
                inner.alive(),
                inner.state.settings.profile.clone(),
                targets,
            )
        };
        let ctl = Controller { secret: &secret };
        let mut results = serde_json::Map::new();
        for sub in &targets {
            let outcome = if profile_id.as_deref() == Some(sub.id.as_str()) {
                self.refresh_profile(sub)
            } else if !sub.enabled {
                Err("subscription is disabled".into())
            } else if profile_id.is_some() {
                Err("not used while another subscription's config is active".into())
            } else if !running {
                Err("start the proxy before updating subscriptions".into())
            } else {
                let path = format!("/providers/proxies/{}", segment(&sub.provider()));
                ctl.request("PUT", &path, None, Duration::from_secs(30))
                    .and_then(|r| {
                        if r.ok() {
                            Ok(())
                        } else {
                            Err(r.error_message())
                        }
                    })
            };
            let mut inner = self.lock();
            match &outcome {
                Ok(()) => inner.sub_errors.remove(&sub.id),
                Err(e) => inner.sub_errors.insert(sub.id.clone(), e.clone()),
            };
            results.insert(
                sub.id.clone(),
                match outcome {
                    Ok(()) => json!({"ok": true}),
                    Err(e) => json!({"ok": false, "error": e}),
                },
            );
        }
        (200, json!({"ok": true, "data": {"results": results}}))
    }

    /// GET /api/proxy/groups — every proxy group in config order with its
    /// members, plus every member proxy with its last delay.
    pub fn groups(&self) -> (u16, Value) {
        let (secret, names) = {
            let mut inner = self.lock();
            if !inner.alive() {
                return (
                    200,
                    json!({"ok": true, "data": {"running": false, "groups": [], "nodes": []}}),
                );
            }
            let names: HashMap<String, (String, String)> = inner
                .state
                .subscriptions
                .iter()
                .map(|s| (s.provider(), (s.id.clone(), s.name.clone())))
                .collect();
            (inner.secret(), names)
        };
        let ctl = Controller { secret: &secret };
        let proxies = match ctl.get("/proxies") {
            Ok(p) => p["proxies"].clone(),
            Err(e) => return (503, json!({"ok": false, "error": e})),
        };
        // Which subscription each node came from (managed mode providers).
        let mut origin: HashMap<String, (String, String)> = HashMap::new();
        if let Ok(p) = ctl.get("/providers/proxies") {
            for (provider, body) in p["providers"].as_object().into_iter().flatten() {
                let Some(sub) = names.get(provider) else {
                    continue;
                };
                for node in body["proxies"].as_array().into_iter().flatten() {
                    if let Some(n) = node["name"].as_str() {
                        origin.insert(n.to_string(), sub.clone());
                    }
                }
            }
        }
        let mut groups = Vec::new();
        let mut nodes = Vec::new();
        for name in ordered_names(&proxies) {
            let p = &proxies[name.as_str()];
            if name == "COMPATIBLE" {
                continue;
            }
            if is_group(p) {
                groups.push(json!({
                    "name": name,
                    "type": p["type"],
                    "now": p["now"],
                    "all": p["all"],
                    "hidden": p["hidden"].as_bool().unwrap_or(false),
                }));
            } else {
                let last = p["history"].as_array().and_then(|h| h.last());
                let sub = origin.get(&name);
                nodes.push(json!({
                    "name": name,
                    "type": p["type"],
                    "udp": p["udp"],
                    "alive": p["alive"],
                    "delay": last.and_then(|h| h["delay"].as_u64()),
                    "subscription_id": sub.map(|s| s.0.clone()),
                    "subscription": sub.map(|s| s.1.clone()),
                }));
            }
        }
        (
            200,
            json!({"ok": true, "data": {"running": true, "groups": groups, "nodes": nodes}}),
        )
    }

    /// PUT /api/proxy/groups — {group, proxy}: choose a member of a select group.
    pub fn group_select(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let Some(group) = v["group"]
            .as_str()
            .filter(|g| !g.is_empty() && *g != "GLOBAL")
        else {
            return bad("group is required");
        };
        let Some(proxy) = v["proxy"].as_str().filter(|p| !p.is_empty()) else {
            return bad("proxy is required");
        };
        let secret = {
            let mut inner = self.lock();
            if !inner.alive() {
                return conflict("the proxy is not running");
            }
            inner.secret()
        };
        let ctl = Controller { secret: &secret };
        let path = format!("/proxies/{}", segment(group));
        match ctl.get(&path) {
            Ok(g) if g["type"] == "Selector" => {
                if !g["all"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|m| m == proxy))
                {
                    return bad("that proxy is not in the group");
                }
            }
            Ok(_) => return bad("only select groups can be switched manually"),
            Err(e) => return (503, json!({"ok": false, "error": e})),
        }
        match ctl.request(
            "PUT",
            &path,
            Some(&json!({"name": proxy})),
            Duration::from_secs(5),
        ) {
            Ok(r) if r.ok() => (
                200,
                json!({"ok": true, "data": {"group": group, "now": proxy}}),
            ),
            Ok(r) => bad(r.error_message()),
            Err(e) => (503, json!({"ok": false, "error": e})),
        }
    }

    /// POST /api/proxy/delay — {group?}: test one group's members, or every
    /// node (health check of each node provider). Returns name → ms for the
    /// group form (0 = timeout); the node list reads delays from history.
    pub fn delay(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let secret = {
            let mut inner = self.lock();
            if !inner.alive() {
                return conflict("the proxy is not running");
            }
            inner.secret()
        };
        let ctl = Controller { secret: &secret };
        let url = config::HEALTH_URL.replace(':', "%3A").replace('/', "%2F");
        if let Some(group) = v["group"].as_str().filter(|g| !g.is_empty()) {
            let path = format!("/group/{}/delay?url={url}&timeout=5000", segment(group));
            return match ctl.request("GET", &path, None, Duration::from_secs(30)) {
                Ok(r) if r.ok() => (
                    200,
                    json!({"ok": true, "data": {"delays": r.json().unwrap_or(Value::Null)}}),
                ),
                Ok(r) => (503, json!({"ok": false, "error": r.error_message()})),
                Err(e) => (503, json!({"ok": false, "error": e})),
            };
        }
        let providers = match ctl.get("/providers/proxies") {
            Ok(p) => p["providers"].clone(),
            Err(e) => return (503, json!({"ok": false, "error": e})),
        };
        // Inline proxies live in the "default" provider; subscriptions and a
        // profile's own proxy-providers are HTTP/File providers. Group-backed
        // pseudo providers are skipped (their members are tested anyway).
        for (name, p) in providers.as_object().into_iter().flatten() {
            let vehicle = p["vehicleType"].as_str().unwrap_or("");
            if name != "default" && vehicle != "HTTP" && vehicle != "File" && vehicle != "Inline" {
                continue;
            }
            let path = format!("/providers/proxies/{}/healthcheck", segment(name));
            let _ = ctl.request("GET", &path, None, Duration::from_secs(30));
        }
        (200, json!({"ok": true, "data": {"delays": {}}}))
    }

    /// GET /proxy.pac (unauthenticated): LAN and plain hosts direct, the rest
    /// via mihomo, falling back to direct when the proxy is down.
    pub fn pac(&self) -> String {
        let port = self.lock().state.settings.mixed_port;
        let ip = lan_ip().unwrap_or_else(|_| "192.168.0.1".into());
        format!(
            "function FindProxyForURL(url, host) {{\n\
             \x20 if (isPlainHostName(host) || host === \"{ip}\" || shExpMatch(host, \"*.local\") ||\n\
             \x20     shExpMatch(host, \"10.*\") || shExpMatch(host, \"192.168.*\") || shExpMatch(host, \"127.*\") ||\n\
             \x20     /^172\\.(1[6-9]|2[0-9]|3[01])\\./.test(host)) return \"DIRECT\";\n\
             \x20 return \"PROXY {ip}:{port}; DIRECT\";\n\
             }}\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "GLOBAL": {"type": "Selector", "now": "DIRECT", "all": ["节点选择", "自动选择", "HK 01", "JP 01"]},
            "节点选择": {"type": "Selector", "now": "自动选择", "all": ["自动选择", "HK 01", "JP 01", "DIRECT"]},
            "自动选择": {"type": "URLTest", "now": "JP 01", "all": ["HK 01", "JP 01"]},
            "HK 01": {"type": "Shadowsocks", "history": [{"delay": 80}]},
            "JP 01": {"type": "Trojan", "history": []},
            "DIRECT": {"type": "Direct"},
        })
    }

    #[test]
    fn route_follows_groups_to_the_node() {
        assert_eq!(
            main_route(&sample()),
            json!({"group": "节点选择", "chain": ["节点选择", "自动选择", "JP 01"]})
        );
        assert_eq!(main_route(&json!({})), Value::Null);
    }

    #[test]
    fn names_follow_config_order_then_the_rest() {
        let names = ordered_names(&sample());
        assert_eq!(&names[..4], &["节点选择", "自动选择", "HK 01", "JP 01"]);
        assert!(names.contains(&"DIRECT".to_string()));
        assert!(!names.contains(&"GLOBAL".to_string()));
    }

    #[test]
    fn iso8601_formats_utc() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_791_331_200), "2026-10-07T00:00:00Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
    }
}

/// End-to-end check against a real U60 Pro (root, /data/mihomo installed):
/// `zte_agent-<hash> --ignored --nocapture --test-threads=1 device_e2e`.
/// Set `E2E_HOLD_SECS` to keep the TUN up for manual client tests.
#[cfg(test)]
mod device_tests {
    use super::*;

    fn ok(label: &str, (status, body): (u16, Value)) -> Value {
        println!("{label}: {status} {body}");
        assert!(status < 300, "{label} failed: {body}");
        body["data"].clone()
    }

    /// Turn TUN on for `E2E_HOLD_SECS` (default 60) so LAN clients can be
    /// tested, then restore the previous TUN setting.
    #[test]
    #[ignore = "turns TUN on for a while on a real U60 Pro, then restores it"]
    fn device_tun_hold() {
        let m = Manager::new();
        let before = m.lock().state.settings.tun;
        let (code, v) = m.settings_set(br#"{"tun":true}"#);
        println!("tun on: {code} error={}", v["error"]);
        assert!(code < 300);
        assert_eq!(v["data"]["tun_active"], true);
        assert!(service::firewall_present());
        println!("READY route={}", v["data"]["route"]);
        let secs = std::env::var("E2E_HOLD_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        std::thread::sleep(Duration::from_secs(secs));
        let (_, st) = m.status();
        println!(
            "after hold: running={} traffic={}",
            st["data"]["running"], st["data"]["traffic"]
        );
        if !before {
            let (code, v) = m.settings_set(br#"{"tun":false}"#);
            println!("tun off: {code} tun_active={}", v["data"]["tun_active"]);
            assert!(code < 300);
            assert!(!service::tun_active());
            assert!(!service::firewall_present());
        }
    }

    /// Switch the first subscription to its own full config and report a
    /// summary (no URLs, no node credentials). Leaves the profile active.
    #[test]
    #[ignore = "switches the first subscription to its own config on a real U60 Pro"]
    fn device_profile() {
        let m = Manager::new();
        let id = m
            .lock()
            .state
            .subscriptions
            .first()
            .map(|s| s.id.clone())
            .expect("add a subscription first");
        let (code, v) =
            m.subscription_edit(json!({"id": id, "use_config": true}).to_string().as_bytes());
        println!("use_config: {code} error={}", v["error"]);
        assert!(code < 300);
        let (_, st) = m.status();
        let st = &st["data"];
        println!(
            "running={} profile={} route={} tun={} last_error={}",
            st["running"], st["profile"], st["route"], st["tun"], st["last_error"]
        );
        assert_eq!(st["profile"]["id"], json!(id));
        let (_, subs) = m.subscriptions();
        for s in subs["data"]["subscriptions"].as_array().unwrap() {
            println!(
                "sub {}: use_config={} full={} groups={} nodes={} updated={} usage={}",
                s["name"],
                s["use_config"],
                s["full_config"],
                s["groups"],
                s["node_count"],
                s["updated_at"],
                s["usage"]
            );
        }
        let (_, g) = m.groups();
        let groups = g["data"]["groups"].as_array().cloned().unwrap_or_default();
        println!(
            "{} groups, {} nodes",
            groups.len(),
            g["data"]["nodes"].as_array().map_or(0, Vec::len)
        );
        for grp in &groups {
            println!(
                "  {} [{}] now={} members={}",
                grp["name"],
                grp["type"],
                grp["now"],
                grp["all"].as_array().map_or(0, Vec::len)
            );
        }
        if let Some(main) = groups.first().and_then(|g| g["name"].as_str()) {
            let (code, d) = m.delay(json!({"group": main}).to_string().as_bytes());
            let delays = d["data"]["delays"].as_object().cloned().unwrap_or_default();
            let ok_count = delays
                .values()
                .filter(|v| v.as_u64().unwrap_or(0) > 0)
                .count();
            println!(
                "delay {main}: {code} tested={} reachable={ok_count}",
                delays.len()
            );
        }
    }

    #[test]
    #[ignore = "runs against a real U60 Pro as root"]
    fn device_e2e() {
        let m = Manager::new();
        assert_eq!(ok("status", m.status())["installed"], true);

        let st = ok("start", m.service(br#"{"action":"start"}"#));
        assert_eq!(st["running"], true);
        assert!(st["version"].is_string());

        let st = ok("tun on", m.settings_set(br#"{"tun":true}"#));
        assert_eq!(st["tun_active"], true);
        assert!(service::firewall_present());

        let st = ok(
            "mode global (hot reload)",
            m.settings_set(br#"{"mode":"global"}"#),
        );
        assert_eq!(st["mode"], "global");
        assert_eq!(st["running"], true);

        assert_eq!(m.settings_set(br#"{"mixed_port":9090}"#).0, 400);
        assert_eq!(m.settings_set(br#"{"bogus":1}"#).0, 400);

        let g = ok("groups", m.groups());
        assert!(g["groups"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(m.pac().contains("PROXY "));

        if let Some(secs) = std::env::var("E2E_HOLD_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
        {
            println!("holding TUN for {secs}s");
            std::thread::sleep(Duration::from_secs(secs));
            ok("status after hold", m.status());
        }

        // Watchdog: a hard-killed mihomo is cleaned up and restarted.
        let pid = m.lock().process.as_ref().unwrap().pid;
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        std::thread::sleep(Duration::from_millis(500));
        m.lock().tick();
        let st = ok("after kill + tick", m.status());
        assert_eq!(st["running"], true);
        assert_ne!(st["pid"], json!(pid));
        assert!(service::firewall_present());

        // Watchdog: accepts flushed by a firewall reload are restored.
        service::firewall_remove();
        m.lock().firewall_checked = None;
        m.lock().tick();
        assert!(service::firewall_present());

        let st = ok("stop", m.service(br#"{"action":"stop"}"#));
        assert_eq!(st["running"], false);
        assert!(!service::tun_active());
        assert!(!service::firewall_present());

        ok(
            "restore defaults",
            m.settings_set(br#"{"tun":false,"mode":"rule"}"#),
        );
    }
}
