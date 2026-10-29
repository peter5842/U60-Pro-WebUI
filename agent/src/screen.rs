//! The geek screen: open and close a `zte-screen` session on the LCD, its
//! settings, and the device-side ways in.
//!
//! The session itself (parking the stock UI, owning the display, restoring)
//! lives in the separate `zte-screen` binary so a display fault can never take
//! the API down. This module only starts it, stops it, and watches for:
//! - a two-finger hold on the lit stock UI (opens a session),
//! - `auto_open`: `boot` opens once per boot after the firmware boot barrier;
//!   `wake` also opens whenever the stock UI lights a dark screen,
//! - a session that died without restoring the stock UI (repairs it),
//! - requests from the session's PROXY page for the few mihomo changes the
//!   agent owns (`ScreenAsk`).
//!
//! See docs/SCREEN.md for the safety model.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::handlers::AppState;
use crate::storage::atomic_write;

const BINARY: &str = "/data/bin/zte-screen";
const SETTINGS: &str = "/data/local/tmp/zte-screen.json";
const RUNDIR: &str = "/tmp/zte-screen";
const BOOT_MARK: &str = "/tmp/zte-screen-boot";
const BOOT_DONE: &str = "/tmp/zte_boot_done";
const STOCK_UI: &str = "/usr/bin/zte_topsw_devui";
const BACKLIGHT: &str = "/sys/class/leds/led:lcd/brightness";
/// Open-at-boot gives up if the firmware is not ready this long after power-on.
const BOOT_WINDOW_SECS: u64 = 600;
const HOLD_MS: i64 = 2000;
const COOLDOWN_MS: i64 = 5000;
/// The screen must have been dark this long before a wake counts.
const DARK_MS: i64 = 1500;
/// Pause automatic opens after a session that was refused or failed.
const BACKOFF_REFUSED_MS: i64 = 60_000;
const BACKOFF_ERROR_MS: i64 = 600_000;
/// At most this many automatic opens per hour.
const AUTO_PER_HOUR: usize = 30;
const TICK_MS: i32 = 400;

static WATCHING: AtomicBool = AtomicBool::new(false);

const PAGES: [&str; 7] = ["home", "sys", "pwr", "cell", "net", "lan", "proxy"];

/// Defaults and limits match `screen/src/settings.rs`.
fn normalize(v: &Value) -> Value {
    let u = |k: &str| v.get(k).and_then(Value::as_u64);
    let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
    let one_of = |k: &str, allowed: &[&str], d: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .filter(|s| allowed.contains(s))
            .unwrap_or(d)
            .to_string()
    };
    json!({
        "brightness": u("brightness").map(|x| x.clamp(20, 255)).unwrap_or(160),
        "idle_secs": u("idle_secs").map(|x| x.clamp(30, 3600)).unwrap_or(120),
        "idle_action": one_of("idle_action", &["stock", "sleep"], "stock"),
        "theme": one_of("theme", &["green", "amber", "cyan"], "green"),
        "scanlines": b("scanlines", true),
        "start_page": one_of("start_page", &PAGES, "home"),
        "gesture": b("gesture", true),
        "auto_open": one_of("auto_open", &["off", "boot", "wake"], "off"),
    })
}

fn read_settings() -> Value {
    let raw = fs::read_to_string(SETTINGS)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!({}));
    normalize(&raw)
}

/// Merge a partial update; reject unknown keys and wrong types rather than
/// silently dropping them.
fn merge_settings(current: &Value, update: &Value) -> Result<Value, String> {
    let Value::Object(update) = update else {
        return Err("expected a JSON object".into());
    };
    let mut merged: Map<String, Value> = current.as_object().cloned().unwrap_or_default();
    for (key, value) in update {
        let ok = match key.as_str() {
            "brightness" | "idle_secs" => value.is_u64(),
            "idle_action" | "theme" | "start_page" | "auto_open" => value.is_string(),
            "scanlines" | "gesture" => value.is_boolean(),
            _ => return Err(format!("unknown setting '{key}'")),
        };
        if !ok {
            return Err(format!("invalid value for '{key}'"));
        }
        merged.insert(key.clone(), value.clone());
    }
    let normalized = normalize(&Value::Object(merged.clone()));
    for (key, value) in &merged {
        if normalized.get(key) != Some(value) && update.contains_key(key) {
            return Err(format!("'{key}' is out of range"));
        }
    }
    Ok(normalized)
}

