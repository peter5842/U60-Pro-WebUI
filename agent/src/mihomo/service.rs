//! mihomo process lifecycle, runtime firewall rules for the TUN, and cleanup.
//!
//! Nothing here is persisted outside /data/mihomo: the process is started by
//! the agent (itself started from rc.local), and the iptables accepts and the
//! TUN's ip rules exist only while mihomo runs. A reboot always returns the
//! router to its stock forwarding path.

use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::process::BoundedCommand;

use super::config::{LAN_BRIDGE, TUN_DEVICE};

pub const DIR: &str = "/data/mihomo";
pub const BINARY: &str = "/data/mihomo/mihomo";
pub const CONFIG: &str = "/data/mihomo/config.yaml";
pub const STAGED: &str = "/data/mihomo/config.staged.yaml";
pub const LOG: &str = "/tmp/mihomo.log";
const PIDFILE: &str = "/var/run/mihomo.pid";
const LOG_CAP: u64 = 1024 * 1024;
const FW_COMMENT: &str = "mihomo-tun";
/// sing-tun's default ip rule priority range and route table.
const RULE_PREFS: std::ops::RangeInclusive<u32> = 9000..=9099;
const ROUTE_TABLE: &str = "2022";

pub struct Process {
    child: Option<Child>,
    pub pid: u32,
    pub started: Instant,
}

pub fn installed() -> bool {
    Path::new(BINARY).is_file()
}

/// `Mihomo Meta v1.19.32 linux arm64 …` → `v1.19.32`.
pub fn version() -> Option<String> {
    let out = Command::new(BINARY).arg("-v").bounded_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .find(|w| w.starts_with('v') && w.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

/// `mihomo -t`: parses the config and loads rule data without starting
/// listeners or fetching subscriptions.
pub fn test_config(path: &str) -> Result<(), String> {
    let out = Command::new(BINARY)
        .args(["-t", "-d", DIR, "-f", path])
        .bounded_output()
        .map_err(|e| format!("could not run mihomo -t: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Err(last_error_line(&text, true).unwrap_or_else(|| "mihomo rejected the configuration".into()))
}

/// The most useful line of mihomo output: the last `level=error`/`fatal`
/// message, else (when `any_line`) the last non-empty line.
pub fn last_error_line(text: &str, any_line: bool) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let pick = lines
        .iter()
        .rev()
        .find(|l| l.contains("level=error") || l.contains("level=fatal"))
        .or_else(|| if any_line { lines.last() } else { None })?;
    let msg = pick
        .split_once("msg=")
        .map(|(_, m)| m.trim_matches('"').replace("\\\"", "\""))
        .unwrap_or_else(|| pick.to_string());
    Some(msg.chars().take(300).collect())
}

pub fn log_tail_error() -> Option<String> {
    let text = fs::read_to_string(LOG).ok()?;
    let start = text.len().saturating_sub(16 * 1024);
    let tail = text.get(start..).unwrap_or(&text);
    last_error_line(tail, false)
}

pub fn spawn() -> Result<Process, String> {
    if !installed() {
        return Err(format!("mihomo is not installed (expected {BINARY})"));
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG)
        .map_err(|e| format!("cannot open {LOG}: {e}"))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("cannot open {LOG}: {e}"))?;
    let mut cmd = Command::new(BINARY);
    cmd.args(["-d", DIR, "-f", CONFIG])
        .env("GOMEMLIMIT", "160MiB")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    // Own session: an agent restart (deploy, crash) must not take mihomo with it.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start mihomo: {e}"))?;
    let pid = child.id();
    let _ = fs::write(PIDFILE, format!("{pid}\n"));
    Ok(Process {
        child: Some(child),
        pid,
        started: Instant::now(),
    })
}

/// Re-attach to a mihomo started by a previous agent instance.
pub fn adopt() -> Option<Process> {
    let pid: u32 = fs::read_to_string(PIDFILE).ok()?.trim().parse().ok()?;
    if !pid_is_mihomo(pid) {
        let _ = fs::remove_file(PIDFILE);
        return None;
    }
    let started = process_age(pid)
        .and_then(|age| Instant::now().checked_sub(age))
        .unwrap_or_else(Instant::now);
    Some(Process {
        child: None,
        pid,
        started,
    })
}

fn pid_is_mihomo(pid: u32) -> bool {
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let first = cmdline.split(|&b| b == 0).next().unwrap_or(&[]);
    if first != BINARY.as_bytes() {
        return false;
    }
    // A zombie still has a cmdline-less /proc entry; treat it as dead.
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let state = stat
        .rsplit(')')
        .next()
        .and_then(|s| s.split_whitespace().next());
    !matches!(state, Some("Z") | Some("X") | None)
}

fn process_age(pid: u32) -> Option<Duration> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit(')').next()?;
    // Field 22 overall is starttime; after ") " the first field is #3.
    let start_ticks: f64 = after.split_whitespace().nth(19)?.parse().ok()?;
    let uptime: f64 = fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if hz <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(
        (uptime - start_ticks / hz).max(0.0),
    ))
}

