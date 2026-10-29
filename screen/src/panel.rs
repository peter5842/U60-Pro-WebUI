//! The child process that owns the display while a session runs. It writes
//! `H` to stdout every second as a heartbeat and `R` once the display has
//! settled; the supervisor kills it and restores the stock UI if either stops.

use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::canvas::Canvas;
use crate::drm::Display;
use crate::input::dev::{now_ms, open_power_key, open_touch, read_events};
use crate::input::{Gesture, PowerKey, Press, Touch};
use crate::settings::{IdleAction, Settings, PATH as SETTINGS_PATH};
use crate::stats::{Collector, Model, Needs};
use crate::supervisor::{self, write_backlight, RUNDIR};
use crate::ui::{Action, Page, Ui, SPLASH_FRAMES};

const IDLE_WARNING_MS: i64 = 10_000;
const NEED_NET: u8 = 1;
const NEED_PROXY: u8 = 2;
const NEED_LAN: u8 = 4;

fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

struct Shared {
    model: Mutex<Model>,
    needs: AtomicU8,
    awake: AtomicBool,
    quit: AtomicBool,
}

fn collector(shared: Arc<Shared>) {
    let mut c = Collector::new();
    // Collect into a private copy so drawing never waits on a slow ubus call.
    let mut local = Model::default();
    while !shared.quit.load(Ordering::SeqCst) {
        let started = now_ms();
        if shared.awake.load(Ordering::SeqCst) {
            let needs = shared.needs.load(Ordering::SeqCst);
            c.tick(
                &mut local,
                Needs {
                    net: needs & NEED_NET != 0,
                    lan: needs & NEED_LAN != 0,
                    proxy: needs & NEED_PROXY != 0,
                },
            );
            *shared.model.lock().unwrap_or_else(|e| e.into_inner()) = local.clone();
        }
        let spent = now_ms() - started;
        std::thread::sleep(Duration::from_millis((1000 - spent).clamp(100, 1000) as u64));
    }
}

fn beat(out: &mut std::io::Stdout, b: u8) -> bool {
    out.write_all(&[b]).and_then(|_| out.flush()).is_ok()
}

pub fn run(limit_secs: Option<u64>) -> i32 {
    supervisor::install_signals();
    match session(limit_secs) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("panel: {e}");
            supervisor::log(&format!("panel error: {e}"));
            1
        }
    }
}

