//! zte-screen: a terminal-style status screen for the U60 Pro's 320x480 LCD.
//!
//! `--supervise` parks the stock UI, runs the panel and always restores the
//! stock UI afterwards. See docs/SCREEN.md for the safety model.
// The hardware modules are Linux-only; on other hosts only --preview is built.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod canvas;
mod cjk;
#[cfg(target_os = "linux")]
mod drm;
mod font;
mod font_data;
mod input;
#[cfg(target_os = "linux")]
mod panel;
mod pet;
mod png;
mod preview;
mod proxy;
mod settings;
mod stats;
#[cfg(target_os = "linux")]
mod supervisor;
mod ui;

const USAGE: &str =
    "usage: zte-screen --supervise [SECONDS] | --stop | --status | --preview DIR | --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let code = match arg(0) {
        Some("--version") => {
            println!("zte-screen {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--preview") => match arg(1) {
            Some(dir) => preview::write_all(std::path::Path::new(dir)),
            None => {
                eprintln!("{USAGE}");
                2
            }
        },
        #[cfg(target_os = "linux")]
        Some("--supervise") => match arg(1).map(str::parse::<u64>) {
            None => supervisor::supervise(None),
            Some(Ok(s)) if (3..=3600).contains(&s) => supervisor::supervise(Some(s)),
            Some(_) => {
                eprintln!("{USAGE}");
                2
            }
        },
        #[cfg(target_os = "linux")]
        Some("--panel") => panel::run(arg(1).and_then(|s| s.parse().ok())),
        #[cfg(target_os = "linux")]
        Some("--stop") | Some("--restore") => supervisor::stop(),
        #[cfg(target_os = "linux")]
        Some("--status") => {
            println!("{}", supervisor::status());
            0
        }
        #[cfg(target_os = "linux")]
        Some("--preflight") => match supervisor::preflight() {
            Ok(()) => {
                println!("ok");
                0
            }
            Err(reason) => {
                println!("{reason}");
                3
            }
        },
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