fn installed() -> bool {
    fs::metadata(BINARY).is_ok_and(|m| m.is_file())
}

fn run_screen(arg: &str, timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(BINARY);
    cmd.arg(arg);
    let out = process_runner::output(&mut cmd, None, timeout, 64 * 1024)
        .map_err(|e| format!("zte-screen {arg}: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn session() -> Value {
    if !installed() {
        return Value::Null;
    }
    run_screen("--status", Duration::from_secs(5))
        .ok()
        .and_then(|s| serde_json::from_str(s.trim()).ok())
        .unwrap_or(Value::Null)
}

fn snapshot() -> Value {
    json!({
        "installed": installed(),
        "session": session(),
        "settings": read_settings(),
        "gesture_listener": WATCHING.load(Ordering::SeqCst),
    })
}

/// GET /api/screen
pub fn status(_state: &AppState) -> (u16, Value) {
    (200, json!({"ok": true, "data": snapshot()}))
}

/// Start `zte-screen --supervise` in its own session, so an agent restart
/// cannot take the display owner with it.
fn spawn_session() -> Result<(), String> {
    let mut cmd = Command::new(BINARY);
    cmd.arg("--supervise")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start zte-screen: {e}"))?;
    // The supervisor runs for the whole session; reap it in the background.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// POST /api/screen — open a session and wait (briefly) for it to come up.
pub fn open(_state: &AppState) -> (u16, Value) {
    if !installed() {
        return (
            404,
            json!({"ok": false, "error": format!("zte-screen is not installed ({BINARY})")}),
        );
    }
    if let Err(e) = spawn_session() {
        return (500, json!({"ok": false, "error": e}));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(400));
        let s = session();
        match s.get("state").and_then(Value::as_str) {
            Some("active") => return (200, json!({"ok": true, "data": snapshot()})),
            Some("refused") => {
                let reason = s.get("reason").and_then(Value::as_str).unwrap_or("refused");
                return (409, json!({"ok": false, "error": reason}));
            }
            Some("error") => {
                return (
                    500,
                    json!({"ok": false, "error": "the screen did not start; the stock UI was restored"}),
                )
            }
            _ => {}
        }
    }
    (202, json!({"ok": true, "data": snapshot()}))
}

/// DELETE /api/screen — end the session and give the panel back to the stock UI.
pub fn close(_state: &AppState) -> (u16, Value) {
    if !installed() {
        return (200, json!({"ok": true, "data": snapshot()}));
    }
    match run_screen("--stop", Duration::from_secs(25)) {
        Ok(_) => (200, json!({"ok": true, "data": snapshot()})),
        Err(e) => (500, json!({"ok": false, "error": e})),
    }
}

/// PUT /api/screen/settings — a running screen picks the change up within ~1 s.
pub fn settings_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let update: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let merged = match merge_settings(&read_settings(), &update) {
        Ok(v) => v,
        Err(e) => return (400, json!({"ok": false, "error": e})),
    };
    let bytes = serde_json::to_vec_pretty(&merged).unwrap_or_default();
    if let Err(e) = atomic_write(Path::new(SETTINGS), &bytes) {
        return (
            500,
            json!({"ok": false, "error": format!("cannot save settings: {e}")}),
        );
    }
    (200, json!({"ok": true, "data": snapshot()}))
}

// -- Watcher --

/// Two-finger hold detector over multi-touch protocol B. Fires once when
/// exactly two fingers stay down and still for `HOLD_MS`; then every finger
/// must lift (and the cooldown pass) before it can fire again.
pub struct HoldDetector {
    slot: usize,
    ids: [i32; 10],
    pos: [(i32, i32); 10],
    start: Option<(i64, [(i32, i32); 2])>,
    /// Movement allowed per finger, in raw digitizer units.
    slop: i32,
    blocked: bool,
    last_fire: i64,
}

impl HoldDetector {
    pub fn new(slop: i32) -> Self {
        HoldDetector {
            slot: 0,
            ids: [-1; 10],
            pos: [(0, 0); 10],
            start: None,
            slop,
            blocked: false,
            last_fire: i64::MIN / 2,
        }
    }

    fn down(&self) -> Vec<usize> {
        (0..10).filter(|i| self.ids[*i] >= 0).collect()
    }

    /// Feed one event; `lit` says whether the stock UI's screen is on.
    pub fn event(&mut self, kind: u16, code: u16, value: i32, ms: i64, lit: bool) {
        const EV_SYN: u16 = 0;
        const EV_ABS: u16 = 3;
        match (kind, code) {
            (EV_SYN, 3) => {
                // SYN_DROPPED: state is unknown until everything lifts.
                self.start = None;
                self.blocked = true;
                self.ids = [-1; 10];
            }
            (EV_ABS, 0x2f) => self.slot = (value.max(0) as usize).min(9),
            (EV_ABS, 0x39) => self.ids[self.slot] = value,
            (EV_ABS, 0x35) => self.pos[self.slot].0 = value,
            (EV_ABS, 0x36) => self.pos[self.slot].1 = value,
            (EV_SYN, 0) => self.report(ms, lit),
            _ => {}
        }
    }

    fn report(&mut self, ms: i64, lit: bool) {
        let down = self.down();
        if down.is_empty() {
            self.blocked = false;
            self.start = None;
            return;
        }
        if self.blocked || !lit || down.len() != 2 {
            // A third finger or a dark screen spoils this hold.
            if down.len() > 2 || !lit {
                self.blocked = true;
            }
            self.start = None;
            return;
        }
        let now = [self.pos[down[0]], self.pos[down[1]]];
        match self.start {
            None => self.start = Some((ms, now)),
            Some((_, at)) => {
                let moved = at
                    .iter()
                    .zip(now.iter())
                    .any(|(a, b)| (a.0 - b.0).abs() > self.slop || (a.1 - b.1).abs() > self.slop);
                if moved {
                    self.blocked = true;
                    self.start = None;
                }
            }
        }
    }

    /// Call periodically (and after events) with the event clock.
    pub fn poll(&mut self, ms: i64) -> bool {
        let Some((t0, _)) = self.start else {
            return false;
        };
        if ms - t0 >= HOLD_MS && ms - self.last_fire >= COOLDOWN_MS {
            self.last_fire = ms;
            self.start = None;
            self.blocked = true;
            return true;
        }
        false
    }
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn stock_running() -> bool {
    let Ok(dir) = fs::read_dir("/proc") else {
        return false;
    };
    dir.flatten().any(|e| {
        e.file_name()
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            && fs::read_link(e.path().join("exe")).is_ok_and(|p| p.as_os_str() == STOCK_UI)
    })
}

fn pid_alive(path: &str) -> bool {
    read_trim(path)
        .and_then(|p| p.parse::<i32>().ok())
        // SAFETY: signal 0 only checks for existence.
        .is_some_and(|pid| pid > 0 && unsafe { libc::kill(pid, 0) } == 0)
}

fn session_running() -> bool {
    pid_alive(&format!("{RUNDIR}/owner"))
}

fn screen_lit() -> bool {
    read_trim(BACKLIGHT)
        .and_then(|b| b.parse::<i32>().ok())
        .is_some_and(|b| b > 0)
}

fn uptime_secs() -> u64 {
    read_trim("/proc/uptime")
        .and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok())
        .map(|s| s as u64)
        .unwrap_or(0)
}

fn monotonic_ms() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes the timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000
}

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// Linux `struct input_event` on 64-bit targets.
#[repr(C)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    kind: u16,
    code: u16,
    value: i32,
}

