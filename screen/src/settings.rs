//! Screen preferences, shared with the agent through one JSON file.
//! The screen writes it when settings change on the device; the agent's
//! `PUT /api/screen/settings` writes the same file and the running screen
//! picks the change up within a couple of seconds.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

pub const PATH: &str = "/data/local/tmp/zte-screen.json";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleAction {
    /// Hand the panel back to the stock UI (default: stock power handling resumes).
    Stock,
    /// Turn the backlight off and stay.
    Sleep,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Green,
    Amber,
    Cyan,
}

impl Theme {
    pub fn name(self) -> &'static str {
        match self {
            Theme::Green => "green",
            Theme::Amber => "amber",
            Theme::Cyan => "cyan",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Theme::Green => Theme::Amber,
            Theme::Amber => Theme::Cyan,
            Theme::Cyan => Theme::Green,
        }
    }
}

/// How sessions start without a tap on the web page or a two-finger hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoOpen {
    Off,
    /// Once per boot, after the stock UI is up.
    Boot,
    /// Whenever the stock UI lights the screen (and once at boot).
    Wake,
}

impl AutoOpen {
    pub fn name(self) -> &'static str {
        match self {
            AutoOpen::Off => "off",
            AutoOpen::Boot => "boot",
            AutoOpen::Wake => "wake",
        }
    }

    pub fn next(self) -> Self {
        match self {
            AutoOpen::Off => AutoOpen::Boot,
            AutoOpen::Boot => AutoOpen::Wake,
            AutoOpen::Wake => AutoOpen::Off,
        }
    }
}

pub const PAGES: [&str; 7] = ["home", "sys", "pwr", "cell", "net", "lan", "proxy"];
pub const IDLE_CHOICES: [u32; 5] = [60, 120, 300, 600, 1800];

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub brightness: u8,
    pub idle_secs: u32,
    pub idle_action: IdleAction,
    pub theme: Theme,
    pub scanlines: bool,
    /// Index into `PAGES`.
    pub start_page: usize,
    /// Read by the agent: open from the stock UI with a two-finger hold.
    pub gesture: bool,
    /// Read by the agent: when to open a session by itself.
    pub auto_open: AutoOpen,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            brightness: 160,
            idle_secs: 120,
            idle_action: IdleAction::Stock,
            theme: Theme::Green,
            scanlines: true,
            start_page: 0,
            gesture: true,
            auto_open: AutoOpen::Off,
        }
    }
}

impl Settings {
    pub fn from_json(v: &Value) -> Self {
        let d = Settings::default();
        let get_u64 = |k: &str| v.get(k).and_then(Value::as_u64);
        let get_bool = |k: &str| v.get(k).and_then(Value::as_bool);
        let get_str = |k: &str| v.get(k).and_then(Value::as_str);
        Settings {
            brightness: get_u64("brightness")
                .map(|b| b.clamp(20, 255) as u8)
                .unwrap_or(d.brightness),
            idle_secs: get_u64("idle_secs")
                .map(|s| s.clamp(30, 3600) as u32)
                .unwrap_or(d.idle_secs),
            idle_action: match get_str("idle_action") {
                Some("sleep") => IdleAction::Sleep,
                Some("stock") => IdleAction::Stock,
                _ => d.idle_action,
            },
            theme: match get_str("theme") {
                Some("amber") => Theme::Amber,
                Some("cyan") => Theme::Cyan,
                Some("green") => Theme::Green,
                _ => d.theme,
            },
            scanlines: get_bool("scanlines").unwrap_or(d.scanlines),
            start_page: get_str("start_page")
                .and_then(|p| PAGES.iter().position(|x| *x == p))
                .unwrap_or(d.start_page),
            gesture: get_bool("gesture").unwrap_or(d.gesture),
            auto_open: match get_str("auto_open") {
                Some("boot") => AutoOpen::Boot,
                Some("wake") => AutoOpen::Wake,
                Some("off") => AutoOpen::Off,
                _ => d.auto_open,
            },
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "brightness": self.brightness,
            "idle_secs": self.idle_secs,
            "idle_action": match self.idle_action { IdleAction::Stock => "stock", IdleAction::Sleep => "sleep" },
            "theme": self.theme.name(),
            "scanlines": self.scanlines,
            "start_page": PAGES[self.start_page.min(PAGES.len() - 1)],
            "gesture": self.gesture,
            "auto_open": self.auto_open.name(),
        })
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|v| Self::from_json(&v))
            .unwrap_or_default()
    }

    /// Write through a temporary file and rename, so a reader never sees half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(&self.to_json()).unwrap_or_default(),
        )?;
        fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let s = Settings {
            brightness: 90,
            idle_secs: 300,
            idle_action: IdleAction::Sleep,
            theme: Theme::Amber,
            scanlines: false,
            start_page: 2,
            gesture: false,
            auto_open: AutoOpen::Wake,
        };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        let clamped =
            Settings::from_json(&json!({"brightness": 1, "idle_secs": 5, "theme": "pink"}));
        assert_eq!(clamped.brightness, 20);
        assert_eq!(clamped.idle_secs, 30);
        assert_eq!(clamped.theme, Theme::Green);
    }

    #[test]
    fn missing_file_gives_defaults() {
        assert_eq!(
            Settings::load(Path::new("/nonexistent/zte-screen.json")),
            Settings::default()
        );
    }
}
