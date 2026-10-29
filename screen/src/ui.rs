//! Page layout and touch handling. Everything is drawn upright into a
//! `Canvas`; the hardware layer only rotates and presents it.

use std::collections::VecDeque;

use crate::canvas::{dim, rgb, Canvas, W};
use crate::font::Size::{Large, Medium, Small};
use crate::pet::Pet;
use crate::settings::{IdleAction, Settings, Theme, IDLE_CHOICES, PAGES};
use crate::stats::{Model, Proc};

pub const HEADER_H: i32 = 40;
pub const TAB_Y: i32 = 436;
pub const TOP: i32 = HEADER_H + 4;
const TAB_W: i32 = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Home,
    Sys,
    Pwr,
    Cell,
    Net,
    Lan,
    Proxy,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::Home,
        Page::Sys,
        Page::Pwr,
        Page::Cell,
        Page::Net,
        Page::Lan,
        Page::Proxy,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Home => "HOME",
            Page::Sys => "SYS",
            Page::Pwr => "PWR",
            Page::Cell => "CELL",
            Page::Net => "NET",
            Page::Lan => "LAN",
            Page::Proxy => "PROXY",
        }
    }

    pub fn index(self) -> usize {
        Page::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

pub struct Palette {
    pub bg: u16,
    pub fg: u16,
    pub bright: u16,
    pub dim: u16,
    pub faint: u16,
    pub grid: u16,
    pub accent: u16,
    pub accent2: u16,
    pub ok: u16,
    pub warn: u16,
    pub crit: u16,
}

impl Palette {
    pub fn of(theme: Theme) -> Self {
        let (fg, bright) = match theme {
            Theme::Green => (rgb(0x4d, 0xff, 0x88), rgb(0xc8, 0xff, 0xd8)),
            Theme::Amber => (rgb(0xff, 0xb0, 0x30), rgb(0xff, 0xe2, 0xa8)),
            Theme::Cyan => (rgb(0x40, 0xd8, 0xff), rgb(0xc8, 0xf4, 0xff)),
        };
        Palette {
            bg: rgb(0x02, 0x06, 0x04),
            fg,
            bright,
            dim: dim(fg, 120),
            faint: dim(fg, 56),
            grid: dim(fg, 28),
            accent: rgb(0x3d, 0xd6, 0xff),
            accent2: rgb(0xff, 0x5f, 0xd2),
            ok: rgb(0x4d, 0xff, 0x88),
            warn: rgb(0xff, 0xb3, 0x3d),
            crit: rgb(0xff, 0x4d, 0x4d),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Warn,
    Crit,
}

/// Conditions worth a banner, most severe first.
pub fn alerts(m: &Model) -> Vec<(Level, String)> {
    let mut out = Vec::new();
    if let Some(t) = m.temp_cpu {
        if t >= 80.0 {
            out.push((Level::Crit, format!("cpu at {t:.0}C")));
        } else if t >= 68.0 {
            out.push((Level::Warn, format!("cpu warm {t:.0}C")));
        }
    }
    if let Some(t) = m.battery.temp_c {
        if t >= 45.0 {
            out.push((Level::Crit, format!("battery hot {t:.0}C")));
        }
    }
    if let Some(p) = m.battery.percent {
        if p <= 10 && !m.battery.external_power {
            out.push((Level::Crit, format!("battery {p}%")));
        } else if p <= 20 && !m.battery.external_power {
            out.push((Level::Warn, format!("battery {p}%")));
        }
    }
    match &m.radio {
        Some(r) if r.rsrp.is_none() => out.push((Level::Crit, "no cellular signal".into())),
        Some(r) if r.rsrp.is_some_and(|v| v < -115.0) => {
            out.push((Level::Warn, "weak signal".into()))
        }
        _ => {}
    }
    if m.wan_ip.is_none() && m.uptime_s > 120 {
        out.push((Level::Crit, "wan has no ipv4".into()));
    }
    if m.mem_total_kb > 0 && m.mem_avail_kb * 10 < m.mem_total_kb {
        out.push((Level::Warn, "memory below 10%".into()));
    }
    for (label, up) in &m.services {
        if !up && *label != "proxy" {
            out.push((Level::Crit, format!("{label} is down")));
        }
    }
    for (disk, used) in &m.disks {
        if *used >= 90.0 {
            out.push((Level::Warn, format!("/{disk} {used:.0}% full")));
        }
    }
    if m.cooling_active > 0 {
        out.push((
            Level::Warn,
            format!("thermal throttle x{}", m.cooling_active),
        ));
    }
    out.sort_by_key(|(l, _)| if *l == Level::Crit { 0 } else { 1 });
    out
}

pub fn bits(bytes_per_s: f64) -> String {
    let b = bytes_per_s * 8.0;
    if b >= 1e9 {
        format!("{:.2}G", b / 1e9)
    } else if b >= 1e8 {
        format!("{:.0}M", b / 1e6)
    } else if b >= 1e6 {
        format!("{:.1}M", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0}k", b / 1e3)
    } else {
        format!("{b:.0}")
    }
}

pub fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2}G", n / 1024.0 / 1024.0 / 1024.0)
    } else if n >= 100.0 * 1024.0 * 1024.0 {
        format!("{:.0}M", n / 1024.0 / 1024.0)
    } else if n >= 1024.0 * 1024.0 {
        format!("{:.1}M", n / 1024.0 / 1024.0)
    } else if n >= 1024.0 {
        format!("{:.0}K", n / 1024.0)
    } else {
        format!("{n:.0}B")
    }
}

pub fn duration(s: u64) -> String {
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 {
        format!("{d}d{h:02}h{m:02}m")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m{:02}s", s % 60)
    }
}

fn kb(n: u64) -> String {
    bytes(n * 1024)
}

pub fn truncate(s: &str, cells: usize) -> String {
    if s.chars().count() <= cells {
        s.to_string()
    } else {
        s.chars()
            .take(cells.saturating_sub(1))
            .chain(std::iter::once('~'))
            .collect()
    }
}

/// Quality class of a reading against thresholds (excellent, good, fair).
/// The thresholds follow the dashboard's policy in web-app/design.md
/// § Signal ratings, so the LCD and the web page agree.
fn grade(v: f32, good: [f32; 3]) -> usize {
    good.iter().position(|t| v >= *t).unwrap_or(3)
}

const GRADES: [&str; 4] = ["EXCELLENT", "GOOD", "FAIR", "POOR"];

pub enum Action {
    None,
    Redraw,
    Exit,
    SettingsChanged,
}

