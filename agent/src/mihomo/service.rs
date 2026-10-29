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
/// Open-file limit for mihomo (kernel nr_open allows up to 1M).
const NOFILE_LIMIT: libc::rlim_t = 65536;
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
        .env("GOMEMLIMIT", "320MiB")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    // Own session: an agent restart (deploy, crash) must not take mihomo with it.
    // The inherited 1024/4096 fd limit is far too low for a TUN gateway: every
    // proxied UDP flow holds a socket, and one P2P client exhausted it, after
    // which mihomo failed every new connection ("too many open files").
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            let limit = libc::rlimit {
                rlim_cur: NOFILE_LIMIT,
                rlim_max: NOFILE_LIMIT,
            };
            libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
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

/// The accepts, plus the IPv6 NOTRACK exactly while sing-tun routes IPv6.
pub fn firewall_present() -> bool {
    let accepts = ["iptables", "ip6tables"]
        .iter()
        .all(|bin| fw_rules().iter().all(|rule| fw(bin, "-C", rule)));
    accepts
        && if tun_routes_v6() {
            notrack_present()
        } else {
            !notrack_jumps().iter().any(|rule| raw6("-C", rule))
        }
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
    if tun_routes_v6() {
        notrack_add()
    } else {
        notrack_remove();
        Ok(())
    }
}

pub fn firewall_remove() {
    bypass_remove();
    notrack_remove();
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

// ── IPv6 kept away from IPA offload ──────────────────────────────────────────
//
// ipacm offloads established IPv6 flows to IPA by their conntrack tuple
// (IPv6CTEnabled in /etc/data/ipa/IPACM_cfg.xml). A LAN flow routed into the
// TUN looks like a forwarded one, so about 250 ms in the client's packets start
// going straight to the modem, mihomo never sees its ACKs, and the transfer
// stalls after the first window (measured 2026-10-29: 40 MB stalled at 32 KB;
// untracked it ran at the Wi-Fi's ~60 Mbit/s). IPv4 is safe: ipacm offloads
// only masqueraded IPv4 flows. TProxy and REDIRECT were no way out (br_netfilter
// drops TProxy's socket; REDIRECT needs conntrack and was offloaded too).
//
// Packets the TUN carries for global IPv6 destinations are therefore left
// untracked; the accepts above are stateless, so they still pass. Without the
// TUN routing IPv6 these rules would leave WAN replies NEW for fw3 to reject,
// so they exist only while sing-tun's IPv6 rules do. Mainland IPv6 that the
// bypass sends past the TUN returns early and stays tracked and offloaded.

const NOTRACK_CHAIN: &str = "mihomo_notrack";
const GLOBAL_V6: &str = "2000::/3";

/// sing-tun routes LAN IPv6 into the TUN: its IPv6 rules use table 2022.
fn tun_routes_v6() -> bool {
    Command::new("ip")
        .args(["-6", "rule", "show"])
        .bounded_output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).lines().any(|l| {
                let words: Vec<&str> = l.split_whitespace().collect();
                words.windows(2).any(|w| w == ["lookup", ROUTE_TABLE])
            })
        })
        .unwrap_or(false)
}

/// LAN packets jump to the chain (where the bypass can return early); TUN
/// output to LAN clients is untracked directly.
fn notrack_jumps() -> [Vec<&'static str>; 2] {
    [
        vec![
            "PREROUTING",
            "-i",
            LAN_BRIDGE,
            "-d",
            GLOBAL_V6,
            "-m",
            "comment",
            "--comment",
            FW_COMMENT,
            "-j",
            NOTRACK_CHAIN,
        ],
        vec![
            "PREROUTING",
            "-i",
            TUN_DEVICE,
            "-d",
            GLOBAL_V6,
            "-m",
            "comment",
            "--comment",
            FW_COMMENT,
            "-j",
            "CT",
            "--notrack",
        ],
    ]
}

fn notrack_rule() -> Vec<&'static str> {
    vec![NOTRACK_CHAIN, "-j", "CT", "--notrack"]
}

fn cn6_return_rule() -> Vec<&'static str> {
    vec![
        NOTRACK_CHAIN,
        "-m",
        "set",
        "--match-set",
        CN6_SET,
        "dst",
        "-j",
        "RETURN",
    ]
}

