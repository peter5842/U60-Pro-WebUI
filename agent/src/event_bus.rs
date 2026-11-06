use std::io::BufRead;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;

use serde_json::Value;

use crate::util::MutexExt;

const RESTART_DELAY_SECS: u64 = 5;
const IGNORED_EVENTS: &[&str] = &["zwrt_deviceui_event.touchstatus"];
/// Bounded so a subscriber that stops draining cannot grow its queue without
/// limit on a device that runs for months. Excess events are dropped.
const CHANNEL_CAPACITY: usize = 64;

struct Subscriber {
    event: String,
    tx: mpsc::SyncSender<Value>,
    dropped: AtomicU64,
}

pub struct EventBus {
    subscribers: Mutex<Vec<Subscriber>>,
}

impl EventBus {
    pub fn new() -> Self {
        EventBus {
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Register interest in a specific ubus event. Returns a receiver
    /// that will get the event payload each time it fires.
    pub fn subscribe(&self, event_name: &str) -> mpsc::Receiver<Value> {
        let (tx, rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let mut subs = self.subscribers.safe_lock();
        subs.push(Subscriber {
            event: event_name.to_string(),
            tx,
            dropped: AtomicU64::new(0),
        });
        rx
    }

    /// Spawn the listener thread. Consumes self into an Arc-compatible form.
    /// Call this after all subscriptions are registered: `ubus listen` is
    /// started with exactly the subscribed event names, so a later
    /// subscription would never fire.
    pub fn start(self) {
        let reaped = reap_orphaned_listeners(Path::new("/proc"));
        if reaped > 0 {
            eprintln!("[event_bus] stopped {reaped} orphaned ubus listen process(es)");
        }
        let events = self.event_names();
        if events.is_empty() {
            return;
        }
        std::thread::spawn(move || self.run_loop(&events));
    }

    /// Distinct subscribed event names, in subscription order.
    fn event_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for sub in self.subscribers.safe_lock().iter() {
            if !names.contains(&sub.event) {
                names.push(sub.event.clone());
            }
        }
        names
    }

    fn run_loop(&self, events: &[String]) {
        loop {
            match spawn_ubus_listen(events) {
                Ok(mut child) => {
                    eprintln!("[event_bus] ubus listen started (pid {})", child.id());
                    if let Some(stdout) = child.stdout.take() {
                        self.read_events(stdout);
                    }
                    // Process exited — reap it
                    let _ = child.wait();
                    eprintln!(
                        "[event_bus] ubus listen exited, restarting in {RESTART_DELAY_SECS}s"
                    );
                }
                Err(e) => {
                    eprintln!("[event_bus] failed to spawn ubus listen: {e}");
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(RESTART_DELAY_SECS));
        }
    }

    fn read_events(&self, stdout: std::process::ChildStdout) {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break, // pipe closed
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // ubus listen outputs: { "event_name": { ...payload... } }
            let parsed: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let obj = match parsed.as_object() {
                Some(o) => o,
                None => continue,
            };

            // Extract single top-level key as event name
            for (event_name, payload) in obj {
                if IGNORED_EVENTS.contains(&event_name.as_str()) {
                    continue;
                }

                self.dispatch(event_name, payload);
            }
        }
    }

    fn dispatch(&self, event_name: &str, payload: &Value) {
        let subs = self.subscribers.safe_lock();
        for sub in subs.iter() {
            if sub.event == event_name {
                // Non-blocking: drop the event if the receiver is backed up or
                // gone, so a stalled consumer can never block this listener
                // thread or grow its queue without bound.
                if sub.tx.try_send(payload.clone()).is_err()
                    && sub.dropped.fetch_add(1, Ordering::Relaxed) == 0
                {
                    eprintln!("[event_bus] {event_name}: subscriber not draining, dropping events");
                }
            }
        }
    }
}

/// Start `ubus listen` for just `events`.
///
/// Without names it would receive every event on the bus. `ubus listen`
/// ignores SIGPIPE (uloop), so an instance whose reader is gone never exits on
/// its own: before this, every agent restart left one behind, each still
/// subscribed to the whole bus.
///
/// The parent-death signal set here does NOT help on the U60: exec'ing `ubus`
/// moves the child from SELinux domain `sys.subj` to `ubus.subj`, and the
/// kernel clears the death signal on any SID transition (even in permissive
/// mode; confirmed 2026-11-06). It is kept for builds without that
/// transition. On the device, [`reap_orphaned_listeners`] at the next start is
/// what cleans up, so at most one orphan exists between a kill and a restart.
fn spawn_ubus_listen(events: &[String]) -> Result<Child, String> {
    let parent = std::process::id();
    let mut cmd = Command::new("ubus");
    cmd.arg("listen")
        .args(events)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: prctl and getppid are async-signal-safe and touch no Rust state.
    unsafe {
        cmd.pre_exec(move || {
            // The death signal follows the spawning thread; run_loop's thread
            // lives as long as the agent does.
            #[cfg(target_os = "linux")]
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The agent may have died between fork and prctl.
            if libc::getppid() as u32 != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
    cmd.spawn().map_err(|e| format!("spawn ubus listen: {e}"))
}

/// SIGTERM every `ubus listen` that was orphaned (parent is init) and whose
/// stdout pipe no other process holds, so nothing can ever read it. Returns
/// how many were signalled.
///
/// These are the listeners earlier agent instances left behind. A `ubus
/// listen` that some other program still reads, or one with a live parent,
/// is left alone.
fn reap_orphaned_listeners(proc_root: &Path) -> usize {
    let mut reaped = 0;
    for pid in orphaned_listeners(proc_root) {
        // SAFETY: plain syscall on a pid we just identified.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            reaped += 1;
        }
    }
    reaped
}

fn orphaned_listeners(proc_root: &Path) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    let pids: Vec<i32> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();

    let mut candidates: Vec<(i32, String)> = Vec::new();
    for &pid in &pids {
        let dir = proc_root.join(pid.to_string());
        if !is_ubus_listen(&dir) || parent_pid(&dir) != Some(1) {
            continue;
        }
        let Some(pipe) = std::fs::read_link(dir.join("fd/1"))
            .ok()
            .and_then(|t| t.to_str().map(str::to_owned))
            .filter(|t| t.starts_with("pipe:["))
        else {
            continue;
        };
        candidates.push((pid, pipe));
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    candidates
        .into_iter()
        .filter(|(pid, pipe)| {
            !pids
                .iter()
                .filter(|other| *other != pid)
                .any(|other| holds_fd(&proc_root.join(other.to_string()), pipe))
        })
        .map(|(pid, _)| pid)
        .collect()
}

fn is_ubus_listen(dir: &Path) -> bool {
    let Ok(cmdline) = std::fs::read(dir.join("cmdline")) else {
        return false;
    };
    let mut args = cmdline.split(|b| *b == 0);
    let program = args.next().unwrap_or_default();
    let base = program.rsplit(|b| *b == b'/').next().unwrap_or_default();
    base == b"ubus" && args.next() == Some(b"listen".as_slice())
}

/// Field 4 of /proc/<pid>/stat. The comm field may contain spaces, so parse
/// after its closing parenthesis.
fn parent_pid(dir: &Path) -> Option<i32> {
    let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

fn holds_fd(dir: &Path, target: &str) -> bool {
    let Ok(fds) = std::fs::read_dir(dir.join("fd")) else {
        return false;
    };
    fds.flatten().any(|fd| {
        std::fs::read_link(fd.path())
            .ok()
            .is_some_and(|t| t.as_os_str() == target)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    struct FakeProc(PathBuf);

    impl FakeProc {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("zte-agent-eventbus-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            FakeProc(root)
        }

        fn add(&self, pid: i32, ppid: i32, argv: &[&str], fds: &[&str]) {
            let dir = self.0.join(pid.to_string());
            std::fs::create_dir_all(dir.join("fd")).unwrap();
            let mut cmdline = argv.join("\0");
            cmdline.push('\0');
            std::fs::write(dir.join("cmdline"), cmdline).unwrap();
            std::fs::write(
                dir.join("stat"),
                format!("{pid} (a b) S {ppid} {pid} {pid} 0 -1"),
            )
            .unwrap();
            for (i, target) in fds.iter().enumerate() {
                symlink(target, dir.join("fd").join((i + 1).to_string())).unwrap();
            }
        }
    }

    impl Drop for FakeProc {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_only_orphans_nobody_reads() {
        let p = FakeProc::new("orphans");
        // Orphan whose pipe nobody else holds: reaped.
        p.add(100, 1, &["ubus", "listen"], &["pipe:[11]"]);
        // Live agent's own listener: parent is not init.
        p.add(
            200,
            50,
            &["ubus", "listen", "BSP_CHARGER_EVENT"],
            &["pipe:[22]"],
        );
        p.add(50, 1, &["/data/zte-agent"], &["pipe:[22]"]);
        // Orphan that another program still reads: kept.
        p.add(300, 1, &["/bin/ubus", "listen"], &["pipe:[33]"]);
        p.add(301, 1, &["/usr/bin/reader"], &["pipe:[33]"]);
        // Orphaned ubus, but not `listen`: kept.
        p.add(400, 1, &["ubus", "call", "x"], &["pipe:[44]"]);
        // Orphaned listener writing to a file, not a pipe: kept.
        p.add(500, 1, &["ubus", "listen"], &["/tmp/events.log"]);
        // Program named like ubus: kept.
        p.add(600, 1, &["ubusd", "listen"], &["pipe:[66]"]);

        assert_eq!(orphaned_listeners(&p.0), vec![100]);
    }

    #[test]
    fn missing_proc_root_finds_nothing() {
        assert!(orphaned_listeners(Path::new("/nonexistent-proc-root")).is_empty());
    }

    #[test]
    fn parent_pid_survives_spaces_and_parens_in_comm() {
        let p = FakeProc::new("stat");
        let dir = p.0.join("7");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stat"), "7 (odd) name) S 42 7 7 0").unwrap();
        assert_eq!(parent_pid(&dir), Some(42));
    }

    #[test]
    fn event_names_are_distinct_in_order() {
        let bus = EventBus::new();
        let _a = bus.subscribe("B");
        let _b = bus.subscribe("A");
        let _c = bus.subscribe("B");
        assert_eq!(bus.event_names(), vec!["B".to_string(), "A".to_string()]);
    }
}