pub struct Ui {
    pub page: Page,
    pub settings: Settings,
    pub settings_open: bool,
    /// Frames since start: the boot splash covers the first few.
    pub splash: u32,
    pub blink: bool,
    /// Seconds left before the idle action, when it is close.
    pub idle_left: Option<u32>,
    /// The whale girl at the top of HOME.
    pub pet: Pet,
    /// The PROXY page's screens and the work it started.
    pub proxy: crate::proxy::View,
}

impl Ui {
    pub fn new(settings: Settings) -> Self {
        let page = Page::ALL[settings.start_page.min(Page::ALL.len() - 1)];
        Ui {
            page,
            settings,
            settings_open: false,
            splash: 0,
            blink: false,
            idle_left: None,
            pet: Pet::load(),
            proxy: crate::proxy::View::default(),
        }
    }

    pub fn palette(&self) -> Palette {
        Palette::of(self.settings.theme)
    }

    pub fn swipe(&mut self, dx: i32) -> Action {
        if self.settings_open {
            return Action::None;
        }
        let i = self.page.index() as i32 + if dx < 0 { 1 } else { -1 };
        if (0..Page::ALL.len() as i32).contains(&i) {
            self.page = Page::ALL[i as usize];
            Action::Redraw
        } else {
            Action::None
        }
    }

    pub fn tap(&mut self, x: i32, y: i32, m: &mut Model) -> Action {
        if self.splash < SPLASH_FRAMES {
            return Action::None;
        }
        if self.settings_open {
            return self.tap_settings(x, y);
        }
        if y >= TAB_Y {
            let i = x / TAB_W;
            if i >= Page::ALL.len() as i32 {
                return Action::Exit;
            }
            self.page = Page::ALL[i as usize];
            return Action::Redraw;
        }
        if y < HEADER_H && x >= 224 {
            self.settings_open = true;
            return Action::Redraw;
        }
        if self.page == Page::Home && self.pet.tap(x, y, m) {
            return Action::Redraw;
        }
        if self.page == Page::Proxy && self.proxy.tap(x, y, m) {
            return Action::Redraw;
        }
        Action::None
    }

    fn tap_settings(&mut self, x: i32, y: i32) -> Action {
        let row = (y - SETTINGS_Y - 44) / SETTINGS_ROW;
        let s = &mut self.settings;
        if !(SETTINGS_X..SETTINGS_X + SETTINGS_W).contains(&x) || y < SETTINGS_Y + 44 {
            self.settings_open = false;
            return Action::Redraw;
        }
        match row {
            0 => {
                let delta: i32 = if x < SETTINGS_X + SETTINGS_W / 2 {
                    -24
                } else {
                    24
                };
                s.brightness = (s.brightness as i32 + delta).clamp(20, 255) as u8;
            }
            1 => {
                let i = IDLE_CHOICES
                    .iter()
                    .position(|c| *c >= s.idle_secs)
                    .unwrap_or(0);
                s.idle_secs = IDLE_CHOICES[(i + 1) % IDLE_CHOICES.len()];
            }
            2 => {
                s.idle_action = match s.idle_action {
                    IdleAction::Stock => IdleAction::Sleep,
                    IdleAction::Sleep => IdleAction::Stock,
                }
            }
            3 => s.theme = s.theme.next(),
            4 => s.scanlines = !s.scanlines,
            5 => s.start_page = (s.start_page + 1) % PAGES.len(),
            6 => s.auto_open = s.auto_open.next(),
            _ => {
                self.settings_open = false;
                return Action::Redraw;
            }
        }
        Action::SettingsChanged
    }

    pub fn render(&self, c: &mut Canvas, m: &Model) {
        let p = self.palette();
        c.clear(p.bg);
        if self.splash < SPLASH_FRAMES {
            self.render_splash(c, m, &p);
        } else {
            self.header(c, m, &p);
            match self.page {
                Page::Home => {
                    self.pet.render(c, &p);
                    home(c, m, &p)
                }
                Page::Sys => sys(c, m, &p),
                Page::Pwr => pwr(c, m, &p),
                Page::Cell => cell(c, m, &p),
                Page::Net => net(c, m, &p),
                Page::Lan => lan(c, m, &p),
                Page::Proxy => self.proxy.render(c, m, &p),
            }
            if let Some(left) = self.idle_left {
                let msg = match self.settings.idle_action {
                    IdleAction::Stock => format!("idle: stock ui in {left}s - tap to stay"),
                    IdleAction::Sleep => format!("idle: display off in {left}s"),
                };
                c.fill_rect(0, TAB_Y - 20, W, 18, p.bg);
                c.hline(0, TAB_Y - 21, W, p.warn);
                c.text_center(W / 2, TAB_Y - 19, Small, p.warn, &msg);
            }
            self.tabs(c, &p);
            if self.settings_open {
                self.render_settings(c, &p);
            }
        }
        if self.settings.scanlines {
            // 咕嘎 is drawn from a picture: keep her room clean.
            let keep =
                (self.splash >= SPLASH_FRAMES && self.page == Page::Home && !self.settings_open)
                    .then_some(crate::pet::ROOM);
            c.scanlines_except(keep);
        }
    }

    fn header(&self, c: &mut Canvas, m: &Model, p: &Palette) {
        let x = c.text(4, 3, Small, p.fg, "root@u60pro");
        let x = c.text(x, 3, Small, p.dim, ":~#");
        if self.blink {
            c.fill_rect(x + 4, 4, 8, 14, p.fg);
        }
        c.text_right(
            W - 4,
            3,
            Small,
            p.bright,
            if m.clock.is_empty() {
                "--:--:--"
            } else {
                &m.clock
            },
        );

        // Second row: radio, then alerts, temperature and battery from the right.
        let y = 22;
        let bars = m.radio.as_ref().and_then(|r| r.bars).unwrap_or(0);
        for i in 0..5 {
            let h = 3 + i * 3;
            let col = if (i as u8) < bars { p.fg } else { p.grid };
            c.fill_rect(4 + i * 4, y + 13 - h, 3, h, col);
        }
        let radio = m
            .radio
            .as_ref()
            .map(|r| format!("{} {}", rat_label(&r.rat), r.band))
            .unwrap_or_else(|| "no radio".into());
        c.text(28, y, Small, p.bright, &truncate(&radio, 14));

        let mut right = W - 4;
        if let Some(pct) = m.battery.percent {
            let col = if pct <= 20 && !m.battery.external_power {
                p.crit
            } else {
                p.fg
            };
            let label = format!("{pct}%");
            right = c.text_right(right, y, Small, col, &label) - 4;
            c.frame(right - 20, y + 3, 18, 10, col);
            c.fill_rect(right - 2, y + 6, 2, 4, col);
            c.fill_rect(right - 18, y + 5, (14 * pct as i32 / 100).max(1), 6, col);
            if m.battery.external_power {
                c.text(right - 34, y, Small, p.warn, "+");
            }
            right -= 40;
        }
        if let Some(t) = m.temp_cpu {
            let col = if t >= 80.0 {
                p.crit
            } else if t >= 68.0 {
                p.warn
            } else {
                p.dim
            };
            right = c.text_right(right, y, Small, col, &format!("{t:.0}\u{b0}")) - 8;
        }
        let a = alerts(m);
        if !a.is_empty() && self.blink {
            let col = if a[0].0 == Level::Crit {
                p.crit
            } else {
                p.warn
            };
            c.text_right(right, y, Small, col, &format!("!{}", a.len()));
        }
        c.dotted(0, HEADER_H - 1, W, 2, p.faint);
    }