/// `ip6tables -t raw <op> <chain> [1] <rest>`; `-I` inserts at the top.
fn raw6(op: &str, rule: &[&str]) -> bool {
    let mut args: Vec<&str> = vec!["-t", "raw", op, rule[0]];
    if op == "-I" {
        args.push("1");
    }
    args.extend_from_slice(&rule[1..]);
    Command::new("ip6tables")
        .args(&args)
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn notrack_chain_exists() -> bool {
    Command::new("ip6tables")
        .args(["-t", "raw", "-S", NOTRACK_CHAIN])
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn notrack_present() -> bool {
    raw6("-C", &notrack_rule())
        && cn6_bypassing() == raw6("-C", &cn6_return_rule())
        && notrack_jumps().iter().all(|rule| raw6("-C", rule))
}

fn notrack_add() -> Result<(), String> {
    if !notrack_chain_exists()
        && !Command::new("ip6tables")
            .args(["-t", "raw", "-N", NOTRACK_CHAIN])
            .bounded_output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    {
        return Err(format!(
            "ip6tables: could not create the {NOTRACK_CHAIN} chain"
        ));
    }
    // While mainland IPv6 bypasses the TUN its early return must precede the
    // notrack (rebuilt here after a flush); otherwise it must not be there.
    if cn6_bypassing() {
        if !raw6("-C", &cn6_return_rule()) && !raw6("-I", &cn6_return_rule()) {
            return Err("ip6tables: could not insert the mainland IPv6 return".into());
        }
    } else {
        for _ in 0..8 {
            if !raw6("-D", &cn6_return_rule()) {
                break;
            }
        }
    }
    if !raw6("-C", &notrack_rule()) && !raw6("-A", &notrack_rule()) {
        return Err("ip6tables: could not add the IPv6 notrack rule".into());
    }
    for rule in notrack_jumps() {
        if !raw6("-C", &rule) && !raw6("-I", &rule) {
            return Err("ip6tables: could not insert the IPv6 notrack jump".into());
        }
    }
    Ok(())
}

fn notrack_remove() {
    for rule in notrack_jumps() {
        for _ in 0..8 {
            if !raw6("-D", &rule) {
                break;
            }
        }
    }
    for op in ["-F", "-X"] {
        let _ = Command::new("ip6tables")
            .args(["-t", "raw", op, NOTRACK_CHAIN])
            .bounded_output();
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
// mainland destinations go DIRECT anyway. LAN packets to mainland ranges
// (ipsets loaded from CN_LIST, one per family) get a mark, and one ip rule per
// family ahead of sing-tun's (8999 < 9000) sends marked packets through the
// main table, so they keep the stock path and IPA offload. Mainland IPv6 also
// returns before the IPv6 notrack, so it stays tracked for that offload.
// Runtime only, like the accepts.

pub const CN_LIST: &str = "/data/mihomo/cn.list";
const CN_SET: &str = "mihomo-cn";
const CN6_SET: &str = "mihomo-cn6";
const BYPASS_MARK: &str = "0x10000000/0x10000000";
const BYPASS_PREF: &str = "8999";
/// A list with fewer prefixes of a family is taken as truncated.
const MIN_PREFIXES: usize = 1000;

/// One address family of the bypass.
struct Family {
    name: &'static str,
    iptables: &'static str,
    ip: &'static str,
    set: &'static str,
    inet: &'static str,
    prefixes: fn(&str) -> Vec<&str>,
}

const FAMILIES: [Family; 2] = [
    Family {
        name: "IPv4",
        iptables: "iptables",
        ip: "-4",
        set: CN_SET,
        inet: "inet",
        prefixes: cn_prefixes,
    },
    Family {
        name: "IPv6",
        iptables: "ip6tables",
        ip: "-6",
        set: CN6_SET,
        inet: "inet6",
        prefixes: cn6_prefixes,
    },
];
const V6: &Family = &FAMILIES[1];

pub fn bypass_available() -> bool {
    Path::new(CN_LIST).exists()
}

/// IPv4 prefixes from the list (one CIDR per line; IPv6 and junk skipped).
pub fn cn_prefixes(text: &str) -> Vec<&str> {
    prefixes_of::<std::net::Ipv4Addr>(text, 32)
}

/// IPv6 prefixes from the list (IPv4 and junk skipped).
pub fn cn6_prefixes(text: &str) -> Vec<&str> {
    prefixes_of::<std::net::Ipv6Addr>(text, 128)
}

fn prefixes_of<A: std::str::FromStr>(text: &str, max_len: u8) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            let Some((addr, len)) = l.split_once('/') else {
                return false;
            };
            addr.parse::<A>().is_ok() && len.parse::<u8>().is_ok_and(|n| (1..=max_len).contains(&n))
        })
        .collect()
}

fn bypass_mark_rule(f: &Family, op: &str) -> bool {
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
        f.set,
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
    Command::new(f.iptables)
        .args(&args)
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn bypass_rule_present(f: &Family) -> bool {
    Command::new("ip")
        .args([f.ip, "rule", "show"])
        .bounded_output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.starts_with(&format!("{BYPASS_PREF}:")))
        })
        .unwrap_or(false)
}