const fn eviocg(nr: u64, size: usize) -> u64 {
    (2 << 30) | ((size as u64) << 16) | ((b'E' as u64) << 8) | nr
}

/// Open the touchscreen read-only (not grabbed: the stock UI keeps receiving
/// every touch) with monotonic event stamps; returns it with a slop of about
/// a twelfth of the screen width in raw units.
fn open_touch() -> Option<(File, i32)> {
    for i in 0..16 {
        let Ok(f) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(format!("/dev/input/event{i}"))
        else {
            continue;
        };
        let fd = f.as_raw_fd();
        let mut name = [0u8; 64];
        // SAFETY: EVIOCGNAME writes at most `len` bytes.
        unsafe { libc::ioctl(fd, eviocg(0x06, name.len()) as _, name.as_mut_ptr()) };
        if !String::from_utf8_lossy(&name).contains("sitronix") {
            continue;
        }
        let mut x = AbsInfo::default();
        // SAFETY: EVIOCGABS(ABS_MT_POSITION_X) fills one input_absinfo.
        unsafe {
            libc::ioctl(
                fd,
                eviocg(0x40 + 0x35, std::mem::size_of::<AbsInfo>()) as _,
                &mut x,
            )
        };
        let clock = libc::CLOCK_MONOTONIC;
        let set_clock = (1u64 << 30) | (4 << 16) | ((b'E' as u64) << 8) | 0xa0;
        // SAFETY: EVIOCSCLOCKID reads one int.
        unsafe { libc::ioctl(fd, set_clock as _, &clock) };
        return Some((f, ((x.maximum - x.minimum) / 12).max(20)));
    }
    None
}