    fn tabs(&self, c: &mut Canvas, p: &Palette) {
        c.hline(0, TAB_Y, W, p.dim);
        for (i, page) in Page::ALL.iter().enumerate() {
            let x = i as i32 * TAB_W;
            if *page == self.page {
                c.fill_rect(x + 2, TAB_Y + 4, TAB_W - 4, 36, p.fg);
                c.text_center(x + TAB_W / 2, TAB_Y + 14, Small, p.bg, page.label());
            } else {
                c.text_center(x + TAB_W / 2, TAB_Y + 14, Small, p.dim, page.label());
            }
            c.vline(x + TAB_W - 1, TAB_Y + 8, 28, p.grid);
        }
        let x = Page::ALL.len() as i32 * TAB_W;
        c.text_center(x + (W - x) / 2, TAB_Y + 14, Small, p.crit, ":q");
    }

    fn render_splash(&self, c: &mut Canvas, m: &Model, p: &Palette) {
        let lines: [(&str, String); 9] = [
            (
                "",
                format!("U60 Pro geek shell v{}", env!("CARGO_PKG_VERSION")),
            ),
            (
                "",
                format!(
                    "kernel {}",
                    if m.kernel.is_empty() { "?" } else { &m.kernel }
                ),
            ),
            ("OK", "drm: qpic 320x480 rgb565 atomic".into()),
            ("OK", "input: sitronix touch, power key".into()),
            ("OK", "stock ui parked".into()),
            ("OK", "watchdog: supervisor heartbeat".into()),
            ("OK", "sensors: thermal, battery, modem".into()),
            ("OK", "net: modem, wifi, mihomo".into()),
            ("", "login: root (automatic)".into()),
        ];
        let shown = ((self.splash + 1) * 3).min(lines.len() as u32) as usize;
        let mut y = 40;
        c.text(8, 12, Small, p.dim, "[    0.000000] booting...");
        for (status, text) in lines.iter().take(shown) {
            let mut x = 8;
            if !status.is_empty() {
                x = c.text(x, y, Small, p.dim, "[ ");
                x = c.text(x, y, Small, p.ok, status);
                x = c.text(x, y, Small, p.dim, " ] ");
            }
            c.text(x, y, Small, p.fg, &truncate(text, ((W - x) / 8) as usize));
            y += 20;
        }
        logo(c, 108, 280, p);
    }

    fn render_settings(&self, c: &mut Canvas, p: &Palette) {
        let s = &self.settings;
        let (x, y, w) = (SETTINGS_X, SETTINGS_Y, SETTINGS_W);
        let h = 44 + SETTINGS_ROW * 8 + 8;
        c.fill_rect(x, y, w, h, p.bg);
        c.frame(x, y, w, h, p.fg);
        c.frame(x + 2, y + 2, w - 4, h - 4, p.faint);
        c.text(x + 12, y + 12, Medium, p.bright, "# screen.conf");
        let rows: [(&str, String); 7] = [
            (
                "brightness",
                format!("[-] {:3}% [+]", s.brightness as u32 * 100 / 255),
            ),
            ("idle", format!("[ {}s ]", s.idle_secs)),
            (
                "on_idle",
                format!(
                    "[ {} ]",
                    if s.idle_action == IdleAction::Stock {
                        "stock"
                    } else {
                        "sleep"
                    }
                ),
            ),
            ("theme", format!("[ {} ]", s.theme.name())),
            (
                "crt_lines",
                format!("[ {} ]", if s.scanlines { "on" } else { "off" }),
            ),
            (
                "start_page",
                format!("[ {} ]", PAGES[s.start_page.min(PAGES.len() - 1)]),
            ),
            ("auto_open", format!("[ {} ]", s.auto_open.name())),
        ];
        for (i, (k, v)) in rows.iter().enumerate() {
            let ry = y + 44 + i as i32 * SETTINGS_ROW;
            c.dotted(x + 8, ry, w - 16, 2, p.grid);
            c.text(x + 12, ry + 13, Small, p.dim, k);
            c.text_right(x + w - 12, ry + 13, Small, p.fg, v);
        }
        let by = y + 44 + 7 * SETTINGS_ROW + 6;
        c.fill_rect(x + w / 2 - 44, by, 88, SETTINGS_ROW - 10, p.fg);
        c.text_center(x + w / 2, by + 9, Small, p.bg, ":wq");
    }
}

pub const SPLASH_FRAMES: u32 = 4;
const SETTINGS_X: i32 = 16;
const SETTINGS_Y: i32 = 56;
const SETTINGS_W: i32 = W - 32;
const SETTINGS_ROW: i32 = 42;

fn rat_label(rat: &str) -> String {
    match rat {
        "SA" => "5G SA".into(),
        "NSA" | "ENDC" => "5G NSA".into(),
        "LTE" | "LTE_CA" | "4G" => "4G".into(),
        "" => "--".into(),
        other => other.to_string(),
    }
}

fn logo(c: &mut Canvas, x: i32, y: i32, p: &Palette) {
    let art = [
        " .--------. ",
        " |  ~~~~  | ",
        " |  U60   | ",
        " |  PRO   | ",
        " |  5G )) | ",
        " | [====] | ",
        " '--------' ",
    ];
    for (i, line) in art.iter().enumerate() {
        let col = if i == 4 { p.accent } else { p.fg };
        c.text(x, y + i as i32 * 16, Small, col, line);
    }
}

fn section(c: &mut Canvas, y: i32, title: &str, p: &Palette) {
    let x = c.text(4, y, Small, p.dim, "# ");
    let x = c.text(x, y, Small, p.bright, title);
    c.dotted(x + 6, y + 8, W - x - 10, 3, p.faint);
}

