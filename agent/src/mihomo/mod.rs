//! mihomo proxy manager: subscriptions, node selection, service control and
//! the optional LAN TUN, behind `/api/proxy/*`.
//!
//! State lives in /data/mihomo/manager.json (0600: it holds subscription URLs
//! and the controller secret). Every change follows render → `mihomo -t` →
//! atomic replace → hot reload, and is rolled back when the reload fails.

mod config;
mod controller;
mod service;

use std::collections::{HashMap, VecDeque};
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
}

fn bad(msg: impl Into<String>) -> (u16, Value) {
    (400, json!({"ok": false, "error": msg.into()}))
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
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.safe_lock()
    }

    /// Background supervisor: (re)starts mihomo when it should run, keeps the
    /// TUN firewall accepts in place across fw3/QCMAP reloads, caps the log.
    pub fn start_watchdog(self: &Arc<Self>) {
        let me = Arc::clone(self);
        std::thread::spawn(move || {
            // Give the LAN bridge and the WAN time to come up after boot.
            std::thread::sleep(Duration::from_secs(10));
            let mut ticks: u64 = 0;
            loop {
                me.lock().tick();
                ticks += 1;
                if ticks % 12 == 0 {
                    service::truncate_log_if_large();
                }
                std::thread::sleep(TICK);
            }
        });
    }
}

impl Inner {
    fn secret(&self) -> String {
        self.state.secret.clone()
    }

