//! Touch and power-key input. The decoders are plain state machines over
//! evdev events so they can be tested on the host; only `open_*` touches
//! device nodes. Neither device is grabbed: the firmware's own long-press
//! power handling keeps working.

use crate::canvas::{H, W};

pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;
pub const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const BTN_TOUCH: u16 = 0x14a;
pub const KEY_POWER: u16 = 116;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

/// Movement allowed for a tap, in screen pixels.
const TAP_SLOP: i32 = 14;
const SWIPE_MIN: i32 = 60;
const TAP_MAX_MS: i64 = 900;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Tap(i32, i32),
    Swipe(i32),
}

#[derive(Clone, Copy)]
pub struct Axis {
    pub min: i32,
    pub max: i32,
}

impl Axis {
    fn scale(self, v: i32, size: i32) -> i32 {
        let span = (self.max - self.min).max(1) as i64;
        ((v - self.min) as i64 * (size - 1) as i64 / span).clamp(0, (size - 1) as i64) as i32
    }
}

/// Multi-touch protocol B decoder. Only slot 0 drives gestures; a second
/// finger turns the current contact into a no-op.
pub struct Touch {
    ax: Axis,
    ay: Axis,
    slot: i32,
    ids: [i32; 2],
    raw: (i32, i32),
    have: (bool, bool),
    down: Option<(i32, i32, i64)>,
    last: (i32, i32),
    multi: bool,
    dropped: bool,
}

impl Touch {
    pub fn new(ax: Axis, ay: Axis) -> Self {
        Touch {
            ax,
            ay,
            slot: 0,
            ids: [-1, -1],
            raw: (0, 0),
            have: (false, false),
            down: None,
            last: (0, 0),
            multi: false,
            dropped: false,
        }
    }

    /// Raw axes to upright screen coordinates. The panel and the digitizer
    /// are both mounted upside down relative to the case.
    fn point(&self) -> (i32, i32) {
        let x = self.ax.scale(self.raw.0, W);
        let y = self.ay.scale(self.raw.1, H);
        (W - 1 - x, H - 1 - y)
    }

    /// Feed one event (timestamp in ms). Returns a gesture on finger lift.
    pub fn event(&mut self, kind: u16, code: u16, value: i32, ms: i64) -> Option<Gesture> {
        if kind == EV_SYN && code == SYN_DROPPED {
            self.dropped = true;
            self.down = None;
            self.ids = [-1, -1];
            return None;
        }
        if self.dropped {
            if kind == EV_SYN && code == SYN_REPORT {
                self.dropped = false;
                self.have = (false, false);
            }
            return None;
        }
        match (kind, code) {
            (EV_ABS, ABS_MT_SLOT) => self.slot = value,
            (EV_ABS, ABS_MT_TRACKING_ID) if (0..2).contains(&self.slot) => {
                self.ids[self.slot as usize] = value;
                if value >= 0 && self.slot == 0 {
                    self.have = (false, false);
                }
            }
            (EV_ABS, ABS_MT_POSITION_X) if self.slot == 0 => {
                self.raw.0 = value;
                self.have.0 = true;
            }
            (EV_ABS, ABS_MT_POSITION_Y) if self.slot == 0 => {
                self.raw.1 = value;
                self.have.1 = true;
            }
            (EV_KEY, BTN_TOUCH) if value == 0 => self.ids = [-1, -1],
            _ => {}
        }
        if kind != EV_SYN || code != SYN_REPORT {
            return None;
        }
        if self.ids[1] >= 0 {
            self.multi = true;
        }
        let touching = self.ids[0] >= 0 && self.have.0 && self.have.1;
        if touching {
            let p = self.point();
            if self.down.is_none() {
                self.down = Some((p.0, p.1, ms));
                self.multi = self.ids[1] >= 0;
            }
            self.last = p;
            return None;
        }
        let (sx, sy, t0) = self.down.take()?;
        if self.multi {
            self.multi = false;
            return None;
        }
        let (dx, dy) = (self.last.0 - sx, self.last.1 - sy);
        if dx.abs() >= SWIPE_MIN && dx.abs() > dy.abs() * 2 {
            return Some(Gesture::Swipe(dx));
        }
        if dx.abs() <= TAP_SLOP && dy.abs() <= TAP_SLOP && ms - t0 <= TAP_MAX_MS {
            return Some(Gesture::Tap(sx, sy));
        }
        None
    }