fn kv(c: &mut Canvas, x: i32, y: i32, key: &str, value: &str, p: &Palette, col: u16) -> i32 {
    let x = c.text(x, y, Small, p.dim, key);
    c.text(x + 8, y, Small, col, value)
}

fn temp_col(t: f32, p: &Palette) -> u16 {
    if t >= 80.0 {
        p.crit
    } else if t >= 68.0 {
        p.warn
    } else {
        p.fg
    }
}

fn load_col(pct: f32, p: &Palette) -> u16 {
    if pct >= 85.0 {
        p.crit
    } else if pct >= 60.0 {
        p.warn
    } else {
        p.fg
    }
}

/// htop-style meter: `label[|||||      text]`.
fn htop_bar(c: &mut Canvas, y: i32, label: &str, parts: &[(f32, u16)], text: &str, p: &Palette) {
    let x = c.text(4, y, Small, p.accent, label);
    let x = c.text(x, y, Small, p.bright, "[");
    let cells = ((W - 8 - x) / 8 - 1) as usize;
    // Like htop, the bar stops short of the figure printed at its end.
    let room = cells.saturating_sub(text.chars().count() + 1);
    let mut drawn = 0usize;
    for (frac, col) in parts {
        let n = ((frac.clamp(0.0, 1.0) * room as f32) + 0.5) as usize;
        let n = n.min(room - drawn);
        c.text(x + drawn as i32 * 8, y, Small, *col, &"|".repeat(n));
        drawn += n;
    }
    let end = x + cells as i32 * 8;
    c.text_right(end, y, Small, p.dim, text);
    c.text(end, y, Small, p.bright, "]");
}

fn graph_box(c: &mut Canvas, y: i32, h: i32, label: &str, p: &Palette) -> (i32, i32, i32, i32) {
    let (x, w) = (4, W - 8);
    c.frame(x, y, w, h, p.faint);
    for i in 1..4 {
        c.dotted(x + 1, y + h * i / 4, w - 2, 4, p.grid);
    }
    for i in 1..6 {
        c.vline(x + w * i / 6, y + 1, h - 2, p.grid);
    }
    c.fill_rect(x + 6, y - 7, label.len() as i32 * 8 + 4, 14, p.bg);
    c.text(x + 8, y - 8, Small, p.dim, label);
    (x + 1, y + 2, w - 2, h - 4)
}

fn series(h: &VecDeque<f32>) -> Vec<f32> {
    h.iter().copied().collect()
}

fn home(c: &mut Canvas, m: &Model, p: &Palette) {
    // The pet owns the top of the page (`Pet::render`); compact status follows.
    let cpu_avg = if m.cpu.is_empty() {
        0.0
    } else {
        m.cpu.iter().sum::<f32>() / m.cpu.len() as f32
    };
    let max_mhz = m.freq_mhz.iter().max().copied().unwrap_or(0);
    let used = m.mem_total_kb.saturating_sub(m.mem_avail_kb);
    let deg = |t: Option<f32>| t.map(|t| format!("{t:.0}\u{b0}")).unwrap_or("--".into());
    let rows: [[(&str, String, u16); 2]; 3] = [
        [
            ("Up", duration(m.uptime_s), p.fg),
            (
                "Load",
                format!("{:.2} {:.2} {:.2}", m.load[0], m.load[1], m.load[2]),
                p.fg,
            ),
        ],
        [
            (
                "CPU",
                format!(
                    "{}x{:.1}G {:.0}%",
                    m.cpu.len(),
                    max_mhz as f32 / 1000.0,
                    cpu_avg
                ),
                load_col(cpu_avg, p),
            ),
            ("Mem", format!("{}/{}", kb(used), kb(m.mem_total_kb)), p.fg),
        ],
        [
            (
                "Temp",
                format!("{}/{}", deg(m.temp_cpu), deg(m.temp_modem)),
                m.temp_cpu.map(|t| temp_col(t, p)).unwrap_or(p.fg),
            ),
            (
                "Pwr",
                format!(
                    "{}% {}",
                    m.battery
                        .percent
                        .map(|v| v.to_string())
                        .unwrap_or("--".into()),
                    if m.battery.external_power {
                        "AC"
                    } else {
                        "BAT"
                    }
                ),
                p.fg,
            ),
        ],
    ];
    let mut y = crate::pet::BOTTOM;
    for row in rows {
        for (i, (k, v, col)) in row.iter().enumerate() {
            let x = 4 + i as i32 * 160;
            c.text(x, y, Small, p.accent, k);
            c.text(x + 40, y, Small, *col, v);
        }
        y += 17;
    }

    let mut y = y + 2;
    section(c, y, "wan", p);
    y += 20;
    match &m.radio {
        Some(r) => {
            let x = c.text(4, y, Medium, p.bright, &rat_label(&r.rat));
            let x = c.text(x + 10, y, Medium, p.accent, &r.band);
            if let Some(rsrp) = r.rsrp {
                let g = grade(rsrp, [-80.0, -90.0, -100.0]);
                let col = [p.ok, p.ok, p.warn, p.crit][g];
                c.text_right(W - 4, y, Medium, col, &format!("{rsrp:.0}dBm"));
            }
            c.text(x + 8, y + 6, Small, p.dim, &truncate(&r.operator, 10));
        }
        None => {
            c.text(4, y, Medium, p.crit, "no radio data");
        }
    }
    y += 26;
    let x = c.text(4, y, Small, p.accent, "\u{2193}");
    let x = c.text(
        x + 4,
        y,
        Small,
        p.bright,
        &format!("{:>6}b/s", m.dl.map(bits).unwrap_or("--".into())),
    );
    let x = c.text(x + 12, y, Small, p.accent2, "\u{2191}");
    c.text(
        x + 4,
        y,
        Small,
        p.bright,
        &format!("{:>6}b/s", m.ul.map(bits).unwrap_or("--".into())),
    );
    let max = m
        .dl_hist
        .iter()
        .chain(m.ul_hist.iter())
        .fold(1.0f32, |a, b| a.max(*b));
    c.plot(232, y - 2, 84, 18, &series(&m.dl_hist), max, p.accent, p.bg);
    c.plot(
        232,
        y - 2,
        84,
        18,
        &series(&m.ul_hist),
        max,
        p.accent2,
        p.bg,
    );
    y += 20;
    kv(c, 4, y, "ip", m.wan_ip.as_deref().unwrap_or("--"), p, p.fg);
    let v6 = match m.wan6_up {
        Some(true) => ("v6 up", p.ok),
        Some(false) => ("v6 down", p.warn),
        None => ("v6 --", p.dim),
    };
    c.text(160, y, Small, v6.1, v6.0);
    c.text_right(
        W - 4,
        y,
        Small,
        p.fg,
        &format!("{} dev", m.clients.unwrap_or(0)),
    );
    y += 17;
    if let Some((level, msg)) = alerts(m).first() {
        let (tag, col) = if *level == Level::Crit {
            ("CRIT", p.crit)
        } else {
            ("WARN", p.warn)
        };
        let x = c.text(4, y, Small, p.dim, "[");
        let x = c.text(x, y, Small, col, tag);
        let x = c.text(x, y, Small, p.dim, "] ");
        c.text(x, y, Small, col, &truncate(msg, 32));
        return;
    }
    let proxy = match (&m.mihomo, &m.proxy) {
        (Some(proc_), Some(px)) => format!(
            "mihomo {} {} conn {}",
            px.mode,
            kb(proc_.rss_kb),
            px.connections
        ),
        (Some(proc_), None) => format!("mihomo up {}", kb(proc_.rss_kb)),
        (None, _) => "mihomo off".into(),
    };
    kv(
        c,
        4,
        y,
        "proxy",
        &truncate(&proxy, 32),
        p,
        if m.mihomo.is_some() { p.fg } else { p.dim },
    );
}