impl Process {
    pub fn alive(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            if !matches!(child.try_wait(), Ok(None)) {
                return false;
            }
        }
        pid_is_mihomo(self.pid)
    }

    pub fn rss_bytes(&self) -> Option<u64> {
        let status = fs::read_to_string(format!("/proc/{}/status", self.pid)).ok()?;
        let kb: u64 = status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        Some(kb * 1024)
    }

    /// SIGTERM, wait up to 5 s, then SIGKILL. Returns true when it had to
    /// force-kill (mihomo then cannot remove its own ip rules).
    pub fn terminate(mut self) -> bool {
        let pid = self.pid as libc::pid_t;
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !self.alive() {
                let _ = fs::remove_file(PIDFILE);
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        unsafe { libc::kill(pid, libc::SIGKILL) };
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
        let _ = fs::remove_file(PIDFILE);
        true
    }
}

// ── Runtime firewall for the TUN ─────────────────────────────────────────────
//
// ZTE's fw3 only forwards lan→wan(rmnet) and only accepts INPUT from lo/br-lan,
// so LAN traffic routed into the TUN is rejected without these. They are
// inserted at the top of the chains, tagged, and never written to config.

fn fw_rules() -> [Vec<&'static str>; 3] {
    [
        vec!["FORWARD", "-i", LAN_BRIDGE, "-o", TUN_DEVICE],
        vec!["FORWARD", "-i", TUN_DEVICE, "-o", LAN_BRIDGE],
        vec!["INPUT", "-i", TUN_DEVICE],
    ]
}

fn fw(bin: &str, op: &str, rule: &[&str]) -> bool {
    let mut args: Vec<&str> = vec![op, rule[0]];
    if op == "-I" {
        args.push("1");
    }
    args.extend_from_slice(&rule[1..]);
    args.extend_from_slice(&["-m", "comment", "--comment", FW_COMMENT, "-j", "ACCEPT"]);
    Command::new(bin)
        .args(&args)
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn firewall_present() -> bool {
    ["iptables", "ip6tables"]
        .iter()
        .all(|bin| fw_rules().iter().all(|rule| fw(bin, "-C", rule)))
}

pub fn firewall_add() -> Result<(), String> {
    for bin in ["iptables", "ip6tables"] {
        for rule in fw_rules() {
            if !fw(bin, "-C", &rule) && !fw(bin, "-I", &rule) {
                return Err(format!(
                    "{bin}: could not insert {} accept for the TUN",
                    rule[0]
                ));
            }
        }
    }
    Ok(())
}

pub fn firewall_remove() {
    bypass_remove();
    for bin in ["iptables", "ip6tables"] {
        for rule in fw_rules() {
            // Remove every copy, bounded in case -D keeps "succeeding".
            for _ in 0..8 {
                if !fw(bin, "-D", &rule) {
                    break;
                }
            }
        }
    }
}

/// Remove ip rules, routes and the device a force-killed TUN left behind.
pub fn cleanup_routing() {
    for family in ["-4", "-6"] {
        let listing = Command::new("ip")
            .args([family, "rule", "show"])
            .bounded_output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let mut prefs: Vec<u32> = listing
            .lines()
            .filter_map(|l| l.split(':').next()?.trim().parse().ok())
            .filter(|p| RULE_PREFS.contains(p))
            .collect();
        prefs.sort_unstable();
        prefs.dedup();
        for pref in prefs {
            let pref = pref.to_string();
            // Several rules may share a priority; delete until none is left.
            for _ in 0..8 {
                let ok = Command::new("ip")
                    .args([family, "rule", "del", "pref", &pref])
                    .bounded_output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if !ok {
                    break;
                }
            }
        }
        let _ = Command::new("ip")
            .args([family, "route", "flush", "table", ROUTE_TABLE])
            .bounded_output();
    }
    if tun_active() {
        let _ = Command::new("ip")
            .args(["link", "del", TUN_DEVICE])
            .bounded_output();
    }
}

// ── Mainland bypass for the TUN ──────────────────────────────────────────────
//
// TUN costs about one CPU core per 100 Mbit/s here (measured 2026-10-06), and
// mainland destinations go DIRECT anyway. LAN packets to mainland IPv4 ranges
// (an ipset loaded from CN_LIST) get a mark, and one ip rule ahead of
// sing-tun's (8999 < 9000) sends marked packets through the main table, so
// they keep the stock path and IPA offload. Runtime only, like the accepts.

pub const CN_LIST: &str = "/data/mihomo/cn.list";
const CN_SET: &str = "mihomo-cn";
const BYPASS_MARK: &str = "0x10000000/0x10000000";
const BYPASS_PREF: &str = "8999";

pub fn bypass_available() -> bool {
    Path::new(CN_LIST).exists()
}

/// IPv4 prefixes from the list (one CIDR per line; IPv6 and junk skipped).
pub fn cn_prefixes(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            let Some((addr, len)) = l.split_once('/') else {
                return false;
            };
            addr.parse::<std::net::Ipv4Addr>().is_ok()
                && len.parse::<u8>().is_ok_and(|n| (1..=32).contains(&n))
        })
        .collect()
}

