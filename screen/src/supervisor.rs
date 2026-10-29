//! Panel ownership: park the stock UI, run the panel as a child with a
//! heartbeat, and always hand the display back.
//!
//! The order and guards follow the community takeover that is verified on
//! B31 (docs/SCREEN.md): only after the firmware boot barrier and daemon sync,
//! only while the stock UI is running and not in power-off charging, stop it
//! through its init script (procd stops respawning it; nothing is disabled),
//! keep the backlight dark until the panel has committed a frame, and restart
//! the stock UI through the same init script on every exit path.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const RUNDIR: &str = "/tmp/zte-screen";
pub const BACKLIGHT: &str = "/sys/class/leds/led:lcd/brightness";
const STOCK_UI: &str = "/usr/bin/zte_topsw_devui";
const STOCK_INIT: &str = "/etc/init.d/zte_topsw_devui";
const BOOT_DONE: &str = "/tmp/zte_boot_done";
const MODE_STATE: &str = "/etc/config/zwrt_zte_mc_tmp";
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const LOG_CAP: u64 = 64 * 1024;

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn install_signals() {
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as libc::sighandler_t);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

fn path(name: &str) -> String {
    format!("{RUNDIR}/{name}")
}

pub fn log(msg: &str) {
    let p = path("screen.log");
    if fs::metadata(&p).is_ok_and(|m| m.len() > LOG_CAP) {
        if let Ok(data) = fs::read(&p) {
            let _ = fs::write(&p, &data[data.len() / 2..]);
        }
    }
    let (clock, date) = crate::stats::local_time();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&p)
    {
        let _ = writeln!(f, "{date} {clock} [{}] {msg}", std::process::id());
    }
}

fn put(name: &str, value: &str) {
    let tmp = path(&format!("{name}.new"));
    if fs::write(&tmp, value).is_ok() {
        let _ = fs::rename(&tmp, path(name));
    }
}

fn get(name: &str) -> Option<String> {
    fs::read_to_string(path(name))
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn read_backlight() -> Option<i32> {
    fs::read_to_string(BACKLIGHT).ok()?.trim().parse().ok()
}

pub fn write_backlight(v: i32) -> bool {
    fs::write(BACKLIGHT, v.clamp(0, 255).to_string()).is_ok()
        && read_backlight() == Some(v.clamp(0, 255))
}

/// Pids whose executable is exactly `exe`.
fn pids_of(exe: &str) -> Vec<i32> {
    let Ok(dir) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            *pid > 1
                && fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|p| p.as_os_str() == exe)
        })
        .collect()
}

pub fn stock_running() -> bool {
    !pids_of(STOCK_UI).is_empty()
}