fn read_events(f: &mut File, out: &mut Vec<(u16, u16, i32, i64)>) -> bool {
    let size = std::mem::size_of::<InputEvent>();
    let mut buf = vec![0u8; size * 64];
    loop {
        match f.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => {
                for c in buf[..n].chunks_exact(size) {
                    // SAFETY: the kernel writes whole input_event records.
                    let e: InputEvent = unsafe { std::ptr::read_unaligned(c.as_ptr() as *const _) };
                    out.push((e.kind, e.code, e.value, e.tv_sec * 1000 + e.tv_usec / 1000));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
}

fn boot_ready() -> bool {
    Path::new(BOOT_DONE).exists() && stock_running()
}

/// Start the watcher thread: gesture, automatic opens, orphan repair and
/// the PROXY page's requests.
pub fn start(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("screen-watch".into())
        .spawn(move || watch(&state))
        .ok();
}

/// What the screen's PROXY page may ask of the agent: the parts of mihomo
/// the agent owns. Node choices and delay tests go to mihomo directly;
/// nothing else is accepted.
#[derive(Clone, Debug, PartialEq)]
pub enum ScreenAsk {
    Mode(&'static str),
    Restart,
    /// One subscription by id, or every enabled one.
    Update(Option<String>),
    /// The subscription list, without URLs.
    Subs,
}

/// `{"id": n, "op": "mode"|"restart"|"update"|"subs", …}`.
pub fn parse_ask(v: &Value) -> Result<(u64, ScreenAsk), String> {
    let id = v["id"].as_u64().ok_or("id is required")?;
    let ask = match v["op"].as_str() {
        Some("mode") => ScreenAsk::Mode(
            ["rule", "global", "direct"]
                .into_iter()
                .find(|m| v["mode"].as_str() == Some(*m))
                .ok_or("mode must be rule, global or direct")?,
        ),
        Some("restart") => ScreenAsk::Restart,
        Some("update") => match v.get("sub") {
            None => ScreenAsk::Update(None),
            Some(Value::String(id)) if !id.is_empty() && id.len() <= 64 => {
                ScreenAsk::Update(Some(id.clone()))
            }
            Some(_) => return Err("sub must be a subscription id".into()),
        },
        Some("subs") => ScreenAsk::Subs,
        _ => return Err("unknown op".into()),
    };
    Ok((id, ask))
}

fn answer(state: &AppState, ask: &ScreenAsk) -> (u16, Value) {
    let m = &state.mihomo;
    match ask {
        ScreenAsk::Mode(mode) => m.settings_set(json!({ "mode": mode }).to_string().as_bytes()),
        ScreenAsk::Restart => m.service(br#"{"action":"restart"}"#),
        ScreenAsk::Update(sub) => {
            let body = sub.as_ref().map_or(json!({}), |id| json!({ "id": id }));
            m.subscription_refresh(body.to_string().as_bytes())
        }
        ScreenAsk::Subs => {
            let (code, mut v) = m.subscriptions();
            // The screen shows names and counts; it never needs the URLs.
            for s in v["data"]["subscriptions"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                if let Some(o) = s.as_object_mut() {
                    o.remove("url_masked");
                }
            }
            (code, v)
        }
    }
}

static ASKING: AtomicBool = AtomicBool::new(false);

/// Take one request from the session directory and answer it on a worker
/// thread, so a slow subscription download never stalls the watcher.
fn serve(state: &Arc<AppState>) {
    let path = format!("{RUNDIR}/agent.request");
    if ASKING.load(Ordering::SeqCst) {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return;
    };
    // A request nobody is waiting for any more (the screen gives up, or an
    // older agent never read it) must not run late.
    let fresh = meta
        .modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(20));
    let body = (meta.is_file() && meta.len() <= 512 && fresh)
        .then(|| fs::read(&path).ok())
        .flatten();
    let _ = fs::remove_file(&path);
    let Some(v) = body.and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
        return;
    };
    let (id, ask) = match parse_ask(&v) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[screen] refused a PROXY request: {e}");
            reply(
                v["id"].as_u64().unwrap_or(0),
                &(400, json!({"ok": false, "error": e})),
            );
            return;
        }
    };
    if ask != ScreenAsk::Subs {
        eprintln!("[screen] PROXY page asks: {ask:?}");
    }
    ASKING.store(true, Ordering::SeqCst);
    let state = Arc::clone(state);
    let spawned = std::thread::Builder::new()
        .name("screen-ask".into())
        .spawn(move || {
            let r = answer(&state, &ask);
            if r.1["ok"] != true {
                eprintln!("[screen] {ask:?} failed: {}", r.1["error"]);
            }
            reply(id, &r);
            ASKING.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        ASKING.store(false, Ordering::SeqCst);
    }
}

fn reply(id: u64, (_, v): &(u16, Value)) {
    let body = json!({"id": id, "ok": v["ok"], "data": v["data"], "error": v["error"]});
    let path = format!("{RUNDIR}/agent.reply");
    if let Err(e) = atomic_write(Path::new(&path), body.to_string().as_bytes()) {
        eprintln!("[screen] could not answer the PROXY page: {e}");
    }
}

/// When to open a session without being asked. Pure, so it can be tested.
#[derive(Default)]
pub struct AutoOpener {
    dark_since: Option<i64>,
    /// A dark screen has been seen since the last session ended.
    armed: bool,
    boot_ready_since: Option<i64>,
    boot_done: bool,
    backoff_until: i64,
    last_state: String,
    opens: VecDeque<i64>,
}

/// What the watcher sees on one tick.
pub struct Observed<'a> {
    pub mode: &'a str,
    pub now: i64,
    pub lit: bool,
    pub stock_up: bool,
    pub boot_barrier: bool,
    pub uptime_s: u64,
    pub session: bool,
    pub state: &'a str,
}

impl AutoOpener {
    pub fn tick(&mut self, o: &Observed) -> bool {
        if o.state != self.last_state {
            match o.state {
                "refused" => self.backoff_until = o.now + BACKOFF_REFUSED_MS,
                "error" => self.backoff_until = o.now + BACKOFF_ERROR_MS,
                _ => {}
            }
            self.last_state = o.state.to_string();
        }
        if o.session {
            // Re-arm only after the screen goes dark once the session is over,
            // so leaving with :q never bounces straight back.
            self.armed = false;
            self.dark_since = None;
            return false;
        }
        if o.mode == "off" {
            self.armed = false;
            return false;
        }
        let ready = o.stock_up && o.boot_barrier;
        while self.opens.front().is_some_and(|t| o.now - t > 3_600_000) {
            self.opens.pop_front();
        }
        let allowed = ready && o.now >= self.backoff_until && self.opens.len() < AUTO_PER_HOUR;

        // Once per boot (both modes): a few seconds after the stock UI is up.
        if !self.boot_done && o.uptime_s < BOOT_WINDOW_SECS {
            if ready {
                let since = *self.boot_ready_since.get_or_insert(o.now);
                if o.now - since >= 5000 && allowed {
                    self.boot_done = true;
                    self.opens.push_back(o.now);
                    return true;
                }
            } else {
                self.boot_ready_since = None;
            }
        }
        if o.mode != "wake" {
            return false;
        }
        if !o.lit {
            let since = *self.dark_since.get_or_insert(o.now);
            if o.now - since >= DARK_MS && o.stock_up {
                self.armed = true;
            }
            return false;
        }
        self.dark_since = None;
        if self.armed && allowed {
            self.armed = false;
            self.opens.push_back(o.now);
            return true;
        }
        false
    }

    /// The boot open already happened in an earlier agent run this boot.
    pub fn mark_boot_done(&mut self) {
        self.boot_done = true;
    }
}

fn watch(state: &Arc<AppState>) {
    let mut touch: Option<(File, i32)> = None;
    let mut detector = HoldDetector::new(40);
    let mut auto = AutoOpener::default();
    if Path::new(BOOT_MARK).exists() {
        auto.mark_boot_done();
    }
    let mut events = Vec::new();
    let mut next_slow = 0i64;
    let mut orphan_seen = 0u32;
    // Scanning /proc for the stock UI is too slow for every tick.
    let mut stock_up = false;
    let mut settings = read_settings();
    loop {
        let now = monotonic_ms();
        if now >= next_slow {
            next_slow = now + 5000;
            stock_up = stock_running();
            settings = read_settings();
            let wanted = installed() && settings["gesture"].as_bool() == Some(true);
            match (wanted, touch.is_some()) {
                (true, false) => {
                    touch = open_touch();
                    if let Some((_, slop)) = &touch {
                        detector = HoldDetector::new(*slop);
                        eprintln!("[screen] two-finger hold listener on");
                    }
                }
                (false, true) => {
                    touch = None;
                    eprintln!("[screen] two-finger hold listener off");
                }
                _ => {}
            }
            WATCHING.store(touch.is_some(), Ordering::SeqCst);

            // A session whose owner died without handing the panel back.
            let parked = read_trim(&format!("{RUNDIR}/ui")).as_deref() == Some("1");
            if installed() && parked && !session_running() && !stock_up {
                orphan_seen += 1;
                if orphan_seen >= 2 {
                    eprintln!(
                        "[screen] repairing a session that ended without restoring the stock UI"
                    );
                    let _ = run_screen("--stop", Duration::from_secs(25));
                    orphan_seen = 0;
                }
            } else {
                orphan_seen = 0;
            }
        }

        let running = session_running();
        if running {
            serve(state);
        }
        if installed() {
            let state = read_trim(&format!("{RUNDIR}/state")).unwrap_or_default();
            let observed = Observed {
                mode: settings["auto_open"].as_str().unwrap_or("off"),
                now,
                lit: screen_lit(),
                stock_up,
                boot_barrier: Path::new(BOOT_DONE).exists(),
                uptime_s: uptime_secs(),
                session: running,
                state: &state,
            };
            if auto.tick(&observed) {
                let _ = fs::write(BOOT_MARK, "");
                eprintln!("[screen] auto-open ({})", observed.mode);
                if let Err(e) = spawn_session() {
                    eprintln!("[screen] {e}");
                }
            }
        }

        let Some((file, _)) = touch.as_mut() else {
            std::thread::sleep(Duration::from_millis(TICK_MS as u64));
            continue;
        };
        let mut fds = [libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        // SAFETY: polling one fd we own.
        unsafe { libc::poll(fds.as_mut_ptr(), 1, TICK_MS) };
        events.clear();
        if !read_events(file, &mut events) {
            touch = None;
            continue;
        }
        // While a session runs the panel owns the touchscreen; just drain.
        if running {
            detector = HoldDetector::new(detector.slop);
            continue;
        }
        let lit = stock_up && screen_lit();
        for (kind, code, value, ms) in events.drain(..) {
            detector.event(kind, code, value, ms, lit);
        }
        if detector.poll(monotonic_ms()) {
            trigger();
        }
    }
}

fn trigger() {
    if session_running() || !boot_ready() || !screen_lit() {
        return;
    }
    eprintln!("[screen] two-finger hold: opening");
    if let Err(e) = spawn_session() {
        eprintln!("[screen] {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proxy_page_may_ask_only_for_what_the_agent_owns() {
        let ok = |v: Value| parse_ask(&v).map(|(_, a)| a);
        assert_eq!(
            ok(json!({"id": 1, "op": "mode", "mode": "global"})),
            Ok(ScreenAsk::Mode("global"))
        );
        assert!(ok(json!({"id": 1, "op": "mode", "mode": "tun"})).is_err());
        assert_eq!(
            ok(json!({"id": 2, "op": "restart"})),
            Ok(ScreenAsk::Restart)
        );
        assert_eq!(
            ok(json!({"id": 3, "op": "update"})),
            Ok(ScreenAsk::Update(None))
        );
        assert_eq!(
            ok(json!({"id": 3, "op": "update", "sub": "abc"})),
            Ok(ScreenAsk::Update(Some("abc".into())))
        );
        assert!(ok(json!({"id": 3, "op": "update", "sub": 5})).is_err());
        assert!(
            ok(json!({"id": 4, "op": "stop"})).is_err(),
            "no stopping the proxy"
        );
        assert!(ok(json!({"id": 5, "op": "delete", "sub": "abc"})).is_err());
        assert!(ok(json!({"op": "subs"})).is_err(), "an id is required");
    }

    const SLOT: u16 = 0x2f;
    const ID: u16 = 0x39;
    const X: u16 = 0x35;

    fn finger(d: &mut HoldDetector, slot: i32, id: i32, x: i32, ms: i64, lit: bool) {
        d.event(3, SLOT, slot, ms, lit);
        d.event(3, ID, id, ms, lit);
        d.event(3, X, x, ms, lit);
        d.event(0, 0, 0, ms, lit);
    }

    #[test]
    fn two_still_fingers_fire_once() {
        let mut d = HoldDetector::new(40);
        finger(&mut d, 0, 1, 100, 0, true);
        finger(&mut d, 1, 2, 300, 10, true);
        assert!(!d.poll(1500));
        assert!(d.poll(2100));
        assert!(!d.poll(4000), "fires once per hold");
        // Lift both, hold again inside the cooldown: no.
        d.event(3, SLOT, 0, 4100, true);
        d.event(3, ID, -1, 4100, true);
        d.event(3, SLOT, 1, 4100, true);
        d.event(3, ID, -1, 4100, true);
        d.event(0, 0, 0, 4100, true);
        finger(&mut d, 0, 3, 100, 4200, true);
        finger(&mut d, 1, 4, 300, 4210, true);
        assert!(!d.poll(6300));
        assert!(d.poll(7200));
    }

    #[test]
    fn one_finger_three_fingers_drift_or_dark_never_fire() {
        let mut d = HoldDetector::new(40);
        finger(&mut d, 0, 1, 100, 0, true);
        assert!(!d.poll(3000), "one finger is a stock long press");

        let mut d = HoldDetector::new(40);
        finger(&mut d, 0, 1, 100, 0, true);
        finger(&mut d, 1, 2, 300, 10, true);
        finger(&mut d, 2, 3, 500, 20, true);
        assert!(!d.poll(3000));

        let mut d = HoldDetector::new(40);
        finger(&mut d, 0, 1, 100, 0, true);
        finger(&mut d, 1, 2, 300, 10, true);
        d.event(3, SLOT, 0, 500, true);
        d.event(3, X, 200, 500, true);
        d.event(0, 0, 0, 500, true);
        assert!(!d.poll(3000));

        let mut d = HoldDetector::new(40);
        finger(&mut d, 0, 1, 100, 0, false);
        finger(&mut d, 1, 2, 300, 10, false);
        assert!(!d.poll(3000));
    }

    #[test]
    fn dropped_events_block_until_lift() {
        let mut d = HoldDetector::new(40);
        finger(&mut d, 0, 1, 100, 0, true);
        finger(&mut d, 1, 2, 300, 10, true);
        d.event(0, 3, 0, 20, true);
        finger(&mut d, 0, 1, 100, 30, true);
        finger(&mut d, 1, 2, 300, 40, true);
        assert!(!d.poll(3000));
    }

    fn obs<'a>(mode: &'a str, now: i64, lit: bool, session: bool, state: &'a str) -> Observed<'a> {
        Observed {
            mode,
            now,
            lit,
            stock_up: true,
            boot_barrier: true,
            uptime_s: 3600,
            session,
            state,
        }
    }

    #[test]
    fn wake_opens_on_dark_to_lit_only() {
        let mut a = AutoOpener::default();
        a.mark_boot_done();
        // Lit from the start: no edge yet.
        assert!(!a.tick(&obs("wake", 0, true, false, "idle")));
        assert!(!a.tick(&obs("wake", 400, false, false, "idle")));
        assert!(
            !a.tick(&obs("wake", 1000, false, false, "idle")),
            "dark too briefly"
        );
        assert!(!a.tick(&obs("wake", 2000, false, false, "idle")));
        assert!(a.tick(&obs("wake", 2400, true, false, "idle")));
        // The session runs, then :q hands back a lit stock UI: no bounce.
        assert!(!a.tick(&obs("wake", 3000, true, true, "active")));
        assert!(!a.tick(&obs("wake", 9000, true, false, "idle")));
        // The stock UI sleeps and wakes again: open again.
        a.tick(&obs("wake", 10_000, false, false, "idle"));
        a.tick(&obs("wake", 12_000, false, false, "idle"));
        assert!(a.tick(&obs("wake", 12_400, true, false, "idle")));
    }

    #[test]
    fn failures_back_off_and_rate_is_capped() {
        let mut a = AutoOpener::default();
        a.mark_boot_done();
        a.tick(&obs("wake", 0, true, false, "error"));
        a.tick(&obs("wake", 400, false, false, "error"));
        a.tick(&obs("wake", 2000, false, false, "error"));
        assert!(
            !a.tick(&obs("wake", 2400, true, false, "error")),
            "backing off after an error"
        );
        let mut a = AutoOpener::default();
        a.mark_boot_done();
        let mut t = 0;
        let mut opened = 0;
        for _ in 0..40 {
            a.tick(&obs("wake", t, false, false, "idle"));
            a.tick(&obs("wake", t + 2000, false, false, "idle"));
            if a.tick(&obs("wake", t + 2400, true, false, "idle")) {
                opened += 1;
            }
            t += 10_000;
        }
        assert_eq!(opened, AUTO_PER_HOUR);
    }

    #[test]
    fn boot_mode_opens_once_after_the_stock_ui_settles() {
        let mut a = AutoOpener::default();
        let early = |now| Observed {
            uptime_s: 60,
            ..obs("boot", now, true, false, "idle")
        };
        assert!(!a.tick(&early(0)));
        assert!(!a.tick(&early(4000)));
        assert!(a.tick(&early(5200)));
        assert!(!a.tick(&early(20_000)));
        // Off never opens.
        let mut b = AutoOpener::default();
        assert!(!b.tick(&Observed {
            uptime_s: 60,
            ..obs("off", 9000, true, false, "idle")
        }));
    }

    #[test]
    fn settings_merge_validates() {
        let current = normalize(&json!({}));
        assert_eq!(current["idle_secs"], 120);
        assert_eq!(current["gesture"], true);
        assert_eq!(current["auto_open"], "off");
        assert_eq!(
            merge_settings(&current, &json!({"auto_open": "wake"})).unwrap()["auto_open"],
            "wake"
        );
        assert!(merge_settings(&current, &json!({"auto_open": "always"})).is_err());
        let merged =
            merge_settings(&current, &json!({"theme": "amber", "brightness": 90})).unwrap();
        assert_eq!(merged["theme"], "amber");
        assert_eq!(merged["brightness"], 90);
        assert!(merge_settings(&current, &json!({"theme": "pink"})).is_err());
        assert!(merge_settings(&current, &json!({"brightness": 5})).is_err());
        assert!(merge_settings(&current, &json!({"volume": 3})).is_err());
        assert!(merge_settings(&current, &json!({"gesture": "yes"})).is_err());
        assert!(merge_settings(&current, &json!([1])).is_err());
    }
}