fn bypass_mark_rule(op: &str) -> bool {
    let mut args = vec!["-t", "mangle", op, "PREROUTING"];
    if op == "-I" {
        args.push("1");
    }
    args.extend_from_slice(&[
        "-i",
        LAN_BRIDGE,
        "-m",
        "set",
        "--match-set",
        CN_SET,
        "dst",
        "-m",
        "comment",
        "--comment",
        FW_COMMENT,
        "-j",
        "MARK",
        "--set-xmark",
        BYPASS_MARK,
    ]);
    Command::new("iptables")
        .args(&args)
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn bypass_rule_present() -> bool {
    Command::new("ip")
        .args(["-4", "rule", "show"])
        .bounded_output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.starts_with(&format!("{BYPASS_PREF}:")))
        })
        .unwrap_or(false)
}

fn set_present() -> bool {
    Command::new("ipset")
        .args(["list", "-n", CN_SET])
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn bypass_present() -> bool {
    set_present() && bypass_mark_rule("-C") && bypass_rule_present()
}

/// Load the set (only when missing: a reload costs ~6000 entries) and insert
/// the mark rule and the ip rule. Returns the number of prefixes loaded.
pub fn bypass_add() -> Result<usize, String> {
    let mut loaded = 0;
    if !set_present() {
        let text = fs::read_to_string(CN_LIST).map_err(|e| format!("{CN_LIST}: {e}"))?;
        let prefixes = cn_prefixes(&text);
        if prefixes.len() < 1000 {
            return Err(format!(
                "{CN_LIST} holds only {} IPv4 prefixes; refusing a partial list",
                prefixes.len()
            ));
        }
        let mut script = format!(
            "create {CN_SET} hash:net family inet maxelem {} -exist\nflush {CN_SET}\n",
            (prefixes.len() * 2).max(16384)
        );
        for p in &prefixes {
            script.push_str(&format!("add {CN_SET} {p} -exist\n"));
        }
        let out = process_runner::output(
            Command::new("ipset").arg("restore"),
            Some(script.as_bytes()),
            Duration::from_secs(20),
            64 * 1024,
        )
        .map_err(|e| format!("ipset restore: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ipset restore: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        loaded = prefixes.len();
    }
    if !bypass_mark_rule("-C") && !bypass_mark_rule("-I") {
        return Err("iptables: could not insert the mainland mark rule".into());
    }
    if !bypass_rule_present() {
        let ok = Command::new("ip")
            .args([
                "-4",
                "rule",
                "add",
                "pref",
                BYPASS_PREF,
                "fwmark",
                BYPASS_MARK,
                "lookup",
                "main",
            ])
            .bounded_output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return Err("ip rule: could not add the mainland bypass rule".into());
        }
    }
    Ok(loaded)
}

pub fn bypass_remove() {
    for _ in 0..8 {
        if !bypass_mark_rule("-D") {
            break;
        }
    }
    for _ in 0..8 {
        let ok = Command::new("ip")
            .args(["-4", "rule", "del", "pref", BYPASS_PREF])
            .bounded_output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            break;
        }
    }
    // Only possible once no rule references the set.
    let _ = Command::new("ipset")
        .args(["destroy", CN_SET])
        .bounded_output();
}

pub fn tun_active() -> bool {
    Path::new("/sys/class/net").join(TUN_DEVICE).exists()
}

pub fn truncate_log_if_large() {
    if fs::metadata(LOG)
        .map(|m| m.len() > LOG_CAP)
        .unwrap_or(false)
    {
        if let Ok(file) = OpenOptions::new().write(true).open(LOG) {
            let _ = file.set_len(0);
        }
    }
}

/// Mainland endpoints that answer 204; rule presets and typical provider
/// configs send them DIRECT, so a failure through mihomo means mihomo itself
/// stopped forwarding rather than a node being down.
const PROBE_URLS: &[&str] = &[
    "http://connectivitycheck.platform.hicloud.com/generate_204",
    "http://connect.rom.miui.com/generate_204",
];

/// One HTTP 204 check, through `proxy` (e.g. `http://192.168.0.1:7890`) or direct.
pub fn probe_204(proxy: Option<&str>) -> bool {
    PROBE_URLS.iter().any(|url| {
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.args([
            "--silent",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            "--max-time",
            "5",
        ]);
        if let Some(p) = proxy {
            cmd.args(["--proxy", p]);
        } else {
            cmd.arg("--noproxy").arg("*");
        }
        cmd.arg(url);
        process_runner::output(&mut cmd, None, Duration::from_secs(8), 1024)
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "204")
            .unwrap_or(false)
    })
}