fn set_present(set: &str) -> bool {
    Command::new("ipset")
        .args(["list", "-n", set])
        .bounded_output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Mainland IPv6 is being sent past the TUN, so it must skip the notrack.
fn cn6_bypassing() -> bool {
    bypass_mark_rule(V6, "-C")
}

pub fn bypass_present() -> bool {
    FAMILIES
        .iter()
        .all(|f| set_present(f.set) && bypass_mark_rule(f, "-C") && bypass_rule_present(f))
        && (!notrack_chain_exists() || raw6("-C", &cn6_return_rule()))
}

fn load_set(f: &Family, prefixes: &[&str]) -> Result<usize, String> {
    if prefixes.len() < MIN_PREFIXES {
        return Err(format!(
            "{CN_LIST} holds only {} {} prefixes; refusing a partial list",
            prefixes.len(),
            f.name
        ));
    }
    let set = f.set;
    let mut script = format!(
        "create {set} hash:net family {} maxelem {} -exist\nflush {set}\n",
        f.inet,
        (prefixes.len() * 2).max(16384)
    );
    for p in prefixes {
        script.push_str(&format!("add {set} {p} -exist\n"));
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
    Ok(prefixes.len())
}

/// Load the sets (only when missing: a reload costs ~10000 entries), then the
/// IPv6 notrack return, then the mark rules and ip rules, so mainland IPv6 is
/// never sent past the TUN untracked. Returns the number of prefixes loaded.
pub fn bypass_add() -> Result<usize, String> {
    let mut loaded = 0;
    let missing: Vec<&Family> = FAMILIES.iter().filter(|f| !set_present(f.set)).collect();
    if !missing.is_empty() {
        let text = fs::read_to_string(CN_LIST).map_err(|e| format!("{CN_LIST}: {e}"))?;
        for f in missing {
            loaded += load_set(f, &(f.prefixes)(&text))?;
        }
    }
    if notrack_chain_exists() && !raw6("-C", &cn6_return_rule()) && !raw6("-I", &cn6_return_rule())
    {
        return Err("ip6tables: could not insert the mainland IPv6 return".into());
    }
    for f in &FAMILIES {
        if !bypass_mark_rule(f, "-C") && !bypass_mark_rule(f, "-I") {
            return Err(format!(
                "{}: could not insert the mainland mark rule",
                f.iptables
            ));
        }
        if !bypass_rule_present(f) {
            let ok = Command::new("ip")
                .args([
                    f.ip,
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
                return Err(format!(
                    "ip rule: could not add the {} mainland bypass rule",
                    f.name
                ));
            }
        }
    }
    Ok(loaded)
}

/// Marks and ip rules go first, so mainland IPv6 is back in the TUN before
/// its notrack return is removed.
pub fn bypass_remove() {
    for f in &FAMILIES {
        for _ in 0..8 {
            if !bypass_mark_rule(f, "-D") {
                break;
            }
        }
        for _ in 0..8 {
            let ok = Command::new("ip")
                .args([f.ip, "rule", "del", "pref", BYPASS_PREF])
                .bounded_output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                break;
            }
        }
    }
    for _ in 0..8 {
        if !raw6("-D", &cn6_return_rule()) {
            break;
        }
    }
    // Only possible once no rule references the set.
    for f in &FAMILIES {
        let _ = Command::new("ipset")
            .args(["destroy", f.set])
            .bounded_output();
    }
}

/// Does mihomo's LAN controller at `addr` accept `secret`? `None` when it
/// cannot be asked. A hot reload does not replace the controller's secret, so
/// this catches a running mihomo still using an older one.
pub fn controller_accepts(addr: &str, secret: &str) -> Option<bool> {
    use std::io::{Read, Write};
    let sock: std::net::SocketAddr = addr.parse().ok()?;
    let mut stream = std::net::TcpStream::connect_timeout(&sock, Duration::from_secs(2)).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let request = format!(
        "GET /version HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {secret}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut head = [0u8; 64];
    let n = stream.read(&mut head).ok()?;
    let status = String::from_utf8_lossy(&head[..n])
        .split_whitespace()
        .nth(1)?
        .to_string();
    Some(status == "200")
}

/// zashboard is installed (scripts/deploy-mihomo.sh) and served at `/ui`.
pub fn ui_installed() -> bool {
    Path::new(super::config::UI_DIR).join("index.html").exists()
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
    fn cn_list_ipv6_prefixes_skip_ipv4_and_junk() {
        let text = "1.0.1.0/24\n2400:3200::/32\n 2001:250::/30 \n::/0\n2001:db8::/129\nfe80::1\n";
        assert_eq!(cn6_prefixes(text), ["2400:3200::/32", "2001:250::/30"]);
    }

    #[test]
    fn ipv6_notrack_covers_lan_ingress_and_tun_output_only() {
        let [lan, tun] = notrack_jumps();
        assert_eq!(&lan[..5], ["PREROUTING", "-i", "br-lan", "-d", "2000::/3"]);
        assert_eq!(lan[lan.len() - 2..], ["-j", NOTRACK_CHAIN]);
        assert_eq!(&tun[..5], ["PREROUTING", "-i", "mihomo", "-d", "2000::/3"]);
        assert_eq!(tun[tun.len() - 3..], ["-j", "CT", "--notrack"]);
        // The mainland return is matched inside the chain, ahead of the notrack.
        assert_eq!(cn6_return_rule()[0], NOTRACK_CHAIN);
        assert!(cn6_return_rule().contains(&CN6_SET));
        assert_eq!(notrack_rule(), [NOTRACK_CHAIN, "-j", "CT", "--notrack"]);
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