fn run(args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    let out = process_runner::output(&mut cmd, None, timeout, 256 * 1024)
        .map_err(|e| format!("{}: {e}", args[0]))?;
    if !out.status.success() {
        return Err(format!("{} exited with {}", args.join(" "), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    done()
}

/// Why taking over now would be unsafe, if it would be.
pub fn preflight() -> Result<(), String> {
    if fs::metadata(BOOT_DONE).is_err() {
        return Err("firmware boot has not finished".into());
    }
    let sync = run(
        &[
            "ubus",
            "call",
            "zwrt_topsw_daemon.sync",
            "get_sync_info",
            "{}",
        ],
        Duration::from_secs(5),
    )?;
    if !sync.contains("sync success") || !sync.contains("register success") {
        return Err("stock daemons have not finished syncing".into());
    }
    if fs::read_to_string(MODE_STATE).is_ok_and(|s| {
        s.lines()
            .any(|l| l.contains("mode_main_state") && l.contains("mode_power_off"))
    }) {
        return Err("device is in power-off charging".into());
    }
    if !stock_running() {
        return Err("stock UI is not running".into());
    }
    if read_backlight().is_none() {
        return Err("backlight is not readable".into());
    }
    Ok(())
}

fn lock() -> std::io::Result<Option<File>> {
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(RUNDIR)?;
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path("lock"))?;
    // SAFETY: flock on a file descriptor we own.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    Ok((r == 0).then_some(f))
}

/// Restart the stock UI if we parked it, then put the brightness back.
pub fn restore() -> bool {
    put("state", "restoring");
    let parked = get("ui").as_deref() == Some("1");
    let mut ok = true;
    if parked && !stock_running() {
        if let Err(e) = run(&[STOCK_INIT, "start"], Duration::from_secs(10)) {
            log(&format!("stock start failed: {e}"));
            ok = false;
        }
        if !wait_for(Duration::from_secs(5), stock_running) {
            log("stock UI did not come back");
            ok = false;
        }
    }
    if let Some(level) = get("brightness").and_then(|b| b.parse::<i32>().ok()) {
        // The stock UI resumes its own sleep policy from here.
        write_backlight(level);
    }
    if ok {
        put("ui", "0");
    }
    put("state", if ok { "idle" } else { "error" });
    log(&format!("restore {}", if ok { "ok" } else { "FAILED" }));
    ok
}

fn kill_child(child: &mut Child) {
    // SAFETY: signalling our own child by pid.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Own the panel until the child exits, then restore. `limit` bounds the
/// session (used by on-device checks).
pub fn supervise(limit: Option<u64>) -> i32 {
    install_signals();
    let lock = match lock() {
        Ok(Some(l)) => l,
        Ok(None) => {
            eprintln!("another screen session is running");
            return 2;
        }
        Err(e) => {
            eprintln!("cannot create {RUNDIR}: {e}");
            return 1;
        }
    };
    put("owner", &std::process::id().to_string());
    if let Err(reason) = preflight() {
        log(&format!("not taking over: {reason}"));
        put("state", "refused");
        put("reason", &reason);
        let _ = fs::remove_file(path("owner"));
        drop(lock);
        eprintln!("{reason}");
        return 3;
    }
    let _ = fs::remove_file(path("reason"));
    let level = read_backlight().unwrap_or(128);
    put("brightness", &level.to_string());
    put("ui", "1");
    put("state", "starting");
    log(&format!("taking over (backlight {level})"));

    let rc = takeover(limit);
    let restored = restore();
    let _ = fs::remove_file(path("owner"));
    drop(lock);
    if restored {
        rc
    } else {
        1
    }
}

fn takeover(limit: Option<u64>) -> i32 {
    let t0 = Instant::now();
    if let Err(e) = run(&[STOCK_INIT, "stop"], Duration::from_secs(10)) {
        log(&format!("stock stop failed: {e}"));
        return 1;
    }
    let stopped_at = t0.elapsed();
    // procd has marked the instance stopped and sent SIGTERM, so it will not
    // respawn it. The stock UI is slow to honour SIGTERM; ZTE's own
    // startdui.sh uses kill -9 on it, so do the same after a short grace.
    // Only the exact binary is signalled.
    if !wait_for(Duration::from_millis(1200), || !stock_running()) {
        for pid in pids_of(STOCK_UI) {
            // SAFETY: signalling the stock UI process found by exe path.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
    if !wait_for(Duration::from_secs(2), || !stock_running()) || stop_requested() {
        log("stock UI did not stop");
        return 1;
    }
    log(&format!(
        "stock UI parked (init.d stop {} ms, total {} ms)",
        stopped_at.as_millis(),
        t0.elapsed().as_millis()
    ));
    // Dark until the panel has committed its first frame.
    write_backlight(0);

    let exe = std::env::current_exe().unwrap_or_else(|_| "/data/bin/zte-screen".into());
    let mut cmd = Command::new(exe);
    cmd.arg("--panel");
    if let Some(s) = limit {
        cmd.arg(s.to_string());
    }
    let errlog = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path("panel.log"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(match errlog {
            Ok(f) => Stdio::from(f),
            Err(_) => Stdio::null(),
        });
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log(&format!("panel spawn failed: {e}"));
            return 1;
        }
    };
    put("child", &child.id().to_string());
    let mut out = child.stdout.take().expect("piped stdout");
    // SAFETY: switching our end of the pipe to non-blocking.
    unsafe {
        let fd = out.as_raw_fd();
        libc::fcntl(
            fd,
            libc::F_SETFL,
            libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK,
        );
    }
    let mut beat = Instant::now();
    let mut ready = false;
    let status = loop {
        let mut buf = [0u8; 64];
        match out.read(&mut buf) {
            Ok(n) if n > 0 => {
                beat = Instant::now();
                if !ready && buf[..n].contains(&b'R') {
                    ready = true;
                    put("state", "active");
                    log("panel ready");
                }
            }
            _ => {}
        }
        if let Ok(Some(status)) = child.try_wait() {
            break Some(status);
        }
        if stop_requested() {
            log("stop requested");
            kill_child(&mut child);
            break None;
        }
        if beat.elapsed() > HEARTBEAT_TIMEOUT {
            log("panel heartbeat lost");
            kill_child(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = fs::remove_file(path("child"));
    match status {
        Some(s) if s.success() && ready => {
            log("panel exited normally");
            0
        }
        Some(s) => {
            log(&format!("panel exited: {s} (ready={ready})"));
            1
        }
        None => 1,
    }
}

fn pid_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks for existence.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

/// Ask a running session to end and wait for it to hand the panel back. With
/// no live owner, repair a session that died without restoring.
pub fn stop() -> i32 {
    if let Some(owner) = get("owner")
        .and_then(|p| p.parse::<i32>().ok())
        .filter(|p| pid_alive(*p))
    {
        // SAFETY: signalling the recorded supervisor.
        unsafe { libc::kill(owner, libc::SIGTERM) };
        if wait_for(Duration::from_secs(12), || !pid_alive(owner)) {
            return 0;
        }
        log("owner did not exit; forcing");
        if let Some(child) = get("child").and_then(|p| p.parse::<i32>().ok()) {
            // SAFETY: as above.
            unsafe { libc::kill(child, libc::SIGKILL) };
        }
        // SAFETY: as above.
        unsafe { libc::kill(owner, libc::SIGKILL) };
        wait_for(Duration::from_secs(2), || !pid_alive(owner));
    }
    match lock() {
        Ok(Some(_l)) => {
            if get("ui").as_deref() == Some("1") {
                log("repairing an orphaned session");
                return if restore() { 0 } else { 1 };
            }
            0
        }
        _ => 1,
    }
}

pub fn status() -> Value {
    let owner = get("owner")
        .and_then(|p| p.parse::<i32>().ok())
        .filter(|p| pid_alive(*p));
    let state = get("state").unwrap_or_else(|| "idle".into());
    json!({
        "state": if owner.is_none() && matches!(state.as_str(), "starting" | "active" | "restoring") { "stale" } else { state.as_str() },
        "owner": owner,
        "reason": get("reason"),
        "stock_ui": stock_running(),
        "parked": get("ui").as_deref() == Some("1"),
        "version": env!("CARGO_PKG_VERSION"),
    })
}