/// Endpoints outside the mainland that answer 204 over HTTPS. Through the
/// mixed port they follow the proxy rules, so a failure means the selected
/// node path is broken (not mihomo itself).
const ROUTE_PROBE_URLS: &[&str] = &[
    "https://www.gstatic.com/generate_204",
    "https://cp.cloudflare.com/generate_204",
];

/// One HTTPS 204 check through the proxy's own routing (mixed port).
pub fn probe_route(proxy: &str) -> bool {
    ROUTE_PROBE_URLS.iter().any(|url| {
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.args([
            "--silent",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            "--max-time",
            "8",
            "--proxy",
            proxy,
            url,
        ]);
        process_runner::output(&mut cmd, None, Duration::from_secs(11), 1024)
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "204")
            .unwrap_or(false)
    })
}

/// True once `ip` is assigned locally, i.e. mihomo can bind its listener.
pub fn address_ready(ip: &str) -> bool {
    std::net::TcpListener::bind((ip, 0)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_list_keeps_ipv4_prefixes_only() {
        let text =
            "1.0.1.0/24\n 1.0.2.0/23 \n2400:3200::/32\n# comment\n10.0.0.0/33\nbad\n0.0.0.0/0\n";
        assert_eq!(cn_prefixes(text), ["1.0.1.0/24", "1.0.2.0/23"]);
    }

    #[test]
    fn picks_last_error_message() {
        let log = "time=\"t\" level=info msg=\"Start\"\n\
                   time=\"t\" level=error msg=\"initial proxy provider sub-x error: Get \\\"https://h/x\\\": EOF\"\n\
                   time=\"t\" level=info msg=\"done\"\n";
        assert_eq!(
            last_error_line(log, false).unwrap(),
            "initial proxy provider sub-x error: Get \"https://h/x\": EOF"
        );
        assert_eq!(
            last_error_line("plain failure\n\n", true).unwrap(),
            "plain failure"
        );
        assert!(last_error_line("level=warning msg=\"x\"\n", false).is_none());
        assert!(last_error_line("\n \n", true).is_none());
    }

    #[test]
    fn firewall_rules_cover_both_directions_and_input() {
        let rules = fw_rules();
        assert_eq!(rules[0], vec!["FORWARD", "-i", "br-lan", "-o", "mihomo"]);
        assert_eq!(rules[1], vec!["FORWARD", "-i", "mihomo", "-o", "br-lan"]);
        assert_eq!(rules[2], vec!["INPUT", "-i", "mihomo"]);
    }
}