    pub fn active(&self) -> bool {
        self.down.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Short,
    Long,
}

/// Power key: a short press (30-800 ms) on release, and a long hold reported
/// once while the key is still down.
#[derive(Default)]
pub struct PowerKey {
    pressed: Option<i64>,
    fired: bool,
    dropped: bool,
}

pub const HOLD_MS: i64 = 1500;

impl PowerKey {
    pub fn event(&mut self, kind: u16, code: u16, value: i32, ms: i64) -> Option<Press> {
        if kind == EV_SYN && code == SYN_DROPPED {
            self.pressed = None;
            self.dropped = true;
            return None;
        }
        if self.dropped {
            if kind == EV_SYN && code == SYN_REPORT {
                self.dropped = false;
            }
            return None;
        }
        if kind != EV_KEY || code != KEY_POWER {
            return None;
        }
        match value {
            1 if self.pressed.is_none() => {
                self.pressed = Some(ms);
                self.fired = false;
                None
            }
            0 => {
                let t0 = self.pressed.take()?;
                let held = ms - t0;
                if self.fired {
                    None
                } else if held >= HOLD_MS {
                    Some(Press::Long)
                } else if (30..800).contains(&held) {
                    Some(Press::Short)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Call periodically with the same clock as the event stamps.
    pub fn poll(&mut self, now: i64) -> Option<Press> {
        let t0 = self.pressed?;
        if !self.fired && now - t0 >= HOLD_MS {
            self.fired = true;
            return Some(Press::Long);
        }
        None
    }
}

#[cfg(target_os = "linux")]
pub mod dev {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    use super::{Axis, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, KEY_POWER};

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

    const fn ior(nr: u64, size: usize) -> u64 {
        (2 << 30) | ((size as u64) << 16) | ((b'E' as u64) << 8) | nr
    }
    const fn iow(nr: u64, size: usize) -> u64 {
        (1 << 30) | ((size as u64) << 16) | ((b'E' as u64) << 8) | nr
    }

    fn open(path: &str) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
    }

    fn name(f: &File) -> String {
        let mut buf = [0u8; 128];
        // SAFETY: EVIOCGNAME(len) writes at most `len` bytes into `buf`.
        let n = unsafe { libc::ioctl(f.as_raw_fd(), ior(0x06, buf.len()) as _, buf.as_mut_ptr()) };
        if n <= 0 {
            return String::new();
        }
        String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(n as usize)])
            .into_owned()
    }

    fn abs(f: &File, code: u16) -> Option<Axis> {
        let mut a = AbsInfo::default();
        // SAFETY: EVIOCGABS(code) fills one input_absinfo.
        let r = unsafe {
            libc::ioctl(
                f.as_raw_fd(),
                ior(0x40 + code as u64, std::mem::size_of::<AbsInfo>()) as _,
                &mut a as *mut AbsInfo,
            )
        };
        (r == 0 && a.maximum > a.minimum).then_some(Axis {
            min: a.minimum,
            max: a.maximum,
        })
    }

    /// Ask the kernel to stamp events with CLOCK_MONOTONIC so hold times can be
    /// compared with `now_ms()`.
    fn monotonic(f: &File) {
        let clock: libc::c_int = libc::CLOCK_MONOTONIC;
        // SAFETY: EVIOCSCLOCKID reads one int.
        unsafe { libc::ioctl(f.as_raw_fd(), iow(0xa0, 4) as _, &clock) };
    }

    fn has_key(f: &File, key: u16) -> bool {
        let mut bits = [0u8; 96];
        // SAFETY: EVIOCGBIT(EV_KEY, len) writes at most `len` bytes.
        let r = unsafe {
            libc::ioctl(
                f.as_raw_fd(),
                ior(0x20 + 1, bits.len()) as _,
                bits.as_mut_ptr(),
            )
        };
        r > 0 && bits[key as usize / 8] & (1 << (key % 8)) != 0
    }

    pub fn now_ms() -> i64 {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime writes the timespec.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000
    }

    /// The Sitronix touchscreen, with its axis ranges.
    pub fn open_touch() -> io::Result<(File, Axis, Axis, String)> {
        for i in 0..16 {
            let path = format!("/dev/input/event{i}");
            let Ok(f) = open(&path) else { continue };
            let n = name(&f);
            if !n.contains("sitronix") {
                continue;
            }
            if let (Some(x), Some(y)) = (abs(&f, ABS_MT_POSITION_X), abs(&f, ABS_MT_POSITION_Y)) {
                monotonic(&f);
                return Ok((f, x, y, path));
            }
        }
        Err(io::Error::other("touchscreen not found"))
    }

    /// The device that reports KEY_POWER and is not the touchscreen.
    pub fn open_power_key() -> io::Result<(File, String)> {
        for i in 0..16 {
            let path = format!("/dev/input/event{i}");
            let Ok(f) = open(&path) else { continue };
            if name(&f).contains("sitronix") || !has_key(&f, KEY_POWER) {
                continue;
            }
            monotonic(&f);
            return Ok((f, path));
        }
        Err(io::Error::other("power key not found"))
    }

    /// Drain pending events: (type, code, value, ms).
    pub fn read_events(f: &mut File, out: &mut Vec<(u16, u16, i32, i64)>) -> io::Result<()> {
        let size = std::mem::size_of::<libc::input_event>();
        let mut buf = vec![0u8; size * 64];
        loop {
            match f.read(&mut buf) {
                Ok(0) => return Err(io::Error::other("input device closed")),
                Ok(n) => {
                    for chunk in buf[..n].chunks_exact(size) {
                        // SAFETY: the kernel writes whole input_event records.
                        let ev: libc::input_event =
                            unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const _) };
                        let ms = ev.time.tv_sec * 1000 + ev.time.tv_usec / 1000;
                        out.push((ev.type_, ev.code, ev.value, ms));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis() -> (Axis, Axis) {
        (Axis { min: 0, max: 319 }, Axis { min: 0, max: 479 })
    }

    /// Raw contact at upright screen point (x, y).
    fn contact(t: &mut Touch, x: i32, y: i32, ms: i64, first: bool) -> Option<Gesture> {
        if first {
            t.event(EV_ABS, ABS_MT_SLOT, 0, ms);
            t.event(EV_ABS, ABS_MT_TRACKING_ID, 7, ms);
        }
        t.event(EV_ABS, ABS_MT_POSITION_X, W - 1 - x, ms);
        t.event(EV_ABS, ABS_MT_POSITION_Y, H - 1 - y, ms);
        t.event(EV_SYN, SYN_REPORT, 0, ms)
    }

    fn lift(t: &mut Touch, ms: i64) -> Option<Gesture> {
        t.event(EV_ABS, ABS_MT_TRACKING_ID, -1, ms);
        t.event(EV_SYN, SYN_REPORT, 0, ms)
    }

    #[test]
    fn tap_is_rotated_upright() {
        let (ax, ay) = axis();
        let mut t = Touch::new(ax, ay);
        assert_eq!(contact(&mut t, 10, 20, 0, true), None);
        assert_eq!(lift(&mut t, 100), Some(Gesture::Tap(10, 20)));
    }

    #[test]
    fn horizontal_drag_is_a_swipe() {
        let (ax, ay) = axis();
        let mut t = Touch::new(ax, ay);
        contact(&mut t, 200, 200, 0, true);
        contact(&mut t, 120, 210, 80, false);
        assert_eq!(lift(&mut t, 120), Some(Gesture::Swipe(-80)));
    }

    #[test]
    fn second_finger_cancels_and_drop_resets() {
        let (ax, ay) = axis();
        let mut t = Touch::new(ax, ay);
        contact(&mut t, 50, 50, 0, true);
        t.event(EV_ABS, ABS_MT_SLOT, 1, 10);
        t.event(EV_ABS, ABS_MT_TRACKING_ID, 8, 10);
        t.event(EV_SYN, SYN_REPORT, 0, 10);
        t.event(EV_ABS, ABS_MT_TRACKING_ID, -1, 20);
        t.event(EV_ABS, ABS_MT_SLOT, 0, 20);
        assert_eq!(lift(&mut t, 30), None);

        contact(&mut t, 50, 50, 100, true);
        t.event(EV_SYN, SYN_DROPPED, 0, 110);
        assert_eq!(lift(&mut t, 120), None);
    }

    #[test]
    fn long_hold_is_not_a_tap() {
        let (ax, ay) = axis();
        let mut t = Touch::new(ax, ay);
        contact(&mut t, 10, 10, 0, true);
        assert_eq!(lift(&mut t, 2000), None);
    }

    #[test]
    fn power_key_short_and_long() {
        let mut k = PowerKey::default();
        assert_eq!(k.event(EV_KEY, KEY_POWER, 1, 1000), None);
        assert_eq!(k.event(EV_KEY, KEY_POWER, 0, 1200), Some(Press::Short));
        // Bounce and repeats are ignored.
        k.event(EV_KEY, KEY_POWER, 1, 2000);
        assert_eq!(k.event(EV_KEY, KEY_POWER, 0, 2010), None);
        // A hold fires once while down; its release is silent.
        k.event(EV_KEY, KEY_POWER, 1, 3000);
        k.event(EV_KEY, KEY_POWER, 2, 3500);
        assert_eq!(k.poll(4000), None);
        assert_eq!(k.poll(4600), Some(Press::Long));
        assert_eq!(k.poll(4700), None);
        assert_eq!(k.event(EV_KEY, KEY_POWER, 0, 5000), None);
        // Press and release read late together still count as long.
        k.event(EV_KEY, KEY_POWER, 1, 6000);
        assert_eq!(k.event(EV_KEY, KEY_POWER, 0, 7700), Some(Press::Long));
        // A dropped report forgets the press.
        k.event(EV_KEY, KEY_POWER, 1, 8000);
        k.event(EV_SYN, SYN_DROPPED, 0, 8100);
        assert_eq!(k.poll(10_000), None);
    }
}