    fn alive(&mut self) -> bool {
        self.process.as_mut().is_some_and(Process::alive)
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
        let rendered =
            serde_json::to_vec_pretty(&config::render(next, &ip)).map_err(|e| e.to_string())?;
        atomic_write(Path::new(service::STAGED), &rendered)
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
            Duration::from_secs(15),
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
        let mut selected = Value::Null;
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
            if let Ok(g) = ctl.get("/proxies/PROXY") {
                let now = g["now"].as_str().unwrap_or("").to_string();
                let auto = if now == "AUTO" {
                    ctl.get("/proxies/AUTO")
                        .ok()
                        .and_then(|a| a["now"].as_str().map(str::to_string))
                } else {
                    None
                };
                selected = json!({"group_choice": now, "auto_choice": auto});
            }
        }
        let process = self.process.as_ref();
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
            "tun": s.tun,
            "tun_active": service::tun_active(),
            "mixed_port": s.mixed_port,
            "lan_ip": ip,
            "proxy_address": ip.as_ref().map(|ip| format!("{ip}:{}", s.mixed_port)),
            "pac_url": ip.as_ref().map(|ip| format!("http://{ip}:9090/proxy.pac")),
            "subscriptions": self.state.subscriptions.len(),
            "traffic": traffic,
            "selected": selected,
            "restarts": self.restarts,
            "last_error": self.last_error,
            "notice": self.notice,
        })
    }

    fn find_sub(&self, id: &str) -> Option<usize> {
        self.state.subscriptions.iter().position(|s| s.id == id)
    }
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
            Err(e) => (409, json!({"ok": false, "error": e})),
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
                (409, json!({"ok": false, "error": e}))
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
        let list: Vec<Value> = inner
            .state
            .subscriptions
            .iter()
            .map(|s| {
                let p = providers.as_ref().and_then(|p| p.get(s.provider()));
                let nodes = p.and_then(|p| p["proxies"].as_array()).map(|a| {
                    a.iter()
                        .filter(|x| x["type"].as_str() != Some("Compatible"))
                        .count()
                });
                let updated = p
                    .and_then(|p| p["updatedAt"].as_str())
                    .filter(|t| !t.starts_with("0001-"))
                    .map(str::to_string);
                let info = p
                    .map(|p| &p["subscriptionInfo"])
                    .filter(|i| i.is_object())
                    .map(|i| {
                        json!({
                            "upload": i["Upload"].as_u64(),
                            "download": i["Download"].as_u64(),
                            "total": i["Total"].as_u64(),
                            "expire": i["Expire"].as_u64(),
                        })
                    });
                json!({
                    "id": s.id,
                    "name": s.name,
                    "url_masked": config::mask_url(&s.url),
                    "enabled": s.enabled,
                    "interval_hours": s.interval_hours,
                    "node_count": nodes,
                    "updated_at": updated,
                    "usage": info,
                    "error": inner.sub_errors.get(&s.id),
                })
            })
            .collect();
        (
            200,
            json!({"ok": true, "data": {"subscriptions": list, "running": providers.is_some()}}),
        )
    }

    /// POST /api/proxy/subscriptions — {name, url, interval_hours?}
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
        let mut inner = self.lock();
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
        let mut next = inner.state.clone();
        next.subscriptions.push(Subscription {
            id: id.clone(),
            name,
            url,
            enabled: true,
            interval_hours: interval,
        });
        match inner.apply(next) {
            Ok(()) => (200, json!({"ok": true, "data": {"id": id}})),
            Err(e) => (409, json!({"ok": false, "error": e})),
        }
    }

    /// PUT /api/proxy/subscriptions — {id, name?, url?, enabled?, interval_hours?}
    pub fn subscription_edit(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let id = v["id"].as_str().unwrap_or("");
        let mut inner = self.lock();
        let Some(idx) = inner.find_sub(id) else {
            return (404, json!({"ok": false, "error": "subscription not found"}));
        };
        let mut next = inner.state.clone();
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
        let url_changed = next.subscriptions[idx].url != inner.state.subscriptions[idx].url;
        match inner.apply(next) {
            Ok(()) => {
                if url_changed {
                    let _ = fs::remove_file(format!("{PROVIDERS_DIR}/sub-{id}.yaml"));
                    inner.sub_errors.remove(id);
                }
                (200, json!({"ok": true, "data": {}}))
            }
            Err(e) => (409, json!({"ok": false, "error": e})),
        }
    }

    /// POST /api/proxy/subscriptions/delete — {id}  (X-Confirm)
    pub fn subscription_delete(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let id = v["id"].as_str().unwrap_or("").to_string();
        let mut inner = self.lock();
        let Some(idx) = inner.find_sub(&id) else {
            return (404, json!({"ok": false, "error": "subscription not found"}));
        };
        let mut next = inner.state.clone();
        next.subscriptions.remove(idx);
        match inner.apply(next) {
            Ok(()) => {
                let _ = fs::remove_file(format!("{PROVIDERS_DIR}/sub-{id}.yaml"));
                inner.sub_errors.remove(&id);
                (200, json!({"ok": true, "data": {}}))
            }
            Err(e) => (409, json!({"ok": false, "error": e})),
        }
    }

    /// POST /api/proxy/subscriptions/update — {id?}: fetch now (one or all).
    pub fn subscription_refresh(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let (secret, targets) = {
            let mut inner = self.lock();
            if !inner.alive() {
                return (
                    409,
                    json!({"ok": false, "error": "start the proxy before updating subscriptions"}),
                );
            }
            let targets: Vec<String> = match v["id"].as_str() {
                Some(id) => match inner.find_sub(id) {
                    Some(i) if inner.state.subscriptions[i].enabled => vec![id.to_string()],
                    Some(_) => return bad("subscription is disabled"),
                    None => return (404, json!({"ok": false, "error": "subscription not found"})),
                },
                None => inner
                    .state
                    .subscriptions
                    .iter()
                    .filter(|s| s.enabled)
                    .map(|s| s.id.clone())
                    .collect(),
            };
            (inner.secret(), targets)
        };
        // Fetching can take a while; do it without holding the manager lock.
        let ctl = Controller { secret: &secret };
        let mut results = serde_json::Map::new();
        let mut errors = HashMap::new();
        for id in &targets {
            let path = format!("/providers/proxies/{}", segment(&config::provider_name(id)));
            let outcome = ctl
                .request("PUT", &path, None, Duration::from_secs(30))
                .and_then(|r| {
                    if r.ok() {
                        Ok(())
                    } else {
                        Err(r.error_message())
                    }
                });
            match outcome {
                Ok(()) => {
                    results.insert(id.clone(), json!({"ok": true}));
                }
                Err(e) => {
                    results.insert(id.clone(), json!({"ok": false, "error": e}));
                    errors.insert(id.clone(), e);
                }
            }
        }
        let mut inner = self.lock();
        for id in &targets {
            match errors.remove(id) {
                Some(e) => inner.sub_errors.insert(id.clone(), e),
                None => inner.sub_errors.remove(id),
            };
        }
        (200, json!({"ok": true, "data": {"results": results}}))
    }

    /// GET /api/proxy/groups — selectable groups plus every node with its last delay.
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
        let mut groups = Vec::new();
        for name in ["PROXY", "AUTO"] {
            if let Ok(g) = ctl.get(&format!("/proxies/{name}")) {
                groups.push(json!({
                    "name": name,
                    "type": g["type"],
                    "now": g["now"],
                    "all": g["all"],
                }));
            }
        }
        let mut nodes = Vec::new();
        if let Ok(p) = ctl.get("/providers/proxies") {
            if let Some(map) = p["providers"].as_object() {
                for (provider, body) in map {
                    let Some((sub_id, sub_name)) = names.get(provider) else {
                        continue;
                    };
                    for node in body["proxies"].as_array().into_iter().flatten() {
                        if node["type"].as_str() == Some("Compatible") {
                            continue;
                        }
                        let last = node["history"].as_array().and_then(|h| h.last());
                        nodes.push(json!({
                            "name": node["name"],
                            "type": node["type"],
                            "udp": node["udp"],
                            "alive": node["alive"],
                            "delay": last.and_then(|h| h["delay"].as_u64()),
                            "subscription_id": sub_id,
                            "subscription": sub_name,
                        }));
                    }
                }
            }
        }
        (
            200,
            json!({"ok": true, "data": {"running": true, "groups": groups, "nodes": nodes}}),
        )
    }

    /// PUT /api/proxy/groups — {group: "PROXY", proxy}
    pub fn group_select(&self, body: &[u8]) -> (u16, Value) {
        let v = match parse(body) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if v["group"].as_str() != Some("PROXY") {
            return bad("only the PROXY group can be selected manually");
        }
        let Some(proxy) = v["proxy"].as_str().filter(|p| !p.is_empty()) else {
            return bad("proxy is required");
        };
        let secret = {
            let mut inner = self.lock();
            if !inner.alive() {
                return (
                    409,
                    json!({"ok": false, "error": "the proxy is not running"}),
                );
            }
            inner.secret()
        };
        let ctl = Controller { secret: &secret };
        match ctl.request(
            "PUT",
            "/proxies/PROXY",
            Some(&json!({"name": proxy})),
            Duration::from_secs(5),
        ) {
            Ok(r) if r.ok() => (200, json!({"ok": true, "data": {"now": proxy}})),
            Ok(r) => bad(r.error_message()),
            Err(e) => (503, json!({"ok": false, "error": e})),
        }
    }

    /// POST /api/proxy/delay — test every node (via AUTO); returns name → ms (0 = timeout).
    pub fn delay(&self, _body: &[u8]) -> (u16, Value) {
        let (secret, has_subs) = {
            let mut inner = self.lock();
            if !inner.alive() {
                return (
                    409,
                    json!({"ok": false, "error": "the proxy is not running"}),
                );
            }
            (
                inner.secret(),
                inner.state.subscriptions.iter().any(|s| s.enabled),
            )
        };
        if !has_subs {
            return (200, json!({"ok": true, "data": {"delays": {}}}));
        }
        let url = config::HEALTH_URL.replace(':', "%3A").replace('/', "%2F");
        let path = format!("/group/AUTO/delay?url={url}&timeout=5000");
        match (Controller { secret: &secret }).request("GET", &path, None, Duration::from_secs(20))
        {
            Ok(r) if r.ok() => (
                200,
                json!({"ok": true, "data": {"delays": r.json().unwrap_or(Value::Null)}}),
            ),
            Ok(r) => (503, json!({"ok": false, "error": r.error_message()})),
            Err(e) => (503, json!({"ok": false, "error": e})),
        }
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
        assert_eq!(g["groups"][0]["name"], "PROXY");
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