fn session(limit_secs: Option<u64>) -> std::io::Result<()> {
    let (mut touch_file, ax, ay, touch_path) = open_touch()?;
    let (mut key_file, key_path) = open_power_key()?;
    let mut display = Display::open()?;
    supervisor::log(&format!(
        "panel: touch {touch_path}, key {key_path}, {}",
        display.log.join("; ")
    ));

    let settings_path = Path::new(SETTINGS_PATH);
    let mut settings = Settings::load(settings_path);
    let mut settings_mtime = mtime(settings_path);
    let mut ui = Ui::new(settings.clone());
    let shared = Arc::new(Shared {
        model: Mutex::new(Model::default()),
        needs: AtomicU8::new(0),
        awake: AtomicBool::new(true),
        quit: AtomicBool::new(false),
    });
    {
        let mut m = shared.model.lock().unwrap_or_else(|e| e.into_inner());
        m.kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
            .to_string();
    }
    let worker = {
        let s = Arc::clone(&shared);
        std::thread::spawn(move || collector(s))
    };

    let mut stdout = std::io::stdout();
    let mut canvas = Canvas::new();
    let mut touch = Touch::new(ax, ay);
    let mut key = PowerKey::default();
    let mut events = Vec::new();

    let start = now_ms();
    let mut last_activity = start;
    let mut next_beat = 0i64;
    let mut next_second = start + 1000;
    let mut next_splash = start + 280;
    let mut settle: Vec<i64> = Vec::new();
    let mut ready = false;
    let mut awake = true;
    let mut need_draw = true;
    let mut first = true;
    let mut exit = false;
    let mut next_frame = 0i64;
    let mut was_awake = true;

    let result = loop {
        let now = now_ms();
        if supervisor::stop_requested() || exit {
            break Ok(());
        }
        if limit_secs.is_some_and(|s| now - start >= s as i64 * 1000) {
            break Ok(());
        }
        // SAFETY: getppid has no preconditions.
        if unsafe { libc::getppid() } == 1 {
            // The supervisor died: put the stock UI back ourselves.
            drop(display);
            shared.quit.store(true, Ordering::SeqCst);
            let _ = worker.join();
            supervisor::restore();
            return Ok(());
        }

        // Splash, once-a-second refresh, blink, idle and external settings.
        if ui.splash < SPLASH_FRAMES && now >= next_splash {
            ui.splash += 1;
            next_splash = now + 280;
            need_draw = true;
        }
        if now >= next_second {
            next_second = now + 1000;
            ui.blink = !ui.blink;
            need_draw = awake;
            if mtime(settings_path) != settings_mtime {
                settings_mtime = mtime(settings_path);
                let fresh = Settings::load(settings_path);
                if fresh != settings {
                    if awake && fresh.brightness != settings.brightness {
                        write_backlight(fresh.brightness as i32);
                    }
                    settings = fresh.clone();
                    ui.settings = fresh;
                }
            }
            // Test hook: `echo talk > /tmp/zte-screen/pet.request`.
            let req = format!("{RUNDIR}/pet.request");
            if let Ok(action) = fs::read_to_string(&req) {
                let _ = fs::remove_file(&req);
                let m = shared.model.lock().unwrap_or_else(|e| e.into_inner());
                ui.pet.request(&action, &m);
            }
            // Test hook: `echo "260 450" > /tmp/zte-screen/tap.request`
            // taps there (one "x y" per line), as a finger would.
            let req = format!("{RUNDIR}/tap.request");
            if let Ok(taps) = fs::read_to_string(&req) {
                let _ = fs::remove_file(&req);
                let mut m = shared.model.lock().unwrap_or_else(|e| e.into_inner());
                for line in taps.lines().take(8) {
                    let mut it = line.split_whitespace().map(|v| v.parse::<i32>());
                    if let (Some(Ok(x)), Some(Ok(y))) = (it.next(), it.next()) {
                        if !matches!(ui.tap(x, y, &mut m), Action::Exit) {
                            last_activity = now;
                        }
                    }
                }
            }
            ui.proxy.tick(ui.page == Page::Proxy);
            if Path::new(&format!("{RUNDIR}/snapshot.request")).exists() {
                let _ = fs::write(format!("{RUNDIR}/frame.png"), crate::png::encode(&canvas));
                let _ = fs::remove_file(format!("{RUNDIR}/snapshot.request"));
            }
        }
        let idle_ms = settings.idle_secs as i64 * 1000;
        let left = idle_ms - (now - last_activity);
        if awake {
            let warn = (left <= IDLE_WARNING_MS).then(|| ((left + 999) / 1000).max(0) as u32);
            if warn != ui.idle_left {
                ui.idle_left = warn;
                need_draw = true;
            }
            if left <= 0 {
                match settings.idle_action {
                    IdleAction::Stock => break Ok(()),
                    // Dark until a short power press; only `:q` or a long
                    // press hands the panel back.
                    IdleAction::Sleep => {
                        write_backlight(0);
                        awake = false;
                        ui.idle_left = None;
                    }
                }
            }
        }
        shared.awake.store(awake, Ordering::SeqCst);
        {
            let m = shared.model.lock().unwrap_or_else(|e| e.into_inner());
            if awake != was_awake {
                was_awake = awake;
                if awake {
                    ui.pet.on_wake();
                } else {
                    ui.pet.on_dark();
                }
            }
            if awake {
                ui.pet.tick(crate::pet::clock(), &m);
            }
        }
        if awake && ready && ui.page == Page::Home && !ui.settings_open && now >= next_frame {
            // HOME animates the pet (8-15 fps); other pages redraw once a second.
            next_frame = now + ui.pet.frame_ms();
            need_draw = true;
        }
        shared.needs.store(
            match ui.page {
                Page::Net | Page::Home => NEED_NET,
                Page::Lan => NEED_LAN,
                Page::Proxy => NEED_PROXY,
                _ => 0,
            },
            Ordering::SeqCst,
        );

        if need_draw && awake {
            {
                let m = shared.model.lock().unwrap_or_else(|e| e.into_inner());
                ui.render(&mut canvas, &m);
            }
            display.present(&canvas)?;
            need_draw = false;
            if first {
                // A QPIC modeset can reset the backlight: light it only after a
                // real frame, then make two bounded follow-up commits.
                first = false;
                if !write_backlight(settings.brightness as i32) {
                    return Err(std::io::Error::other("backlight did not accept the level"));
                }
                settle = vec![now + 150, now + 350];
            }
        }
        if let Some(at) = settle.first().copied() {
            if now >= at {
                settle.remove(0);
                display.present(&canvas)?;
                if settle.is_empty() && !ready {
                    ready = true;
                    if !beat(&mut stdout, b'R') {
                        break Ok(());
                    }
                }
            }
        }
        if now >= next_beat {
            next_beat = now + 1000;
            if !beat(&mut stdout, b'H') {
                break Ok(());
            }
        }

        let mut fds = [
            libc::pollfd {
                fd: touch_file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: key_file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: polling two fds we own.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, 40) };
        if r < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break Err(std::io::Error::last_os_error());
        }
        if fds
            .iter()
            .any(|f| f.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0)
        {
            break Err(std::io::Error::other("input device lost"));
        }

        events.clear();
        read_events(&mut key_file, &mut events)?;
        let mut presses: Vec<Press> = events
            .iter()
            .filter_map(|(t, c, v, ms)| key.event(*t, *c, *v, *ms))
            .collect();
        presses.extend(key.poll(now_ms()));
        for press in presses {
            match press {
                Press::Long => {
                    exit = true;
                }
                Press::Short if ready => {
                    last_activity = now_ms();
                    if awake {
                        write_backlight(0);
                        awake = false;
                    } else {
                        write_backlight(settings.brightness as i32);
                        awake = true;
                        need_draw = true;
                    }
                    // Touches queued across the transition are dropped below.
                    touch = Touch::new(ax, ay);
                }
                Press::Short => {}
            }
        }

        events.clear();
        read_events(&mut touch_file, &mut events)?;
        for (t, c, v, ms) in events.drain(..) {
            let gesture = touch.event(t, c, v, ms);
            if !awake || !ready {
                continue;
            }
            last_activity = now_ms();
            let action = {
                let mut m = shared.model.lock().unwrap_or_else(|e| e.into_inner());
                match gesture {
                    Some(Gesture::Tap(x, y)) => ui.tap(x, y, &mut m),
                    Some(Gesture::Swipe(dx)) => ui.swipe(dx),
                    None => Action::None,
                }
            };
            match action {
                Action::None => {}
                Action::Redraw => need_draw = true,
                Action::Exit => exit = true,
                Action::SettingsChanged => {
                    if ui.settings.brightness != settings.brightness {
                        write_backlight(ui.settings.brightness as i32);
                    }
                    settings = ui.settings.clone();
                    if settings.save(settings_path).is_ok() {
                        settings_mtime = mtime(settings_path);
                    }
                    need_draw = true;
                }
            }
        }
        if touch.active() && awake {
            last_activity = now_ms();
        }
    };

    // Sign-off frame, so the hand-back reads like a session ending.
    if awake && ready {
        let p = ui.palette();
        canvas.clear(p.bg);
        canvas.text(
            8,
            200,
            crate::font::Size::Small,
            p.fg,
            "root@u60pro:~# logout",
        );
        canvas.text(
            8,
            220,
            crate::font::Size::Small,
            p.dim,
            "Connection to u60pro closed.",
        );
        let _ = display.present(&canvas);
        std::thread::sleep(Duration::from_millis(350));
    }
    ui.pet.save();
    shared.quit.store(true, Ordering::SeqCst);
    let _ = worker.join();
    drop(display);
    result
}