fn sys(c: &mut Canvas, m: &Model, p: &Palette) {
    let mut y = TOP;
    for (i, pct) in m.cpu.iter().enumerate() {
        let mhz = m.freq_mhz.get(i).copied().unwrap_or(0);
        htop_bar(
            c,
            y,
            &format!("{i}"),
            &[(pct / 100.0, load_col(*pct, p))],
            &format!("{pct:4.1}% {mhz:4}"),
            p,
        );
        y += 17;
    }
    let total = m.mem_total_kb.max(1) as f32;
    let used = m.mem_total_kb.saturating_sub(m.mem_avail_kb);
    let cached = m.mem_cached_kb.min(m.mem_avail_kb);
    htop_bar(
        c,
        y,
        "M",
        &[(used as f32 / total, p.ok), (cached as f32 / total, p.warn)],
        &format!("{}/{}", kb(used), kb(m.mem_total_kb)),
        p,
    );
    y += 22;
    let x = kv(
        c,
        4,
        y,
        "tasks",
        &format!("{} {} run", m.tasks, m.running),
        p,
        p.fg,
    );
    kv(c, x.max(150) + 8, y, "up", &duration(m.uptime_s), p, p.fg);
    y += 17;
    kv(
        c,
        4,
        y,
        "load",
        &format!("{:.2} {:.2} {:.2}", m.load[0], m.load[1], m.load[2]),
        p,
        p.fg,
    );
    c.text_right(W - 4, y, Small, p.dim, &m.governor);
    y += 17;
    kv(
        c,
        4,
        y,
        "ctx",
        &format!(
            "{:.1}k/s  irq {:.1}k/s",
            m.ctxt_rate / 1000.0,
            m.intr_rate / 1000.0
        ),
        p,
        p.fg,
    );
    y += 26;
    let (gx, gy, gw, gh) = graph_box(c, y, 56, "cpu% 60s", p);
    c.plot(gx, gy, gw, gh, &series(&m.cpu_hist), 100.0, p.fg, p.faint);
    y += 64;
    let x = c.text(4, y, Small, p.dim, "temp");
    let mut x = x + 8;
    for (label, t) in [
        ("cpu", m.temp_cpu),
        ("mdm", m.temp_modem),
        ("xo", m.temp_board),
        ("bat", m.battery.temp_c),
    ] {
        x = c.text(x, y, Small, p.dim, label);
        let s = t.map(|v| format!("{v:.0}\u{b0}")).unwrap_or("--".into());
        x = c.text(
            x + 4,
            y,
            Small,
            t.map(|v| temp_col(v, p)).unwrap_or(p.dim),
            &s,
        ) + 8;
    }
    y += 17;
    let mut x = c.text(4, y, Small, p.dim, "disk") + 8;
    for (label, used) in &m.disks {
        x = c.text(x, y, Small, p.dim, label);
        let col = if *used >= 90.0 {
            p.crit
        } else if *used >= 75.0 {
            p.warn
        } else {
            p.fg
        };
        x = c.text(x + 4, y, Small, col, &format!("{used:.0}%")) + 8;
    }
    y += 22;
    c.fill_rect(0, y - 1, W, 17, p.fg);
    c.text(4, y, Small, p.bg, "  PID  CPU%    RSS COMMAND");
    y += 18;
    for proc_ in m.top.iter().take(7) {
        proc_row(c, y, proc_, p);
        y += 16;
    }
}

fn proc_row(c: &mut Canvas, y: i32, proc_: &Proc, p: &Palette) {
    let col = load_col(proc_.cpu, p);
    c.text(4, y, Small, p.dim, &format!("{:>5}", proc_.pid));
    c.text(52, y, Small, col, &format!("{:>5.1}", proc_.cpu));
    c.text(100, y, Small, p.fg, &format!("{:>6}", kb(proc_.rss_kb)));
    c.text(156, y, Small, p.bright, &truncate(&proc_.name, 20));
}

fn cell(c: &mut Canvas, m: &Model, p: &Palette) {
    let Some(r) = &m.radio else {
        c.text(4, TOP + 20, Medium, p.crit, "waiting for modem...");
        return;
    };
    let mut y = TOP + 2;
    let x = c.text(4, y, Large, p.bright, &rat_label(&r.rat));
    let x = c.text(x + 12, y, Large, p.accent, &r.band);
    if !r.bandwidth.is_empty() {
        c.text(x + 6, y + 14, Small, p.dim, &format!("{}MHz", r.bandwidth));
    }
    for i in 0..5u8 {
        let h = 6 + i as i32 * 5;
        let col = if Some(i) < r.bars { p.fg } else { p.grid };
        c.fill_rect(W - 48 + i as i32 * 9, y + 30 - h, 7, h, col);
    }
    y += 36;
    let x = c.text(4, y, Small, p.fg, &truncate(&r.operator, 18));
    let x = c.text(
        x + 8,
        y,
        Small,
        if r.roaming { p.warn } else { p.dim },
        if r.roaming { "ROAM" } else { "HOME" },
    );
    c.text(
        x + 8,
        y,
        Small,
        p.dim,
        &truncate(&r.mode.to_lowercase(), 12),
    );
    y += 22;

    // Label, value, unit, grade thresholds, meter range.
    type Reading<'a> = (&'a str, Option<f32>, &'a str, [f32; 3], (f32, f32));
    let rows: [Reading; 2] = [
        (
            "RSRP",
            r.rsrp,
            "dBm",
            [-80.0, -90.0, -100.0],
            (-130.0, -60.0),
        ),
        ("SINR", r.sinr, "dB", [20.0, 10.0, 0.0], (-5.0, 30.0)),
    ];
    for (label, v, unit, good, range) in rows {
        c.text(4, y + 8, Small, p.dim, label);
        match v {
            Some(v) => {
                let g = grade(v, good);
                let col = [p.ok, p.ok, p.warn, p.crit][g];
                let x = c.text(48, y, Large, col, &format!("{v:.0}"));
                c.text(x + 4, y + 14, Small, p.dim, unit);
                let frac = (v - range.0) / (range.1 - range.0);
                c.meter(160, y + 4, 100, 12, frac, col, p.grid);
                c.text_right(W - 4, y + 18, Small, col, GRADES[g]);
            }
            None => {
                c.text(48, y, Large, p.dim, "--");
            }
        }
        y += 38;
    }
    let fmt =
        |v: Option<f32>, unit: &str| v.map(|v| format!("{v:.0}{unit}")).unwrap_or("--".into());
    let x = kv(c, 4, y, "rsrq", &fmt(r.rsrq, "dB"), p, p.fg);
    kv(c, x.max(150) + 8, y, "rssi", &fmt(r.rssi, "dBm"), p, p.fg);
    y += 17;
    let x = kv(
        c,
        4,
        y,
        "pci",
        &r.pci.map(|v| v.to_string()).unwrap_or("--".into()),
        p,
        p.fg,
    );
    kv(
        c,
        x.max(150) + 8,
        y,
        "arfcn",
        &r.arfcn.map(|v| v.to_string()).unwrap_or("--".into()),
        p,
        p.fg,
    );
    y += 17;
    kv(
        c,
        4,
        y,
        "ca",
        if r.ca.is_empty() { "none" } else { &r.ca },
        p,
        if r.ca.is_empty() { p.dim } else { p.accent },
    );
    y += 28;
    let (gx, gy, gw, gh) = graph_box(c, y, 64, "rsrp 3min", p);
    let rsrp: Vec<f32> = m.rsrp_hist.iter().map(|v| v + 140.0).collect();
    c.plot(gx, gy, gw, gh, &rsrp, 100.0, p.fg, p.faint);
    c.text_right(W - 8, y + 4, Small, p.faint, "-40");
    c.text_right(W - 8, y + 64 - 18, Small, p.faint, "-140");
    y += 76;
    let (gx, gy, gw, gh) = graph_box(c, y, 64, "sinr 3min", p);
    let sinr: Vec<f32> = m.sinr_hist.iter().map(|v| v + 10.0).collect();
    c.plot(gx, gy, gw, gh, &sinr, 50.0, p.accent, dim(p.accent, 60));
    c.text_right(W - 8, y + 4, Small, p.faint, "40");
    c.text_right(W - 8, y + 64 - 18, Small, p.faint, "-10");
}

fn net(c: &mut Canvas, m: &Model, p: &Palette) {
    let mut y = TOP;
    c.text(4, y + 10, Small, p.accent, "\u{2193}");
    let x = c.text(
        16,
        y,
        Large,
        p.accent,
        &m.dl.map(bits).unwrap_or("--".into()),
    );
    c.text(x + 2, y + 14, Small, p.dim, "b/s");
    c.text(164, y + 10, Small, p.accent2, "\u{2191}");
    let x = c.text(
        176,
        y,
        Large,
        p.accent2,
        &m.ul.map(bits).unwrap_or("--".into()),
    );
    c.text(x + 2, y + 14, Small, p.dim, "b/s");
    y += 34;
    c.text(
        4,
        y,
        Small,
        p.dim,
        &format!(
            "peak \u{2193}{}  \u{2191}{}  (b/s)",
            bits(m.peak_dl),
            bits(m.peak_ul)
        ),
    );
    y += 26;
    let (gx, gy, gw, gh) = graph_box(c, y, 60, "wan 60s", p);
    let max = m
        .dl_hist
        .iter()
        .chain(m.ul_hist.iter())
        .fold(1.0f32, |a, b| a.max(*b));
    c.plot(
        gx,
        gy,
        gw,
        gh,
        &series(&m.dl_hist),
        max,
        p.accent,
        dim(p.accent, 50),
    );
    c.plot(
        gx,
        gy,
        gw,
        gh,
        &series(&m.ul_hist),
        max,
        p.accent2,
        p.accent2,
    );
    c.text_right(W - 8, y + 4, Small, p.faint, &bits(max as f64));
    y += 68;
    let session = match (m.session_s, m.session_rx, m.session_tx) {
        (Some(s), Some(rx), Some(tx)) => format!(
            "{} \u{2193}{} \u{2191}{}",
            duration(s),
            bytes(rx),
            bytes(tx)
        ),
        _ => "--".into(),
    };
    kv(c, 4, y, "session", &session, p, p.fg);
    y += 17;
    let usage = |rx: Option<u64>, tx: Option<u64>| match (rx, tx) {
        (Some(rx), Some(tx)) => bytes(rx + tx),
        _ => "--".into(),
    };
    let x = kv(c, 4, y, "today", &usage(m.day_rx, m.day_tx), p, p.fg);
    kv(
        c,
        x.max(150) + 8,
        y,
        "month",
        &usage(m.month_rx, m.month_tx),
        p,
        p.fg,
    );
    y += 17;
    let x = kv(
        c,
        4,
        y,
        "wan4",
        m.wan_ip.as_deref().unwrap_or("--"),
        p,
        p.fg,
    );
    let v6 = match m.wan6_up {
        Some(true) => ("up", p.ok),
        Some(false) => ("down", p.warn),
        None => ("--", p.dim),
    };
    let x = c.text(x.max(170) + 8, y, Small, p.dim, "wan6");
    c.text(x + 8, y, Small, v6.1, v6.0);
    y += 17;
    kv(
        c,
        4,
        y,
        "dns",
        &truncate(
            &m.dns
                .iter()
                .filter(|d| !d.contains(':'))
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            34,
        ),
        p,
        p.fg,
    );
    y += 22;
    c.fill_rect(0, y - 1, W, 17, p.fg);
    c.text(4, y, Small, p.bg, "IFACE   STATE     RX/s     TX/s");
    y += 18;
    for i in &m.ifaces {
        let label = match i.label {
            "wifi2g" => format!(
                "wifi2g:{}",
                m.sta_2g.map(|n| n.to_string()).unwrap_or("-".into())
            ),
            "wifi5g" => format!(
                "wifi5g:{}",
                m.sta_5g.map(|n| n.to_string()).unwrap_or("-".into())
            ),
            l => l.to_string(),
        };
        c.text(4, y, Small, p.bright, &label);
        c.text(
            76,
            y,
            Small,
            if i.up { p.ok } else { p.dim },
            if i.up { "up" } else { "down" },
        );
        c.text_right(220, y, Small, p.accent, &bits(i.rx_rate));
        c.text_right(W - 4, y, Small, p.accent2, &bits(i.tx_rate));
        y += 16;
    }
    y += 6;
    let ct = m.conntrack.unwrap_or(0);
    let ct_max = m.conntrack_max.unwrap_or(65536).max(1);
    let x = kv(c, 4, y, "conntrack", &format!("{ct}/{ct_max}"), p, p.fg);
    c.meter(
        x + 8,
        y + 3,
        (W - x - 12).max(10),
        10,
        ct as f32 / ct_max as f32,
        p.fg,
        p.grid,
    );
    y += 17;
    let x = kv(
        c,
        4,
        y,
        "tcp",
        &m.tcp_inuse.map(|v| v.to_string()).unwrap_or("--".into()),
        p,
        p.fg,
    );
    kv(
        c,
        x.max(110) + 8,
        y,
        "leases",
        &m.clients.map(|v| v.to_string()).unwrap_or("--".into()),
        p,
        p.fg,
    );
    y += 17;
    let proxy = match (&m.mihomo, &m.proxy) {
        (Some(_), Some(px)) => format!(
            "{} {}c \u{2193}{} \u{2191}{}",
            px.mode,
            px.connections,
            bytes(px.down_total),
            bytes(px.up_total)
        ),
        (Some(proc_), None) => format!("up {}", kb(proc_.rss_kb)),
        (None, _) => "off".into(),
    };
    kv(
        c,
        4,
        y,
        "mihomo",
        &truncate(&proxy, 31),
        p,
        if m.mihomo.is_some() { p.fg } else { p.dim },
    );
}

fn pwr(c: &mut Canvas, m: &Model, p: &Palette) {
    let b = &m.battery;
    let mut y = TOP + 4;
    let pct = b.percent.unwrap_or(0) as i32;
    let col = match b.percent {
        Some(v) if v <= 10 => p.crit,
        Some(v) if v <= 20 => p.warn,
        Some(_) => p.fg,
        None => p.dim,
    };
    // Big battery: shell, terminal nub, and a cell-segmented fill.
    c.frame(4, y + 4, 112, 56, col);
    c.frame(5, y + 5, 110, 54, col);
    c.fill_rect(116, y + 20, 6, 24, col);
    for i in 0..10 {
        let on = pct >= (i + 1) * 10 - 5;
        c.fill_rect(10 + i * 10, y + 10, 8, 44, if on { col } else { p.grid });
    }
    let label = b.percent.map(|v| format!("{v}%")).unwrap_or("--".into());
    c.text_right(W - 4, y - 2, crate::font::Size::Huge, col, &label);
    y += 70;
    let source = if b.external_power {
        "external power"
    } else {
        "on battery"
    };
    let x = c.text(4, y, Small, p.bright, &b.status.to_lowercase());
    let x = c.text(
        x + 12,
        y,
        Small,
        if b.external_power { p.warn } else { p.dim },
        source,
    );
    let limit = b
        .limit
        .map(|l| format!("limit {l}%"))
        .unwrap_or("no limit".into());
    c.text(x.max(200) + 8, y, Small, p.dim, &limit);
    y += 20;
    let watts = match (b.voltage_mv, b.current_ma) {
        (Some(v), Some(i)) => format!("{:.2}W", (v * i).abs() as f32 / 1e6),
        _ => "--".into(),
    };
    let x = kv(
        c,
        4,
        y,
        "V",
        &b.voltage_mv
            .map(|v| format!("{:.3}", v as f32 / 1000.0))
            .unwrap_or("--".into()),
        p,
        p.fg,
    );
    let x = kv(
        c,
        x + 16,
        y,
        "I",
        &b.current_ma
            .map(|i| format!("{i}mA"))
            .unwrap_or("--".into()),
        p,
        p.accent,
    );
    kv(c, x + 16, y, "P", &watts, p, p.fg);
    y += 17;
    let x = kv(
        c,
        4,
        y,
        "temp",
        &b.temp_c
            .map(|t| format!("{t:.1}\u{b0}"))
            .unwrap_or("--".into()),
        p,
        b.temp_c
            .map(|t| {
                if t >= 45.0 {
                    p.crit
                } else if t >= 40.0 {
                    p.warn
                } else {
                    p.fg
                }
            })
            .unwrap_or(p.dim),
    );
    kv(
        c,
        x.max(150) + 8,
        y,
        "health",
        &b.health.to_lowercase(),
        p,
        if b.health == "Good" { p.ok } else { p.warn },
    );
    y += 17;
    let x = kv(
        c,
        4,
        y,
        "cycles",
        &b.cycles.map(|v| v.to_string()).unwrap_or("--".into()),
        p,
        p.fg,
    );
    kv(
        c,
        x.max(150) + 8,
        y,
        "capacity",
        &b.wear.map(|w| format!("{w:.0}%")).unwrap_or("--".into()),
        p,
        p.fg,
    );
    y += 17;
    let eta = match (b.time_to_full_s, b.time_to_empty_s) {
        (Some(s), _) if b.external_power => format!("full in {}", duration(s)),
        (_, Some(s)) if !b.external_power => format!("empty in {}", duration(s)),
        _ => "--".into(),
    };
    kv(c, 4, y, "eta", &eta, p, p.fg);
    y += 28;
    let (gx, gy, gw, gh) = graph_box(c, y, 48, "current mA 2min", p);
    let lo = m.current_hist.iter().fold(0.0f32, |a, v| a.min(*v));
    let hi = m.current_hist.iter().fold(1.0f32, |a, v| a.max(*v));
    let shifted: Vec<f32> = m.current_hist.iter().map(|v| v - lo).collect();
    c.plot(
        gx,
        gy,
        gw,
        gh,
        &shifted,
        (hi - lo).max(1.0),
        p.accent,
        dim(p.accent, 50),
    );
    y += 58;
    section(c, y, "thermal", p);
    y += 20;
    for (name, t) in m.zones.iter().take(8) {
        let col = temp_col(*t, p);
        c.text(4, y, Small, p.dim, &truncate(name, 10));
        c.meter(92, y + 3, 150, 10, (t - 20.0) / 70.0, col, p.grid);
        c.text_right(W - 4, y, Small, col, &format!("{t:.1}\u{b0}"));
        y += 16;
    }
    let throttle = if m.cooling_active == 0 {
        "none".to_string()
    } else {
        format!("{} active", m.cooling_active)
    };
    kv(
        c,
        4,
        y + 2,
        "throttle",
        &throttle,
        p,
        if m.cooling_active == 0 { p.ok } else { p.warn },
    );
}

fn lan(c: &mut Canvas, m: &Model, p: &Palette) {
    let mut y = TOP;
    section(c, y, "wifi", p);
    y += 20;
    if m.wifi.is_empty() {
        c.text(4, y, Small, p.dim, "reading radios...");
        y += 17;
    }
    for b in &m.wifi {
        let x = c.text(4, y, Small, p.accent, b.label);
        let x = c.text(x.max(44) + 4, y, Small, p.bright, &truncate(&b.ssid, 10));
        let detail = format!("ch{} {} {}", b.channel, b.width, b.txpower);
        c.text(x.max(128) + 4, y, Small, p.dim, &truncate(&detail, 18));
        c.text_right(
            W - 4,
            y,
            Small,
            if b.stations > 0 { p.fg } else { p.dim },
            &format!("{}sta", b.stations),
        );
        y += 17;
    }
    y += 6;
    section(c, y, &format!("clients ({})", m.lan.len()), p);
    y += 20;
    c.fill_rect(0, y - 1, W, 17, p.fg);
    c.text(4, y, Small, p.bg, "HOST          IP   LINK  SIG  RATE");
    y += 19;
    for client in m.lan.iter().take(((TAB_Y - 8 - y) / 17) as usize) {
        let host = if client.host.is_empty() {
            "?"
        } else {
            &client.host
        };
        c.text(4, y, Small, p.bright, &truncate(host, 13));
        let octet = client.ip.rsplit('.').next().unwrap_or("");
        c.text(108, y, Small, p.fg, &format!(".{octet}"));
        match &client.station {
            Some(st) => {
                c.text(148, y, Small, p.accent, st.band);
                if let Some(sig) = st.signal {
                    let col = if sig >= -60 {
                        p.ok
                    } else if sig >= -72 {
                        p.warn
                    } else {
                        p.crit
                    };
                    c.text_right(236, y, Small, col, &sig.to_string());
                }
                if let Some(rate) = st.rate_mbit {
                    c.text_right(W - 4, y, Small, p.fg, &format!("{rate:.0}M"));
                }
            }
            None => {
                c.text(148, y, Small, p.dim, "wire");
            }
        }
        y += 17;
    }
    if m.lan.is_empty() {
        c.text(4, y, Small, p.dim, "(no leases)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Radio;

    fn sample() -> Model {
        let mut m = Model {
            clock: "17:58:31".into(),
            kernel: "5.15.194-perf".into(),
            firmware: "CN_ZTE_MU5250V1.0.0B31".into(),
            cpu: vec![12.0, 3.0, 1.0, 5.0],
            freq_mhz: vec![2208, 1516, 1516, 1516],
            mem_total_kb: 1_628_400,
            mem_avail_kb: 742_000,
            mem_cached_kb: 382_000,
            uptime_s: 15_000,
            temp_cpu: Some(49.0),
            wan_ip: Some("10.0.0.1".into()),
            services: crate::stats::SERVICES
                .iter()
                .map(|(l, _)| (*l, true))
                .collect(),
            radio: Some(Radio {
                rat: "SA".into(),
                band: "n41".into(),
                rsrp: Some(-57.0),
                sinr: Some(36.9),
                bars: Some(5),
                ..Default::default()
            }),
            ..Default::default()
        };
        m.battery.percent = Some(100);
        m.battery.external_power = true;
        m
    }

    #[test]
    fn formats_are_compact() {
        assert_eq!(bits(17290.0), "138k");
        assert_eq!(bits(1_896_530.0), "15.2M");
        assert_eq!(bytes(28_820_212), "27.5M");
        assert_eq!(duration(1375), "22m55s");
        assert_eq!(duration(15_000), "4h10m");
        assert_eq!(duration(90_061), "1d01h01m");
        assert_eq!(truncate("abcdef", 4), "abc~");
    }

    #[test]
    fn healthy_model_raises_no_alerts() {
        assert!(alerts(&sample()).is_empty());
    }

    #[test]
    fn alerts_put_critical_first() {
        let mut m = sample();
        m.temp_cpu = Some(70.0);
        m.services[0].1 = false;
        let a = alerts(&m);
        assert_eq!(a[0].0, Level::Crit);
        assert!(a[0].1.contains("agent"));
        assert_eq!(a[1].0, Level::Warn);
    }

    #[test]
    fn taps_switch_pages_and_exit() {
        let mut ui = Ui::new(Settings::default());
        ui.splash = SPLASH_FRAMES;
        let mut m = sample();
        ui.tap(TAB_W + 5, TAB_Y + 10, &mut m);
        assert_eq!(ui.page, Page::Sys);
        assert!(matches!(ui.tap(W - 5, TAB_Y + 10, &mut m), Action::Exit));
        assert!(matches!(ui.swipe(-80), Action::Redraw));
        assert_eq!(ui.page, Page::Pwr);
    }

    #[test]
    fn taps_are_ignored_during_the_splash() {
        let mut ui = Ui::new(Settings::default());
        assert!(matches!(
            ui.tap(W - 5, TAB_Y + 10, &mut sample()),
            Action::None
        ));
    }

    #[test]
    fn settings_overlay_edits_and_closes() {
        let mut ui = Ui::new(Settings::default());
        ui.splash = SPLASH_FRAMES;
        let mut m = sample();
        ui.tap(W - 10, 10, &mut m);
        assert!(ui.settings_open);
        let row = |i: i32| SETTINGS_Y + 44 + i * SETTINGS_ROW + 10;
        assert!(matches!(
            ui.tap(W - 40, row(0), &mut m),
            Action::SettingsChanged
        ));
        assert_eq!(ui.settings.brightness, 184);
        ui.tap(W / 2, row(3), &mut m);
        assert_eq!(ui.settings.theme, Theme::Amber);
        ui.tap(W / 2, row(6), &mut m);
        assert_eq!(ui.settings.auto_open, crate::settings::AutoOpen::Boot);
        ui.tap(W / 2, row(7), &mut m);
        assert!(!ui.settings_open);
    }

    #[test]
    fn every_page_renders() {
        let mut ui = Ui::new(Settings::default());
        ui.splash = SPLASH_FRAMES;
        let mut c = Canvas::new();
        for page in Page::ALL {
            ui.page = page;
            ui.render(&mut c, &sample());
            assert!(c.px.iter().any(|px| *px != c.px[0]));
        }
        ui.render(&mut c, &Model::default());
    }
}
