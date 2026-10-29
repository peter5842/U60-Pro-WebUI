//! Read-only collectors. Everything here reads procfs/sysfs, calls a `get`
//! ubus method through the bounded runner, or asks mihomo's local controller
//! socket a GET; nothing changes device state. (The PROXY page's controls
//! live in `proxy/jobs.rs`.)

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

pub const HISTORY: usize = 60;
const CHARGE_LIMIT: &str = "/data/local/tmp/charge_limit.json";
/// Interfaces shown on the NET page, in order, with short labels.
const IFACES: &[(&str, &str)] = &[
    ("rmnet_data0", "wan"),
    ("br-lan", "lan"),
    ("wlan0", "wifi2g"),
    ("wlan2", "wifi5g"),
    ("ecm0", "usb"),
    ("mihomo", "tun"),
];
/// Services on the HOME health row: label and process name (`comm`).
pub const SERVICES: &[(&str, &str)] = &[
    ("agent", "zte-agent"),
    ("web", "dashboard-uhttp"),
    ("ssh", "dropbear"),
    ("proxy", "mihomo"),
    ("dns", "dnsmasq"),
    ("wifi", "hostapd"),
    ("data", "zte_topsw_data"),
];

#[derive(Clone, Default, Debug)]
pub struct Radio {
    /// `network_type` as the firmware reports it: SA, NSA, LTE, …
    pub rat: String,
    pub band: String,
    pub bandwidth: String,
    pub pci: Option<i64>,
    pub arfcn: Option<i64>,
    pub operator: String,
    pub roaming: bool,
    pub rsrp: Option<f32>,
    pub rsrq: Option<f32>,
    pub sinr: Option<f32>,
    pub rssi: Option<f32>,
    pub bars: Option<u8>,
    /// Carrier aggregation summary, empty without CA.
    pub ca: String,
    pub mode: String,
}

#[derive(Clone, Default, Debug)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub rss_kb: u64,
}

#[derive(Clone, Default, Debug)]
pub struct Iface {
    pub label: &'static str,
    pub up: bool,
    pub rx_rate: f64,
    pub tx_rate: f64,
}

#[derive(Clone, Default, Debug)]
pub struct Station {
    pub mac: String,
    pub band: &'static str,
    pub signal: Option<i32>,
    pub rate_mbit: Option<f32>,
    pub connected_s: Option<u64>,
}

#[derive(Clone, Default, Debug)]
pub struct Client {
    pub host: String,
    pub ip: String,
    pub station: Option<Station>,
}

#[derive(Clone, Default, Debug)]
pub struct WifiBand {
    pub label: &'static str,
    pub ssid: String,
    pub channel: String,
    pub width: String,
    pub txpower: String,
    pub stations: u32,
}

#[derive(Clone, Default, Debug)]
pub struct Battery {
    pub percent: Option<u8>,
    pub status: String,
    pub health: String,
    pub voltage_mv: Option<i64>,
    pub current_ma: Option<i64>,
    pub temp_c: Option<f32>,
    pub cycles: Option<i64>,
    /// Remaining capacity relative to design, in percent.
    pub wear: Option<f32>,
    pub external_power: bool,
    pub limit: Option<u8>,
    pub time_to_full_s: Option<u64>,
    pub time_to_empty_s: Option<u64>,
}

#[derive(Clone, Default, Debug)]
pub struct Proxy {
    pub mode: String,
    pub connections: usize,
    pub down_total: u64,
    pub up_total: u64,
}

#[derive(Default, Clone)]
pub struct Model {
    pub clock: String,
    pub date: String,
    pub kernel: String,
    pub firmware: String,
    pub cpu: Vec<f32>,
    pub cpu_hist: VecDeque<f32>,
    pub freq_mhz: Vec<u32>,
    pub governor: String,
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub mem_cached_kb: u64,
    pub load: [f32; 3],
    pub tasks: u32,
    pub running: u32,
    pub ctxt_rate: f32,
    pub intr_rate: f32,
    pub uptime_s: u64,
    pub temp_cpu: Option<f32>,
    pub temp_modem: Option<f32>,
    pub temp_board: Option<f32>,
    /// Every thermal zone family (hottest member), hottest first.
    pub zones: Vec<(String, f32)>,
    pub cooling_active: u32,
    pub battery: Battery,
    pub current_hist: VecDeque<f32>,
    pub disks: Vec<(&'static str, f32)>,
    pub top: Vec<Proc>,
    pub services: Vec<(&'static str, bool)>,
    pub conntrack: Option<u32>,
    pub conntrack_max: Option<u32>,
    pub tcp_inuse: Option<u32>,
    pub clients: Option<u32>,
    pub sta_2g: Option<u32>,
    pub sta_5g: Option<u32>,
    pub wifi: Vec<WifiBand>,
    pub lan: Vec<Client>,
    pub radio: Option<Radio>,
    pub rsrp_hist: VecDeque<f32>,
    pub sinr_hist: VecDeque<f32>,
    pub dl: Option<f64>,
    pub ul: Option<f64>,
    pub dl_hist: VecDeque<f32>,
    pub ul_hist: VecDeque<f32>,
    pub peak_dl: f64,
    pub peak_ul: f64,
    pub session_rx: Option<u64>,
    pub session_tx: Option<u64>,
    pub session_s: Option<u64>,
    pub day_rx: Option<u64>,
    pub day_tx: Option<u64>,
    pub month_rx: Option<u64>,
    pub month_tx: Option<u64>,
    pub wan_ip: Option<String>,
    pub wan6_up: Option<bool>,
    pub dns: Vec<String>,
    pub ifaces: Vec<Iface>,
    pub mihomo: Option<Proc>,
    pub proxy: Option<Proxy>,
    /// mihomo in detail, read only while the PROXY page is open.
    pub live: crate::proxy::Live,
}

pub fn push(hist: &mut VecDeque<f32>, v: f32) {
    if hist.len() == HISTORY {
        hist.pop_front();
    }
    hist.push_back(v);
}

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn read_num<T: std::str::FromStr>(path: &str) -> Option<T> {
    read(path)?.trim().parse().ok()
}

fn ubus(object: &str, method: &str, params: &str) -> Option<Value> {
    let mut cmd = Command::new("ubus");
    cmd.args(["call", object, method, params]);
    let out = process_runner::output(&mut cmd, None, Duration::from_secs(4), 256 * 1024).ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

fn num(v: &Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Map `zte_nwinfo_api nwinfo_get_netinfo` to the fields the screen shows.
/// NR readings win when the modem reports them (SA and NSA); LTE otherwise.
pub fn parse_radio(v: &Value) -> Radio {
    let nonzero = |x: Option<f64>| x.filter(|n| *n != 0.0).map(|n| n as f32);
    let nr = nonzero(num(v, "nr5g_rsrp")).is_some();
    let pick = |nr_key: &str, lte_key: &str| {
        if nr {
            nonzero(num(v, nr_key))
        } else {
            nonzero(num(v, lte_key))
        }
    };
    let mut band = text(v, "wan_active_band");
    if band.is_empty() {
        band = text(v, if nr { "nr5g_action_band" } else { "lte_band" });
    }
    let ca = [text(v, "lteca"), text(v, "nrca")]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut operator = text(v, "network_provider_fullname");
    if operator.is_empty() {
        operator = text(v, "network_provider");
    }
    Radio {
        rat: text(v, "network_type"),
        band,
        bandwidth: if nr {
            text(v, "nr5g_bandwidth")
        } else {
            String::new()
        },
        pci: num(v, if nr { "nr5g_pci" } else { "lte_pci" }).map(|n| n as i64),
        arfcn: num(
            v,
            if nr {
                "nr5g_action_channel"
            } else {
                "lte_earfcn"
            },
        )
        .map(|n| n as i64),
        operator,
        roaming: text(v, "simcard_roam").eq_ignore_ascii_case("roaming"),
        rsrp: pick("nr5g_rsrp", "lte_rsrp"),
        rsrq: pick("nr5g_rsrq", "lte_rsrq"),
        sinr: pick("nr5g_snr", "lte_snr"),
        rssi: pick("nr5g_rssi", "lte_rssi"),
        bars: num(v, "signalbar").map(|n| n as u8),
        ca,
        mode: text(v, "net_select"),
    }
}

#[derive(Default, Clone, Copy)]
struct CpuTimes {
    busy: u64,
    total: u64,
}

fn parse_stat_line(line: &str) -> Option<CpuTimes> {
    let f: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if f.len() < 5 {
        return None;
    }
    let idle = f[3] + f[4];
    let total: u64 = f.iter().take(8).sum();
    Some(CpuTimes {
        busy: total - idle,
        total,
    })
}

fn usage(prev: CpuTimes, now: CpuTimes) -> f32 {
    let total = now.total.saturating_sub(prev.total);
    if total == 0 {
        return 0.0;
    }
    now.busy.saturating_sub(prev.busy) as f32 * 100.0 / total as f32
}

/// Hottest CPU core, hottest modem subsystem sensor and the XO (board) sensor,
/// plus how many cooling devices are currently throttling.
struct Thermal {
    cpu: Option<f32>,
    modem: Option<f32>,
    board: Option<f32>,
    zones: Vec<(String, f32)>,
    cooling: u32,
}

/// Zone family: `cpuss-2` and `cpuss-0` are both `cpuss`.
fn zone_family(kind: &str) -> &str {
    match kind.rsplit_once('-') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => kind,
    }
}

fn read_thermal() -> Thermal {
    let (mut cpu, mut modem, mut board) = (None::<f32>, None::<f32>, None::<f32>);
    let mut families: HashMap<String, f32> = HashMap::new();
    let mut cooling = 0;
    let Ok(dir) = fs::read_dir("/sys/class/thermal") else {
        return Thermal {
            cpu: None,
            modem: None,
            board: None,
            zones: Vec::new(),
            cooling: 0,
        };
    };
    for entry in dir.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("cooling_device") {
            if fs::read_to_string(path.join("cur_state")).is_ok_and(|s| s.trim() != "0") {
                cooling += 1;
            }
            continue;
        }
        let Some(kind) = fs::read_to_string(path.join("type")).ok() else {
            continue;
        };
        let Some(milli) = fs::read_to_string(path.join("temp"))
            .ok()
            .and_then(|t| t.trim().parse::<f32>().ok())
        else {
            continue;
        };
        let c = milli / 1000.0;
        if !(-40.0..150.0).contains(&c) {
            continue;
        }
        let kind = kind.trim();
        let family = families.entry(zone_family(kind).to_string()).or_insert(c);
        *family = family.max(c);
        let slot = if kind.starts_with("cpuss") {
            &mut cpu
        } else if kind.starts_with("mdmss") || kind.starts_with("mdmq6") {
            &mut modem
        } else if kind == "xo-therm" {
            &mut board
        } else {
            continue;
        };
        *slot = Some(slot.map_or(c, |v| v.max(c)));
    }
    let mut zones: Vec<(String, f32)> = families.into_iter().collect();
    zones.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    Thermal {
        cpu,
        modem,
        board,
        zones,
        cooling,
    }
}

fn read_battery() -> Battery {
    let base = "/sys/class/power_supply/battery";
    let get = |f: &str| read(&format!("{base}/{f}")).map(|s| s.trim().to_string());
    let int = |f: &str| get(f).and_then(|s| s.parse::<i64>().ok());
    let wear = match (int("charge_full"), int("charge_full_design")) {
        (Some(full), Some(design)) if design > 0 && full > 0 => {
            Some(full as f32 * 100.0 / design as f32)
        }
        _ => None,
    };
    let limit = read(CHARGE_LIMIT)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            let enabled = v.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let limit = v
                .get("limit")
                .or_else(|| v.get("charge_limit"))
                .and_then(Value::as_u64)?;
            (enabled && limit < 100).then_some(limit as u8)
        });
    Battery {
        percent: int("capacity").map(|v| v.clamp(0, 100) as u8),
        status: get("status").unwrap_or_default(),
        health: get("health").unwrap_or_default(),
        voltage_mv: int("voltage_now").map(|v| v / 1000),
        current_ma: int("current_now").map(|v| v / 1000),
        temp_c: int("temp").map(|v| v as f32 / 10.0),
        cycles: int("cycle_count"),
        wear,
        external_power: read_num::<u8>("/sys/class/power_supply/usb/online") == Some(1),
        limit,
        // The driver reports huge placeholders when the estimate does not apply.
        time_to_full_s: int("time_to_full_now")
            .filter(|s| (1..86_400).contains(s))
            .map(|s| s as u64),
        time_to_empty_s: int("time_to_empty_avg")
            .filter(|s| (1..360_000).contains(s))
            .map(|s| s as u64),
    }
}

fn disk_used(path: &str) -> Option<f32> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: statvfs writes only into `s`; `c` is a valid NUL-terminated path.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 || s.f_blocks == 0 {
        return None;
    }
    let used = s.f_blocks.saturating_sub(s.f_bfree) as f64;
    let usable = used + s.f_bavail as f64;
    Some((used * 100.0 / usable.max(1.0)) as f32)
}

// libc marks musl's time_t alias deprecated ahead of its 64-bit switch; it is
// already 64-bit on aarch64.
#[allow(deprecated)]
pub fn local_time() -> (String, String) {
    let mut t: libc::time_t = 0;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: time() and localtime_r() only write the locals passed to them.
    unsafe {
        libc::time(&mut t);
        libc::localtime_r(&t, &mut tm);
    }
    (
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec),
        format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        ),
    )
}

/// Per-process CPU accounting between two scans of /proc.
#[derive(Default)]
struct ProcScan {
    ticks: HashMap<u32, u64>,
    total: u64,
}

struct ScanResult {
    scan: ProcScan,
    procs: Vec<Proc>,
    tasks: u32,
    names: HashSet<String>,
}

fn scan_procs(prev: &ProcScan, cpu_total: u64, ncpu: usize) -> ScanResult {
    let page_kb = 4;
    let mut out = ScanResult {
        scan: ProcScan {
            ticks: HashMap::new(),
            total: cpu_total,
        },
        procs: Vec::new(),
        tasks: 0,
        names: HashSet::new(),
    };
    let dt = cpu_total.saturating_sub(prev.total).max(1) as f32;
    let Ok(dir) = fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        out.tasks += 1;
        let Some(stat) = read(&format!("/proc/{pid}/stat")) else {
            continue;
        };
        // comm may contain spaces; the fields after it start past the last ')'.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            continue;
        };
        let name = stat[open + 1..close].to_string();
        let f: Vec<&str> = stat.get(close + 2..).unwrap_or("").split(' ').collect();
        if f.len() < 22 {
            continue;
        }
        let ticks = f[11].parse::<u64>().unwrap_or(0) + f[12].parse::<u64>().unwrap_or(0);
        let rss_kb = f[21].parse::<u64>().unwrap_or(0) * page_kb;
        out.scan.ticks.insert(pid, ticks);
        let cpu = prev
            .ticks
            .get(&pid)
            .map(|before| ticks.saturating_sub(*before) as f32 * 100.0 * ncpu as f32 / dt)
            .unwrap_or(0.0);
        out.names.insert(name.clone());
        out.procs.push(Proc {
            pid,
            name,
            cpu,
            rss_kb,
        });
    }
    out
}

fn iw(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("iw");
    cmd.args(args);
    let out = process_runner::output(&mut cmd, None, Duration::from_secs(3), 512 * 1024).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn parse_stations(dump: &str, band: &'static str) -> Vec<Station> {
    let mut out: Vec<Station> = Vec::new();
    for line in dump.lines() {
        if let Some(rest) = line.strip_prefix("Station ") {
            out.push(Station {
                mac: rest.split_whitespace().next().unwrap_or("").to_lowercase(),
                band,
                ..Default::default()
            });
            continue;
        }
        let Some(st) = out.last_mut() else { continue };
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let first = value.split_whitespace().next().unwrap_or("");
        match key {
            "signal" => st.signal = first.parse().ok(),
            "tx bitrate" => st.rate_mbit = first.parse().ok(),
            "connected time" => st.connected_s = first.parse().ok(),
            _ => {}
        }
    }
    out
}

pub fn parse_wifi_info(info: &str, label: &'static str, stations: u32) -> WifiBand {
    let mut band = WifiBand {
        label,
        stations,
        ..Default::default()
    };
    for line in info.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("ssid ") {
            band.ssid = v.to_string();
        } else if let Some(v) = line.strip_prefix("channel ") {
            band.channel = v.split_whitespace().next().unwrap_or("").to_string();
            band.width = v
                .split("width: ")
                .nth(1)
                .and_then(|w| w.split(',').next())
                .unwrap_or("")
                .replace(" MHz", "M");
        } else if let Some(v) = line.strip_prefix("txpower ") {
            band.txpower = v.replace(".00 dBm", "dBm");
        }
    }
    band
}

/// DHCP leases joined with Wi-Fi stations by MAC. Lease lines are
/// `expiry mac ip hostname clientid`.
pub fn join_clients(leases: &str, stations: &[Station]) -> Vec<Client> {
    let mut out: Vec<Client> = leases
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 4).then(|| Client {
                host: if f[3] == "*" {
                    String::new()
                } else {
                    f[3].to_string()
                },
                ip: f[2].to_string(),
                station: stations
                    .iter()
                    .find(|s| s.mac == f[1].to_lowercase())
                    .cloned(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        let key = |c: &Client| {
            c.station
                .as_ref()
                .and_then(|s| s.signal)
                .map_or(i32::MIN, |s| s)
        };
        key(b).cmp(&key(a)).then(a.ip.cmp(&b.ip))
    });
    out
}

pub struct Collector {
    prev_total: CpuTimes,
    prev_cores: Vec<CpuTimes>,
    prev_ctxt: Option<(u64, u64)>,
    prev_dev: HashMap<String, (u64, u64)>,
    procs: ProcScan,
    ticks: u64,
    proxy: crate::proxy::Poller,
    proxy_open: bool,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

/// Which pages are on screen, so their slower sources are read only when needed.
#[derive(Clone, Copy, Default)]
pub struct Needs {
    pub net: bool,
    pub lan: bool,
    pub proxy: bool,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            prev_total: CpuTimes::default(),
            prev_cores: Vec::new(),
            prev_ctxt: None,
            prev_dev: HashMap::new(),
            procs: ProcScan::default(),
            ticks: 0,
            proxy: crate::proxy::Poller::default(),
            proxy_open: false,
        }
    }

    /// One tick per second. Slower sources refresh on multiples of it.
    pub fn tick(&mut self, m: &mut Model, needs: Needs) {
        let n = self.ticks;
        self.ticks += 1;
        (m.clock, m.date) = local_time();
        if n == 0 {
            m.kernel = read("/proc/sys/kernel/osrelease")
                .unwrap_or_default()
                .trim()
                .to_string();
            m.firmware = ubus("zwrt_zte_mdm.api", "get_zwrt_common_info", "{}")
                .map(|v| text(&v, "integrate_version"))
                .unwrap_or_default();
        }
        self.cpu(m);
        self.memory(m);
        if let Some(l) = read("/proc/loadavg") {
            let v: Vec<&str> = l.split_whitespace().collect();
            if v.len() >= 4 {
                m.load = [
                    v[0].parse().unwrap_or(0.0),
                    v[1].parse().unwrap_or(0.0),
                    v[2].parse().unwrap_or(0.0),
                ];
                m.running = v[3]
                    .split('/')
                    .next()
                    .and_then(|r| r.parse().ok())
                    .unwrap_or(0);
            }
        }
        m.uptime_s = read("/proc/uptime")
            .and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok())
            .map(|s| s as u64)
            .unwrap_or(0);
        self.throughput(m);
        self.interfaces(m);

        if n % 2 == 0 {
            let t = read_thermal();
            (
                m.temp_cpu,
                m.temp_modem,
                m.temp_board,
                m.cooling_active,
                m.zones,
            ) = (t.cpu, t.modem, t.board, t.cooling, t.zones);
            m.battery = read_battery();
            if let Some(ma) = m.battery.current_ma {
                push(&mut m.current_hist, ma as f32);
            }
            self.processes(m);
            m.conntrack = read_num("/proc/sys/net/netfilter/nf_conntrack_count");
            m.tcp_inuse = read("/proc/net/sockstat").and_then(|s| {
                let line = s.lines().find(|l| l.starts_with("TCP:"))?;
                let mut it = line.split_whitespace();
                it.position(|w| w == "inuse")?;
                it.next()?.parse().ok()
            });
            m.clients = read("/tmp/dhcp.leases")
                .map(|l| l.lines().filter(|x| !x.trim().is_empty()).count() as u32);
        }
        if n % 3 == 0 {
            if let Some(v) = ubus("zte_nwinfo_api", "nwinfo_get_netinfo", "{}") {
                let r = parse_radio(&v);
                if let Some(rsrp) = r.rsrp {
                    push(&mut m.rsrp_hist, rsrp);
                }
                if let Some(sinr) = r.sinr {
                    push(&mut m.sinr_hist, sinr);
                }
                m.radio = Some(r);
            }
        }
        if n % if needs.lan { 3 } else { 5 } == 0 {
            let s2 = iw(&["dev", "wlan0", "station", "dump"]).map(|d| parse_stations(&d, "2.4G"));
            let s5 = iw(&["dev", "wlan2", "station", "dump"]).map(|d| parse_stations(&d, "5G"));
            m.sta_2g = s2.as_ref().map(|s| s.len() as u32);
            m.sta_5g = s5.as_ref().map(|s| s.len() as u32);
            if needs.lan {
                let stations: Vec<Station> = s2.into_iter().chain(s5).flatten().collect();
                m.lan = join_clients(&read("/tmp/dhcp.leases").unwrap_or_default(), &stations);
                m.wifi = [("wlan0", "2.4G", m.sta_2g), ("wlan2", "5G", m.sta_5g)]
                    .iter()
                    .filter_map(|(dev, label, n)| {
                        iw(&["dev", dev, "info"])
                            .map(|i| parse_wifi_info(&i, label, n.unwrap_or(0)))
                    })
                    .collect();
            }
        }
        if needs.proxy {
            self.proxy
                .tick(&mut m.live, crate::pet::clock(), !self.proxy_open);
        }
        self.proxy_open = needs.proxy;
        if n % 5 == 0 {
            m.proxy = if m.mihomo.is_some() && (needs.net || n % 15 == 0) {
                crate::proxy::get("/configs").map(|configs| {
                    let conns = crate::proxy::get("/connections");
                    Proxy {
                        mode: text(&configs, "mode"),
                        connections: conns
                            .as_ref()
                            .and_then(|c| c.get("connections")?.as_array().map(Vec::len))
                            .unwrap_or(0),
                        down_total: conns
                            .as_ref()
                            .and_then(|c| num(c, "downloadTotal"))
                            .unwrap_or(0.0) as u64,
                        up_total: conns
                            .as_ref()
                            .and_then(|c| num(c, "uploadTotal"))
                            .unwrap_or(0.0) as u64,
                    }
                })
            } else if m.mihomo.is_none() {
                None
            } else {
                m.proxy.take()
            };
        }
        if n % 10 == 0 {
            if let Some(v) = ubus("network.interface.zte_wan", "status", "{}") {
                m.wan_ip = v
                    .get("ipv4-address")
                    .and_then(|a| a.get(0))
                    .and_then(|a| a.get("address"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            m.wan6_up = ubus("network.interface.zte_wan6", "status", "{}")
                .and_then(|v| v.get("up").and_then(Value::as_bool));
            m.dns = read("/tmp/resolv.conf.d/resolv.conf.auto")
                .or_else(|| read("/tmp/resolv.conf"))
                .map(|s| {
                    s.lines()
                        .filter_map(|l| l.strip_prefix("nameserver "))
                        .map(|a| a.trim().to_string())
                        .collect()
                })
                .unwrap_or_default();
            m.conntrack_max = read_num("/proc/sys/net/netfilter/nf_conntrack_max");
            m.disks = [("/data", "data"), ("/tmp", "tmp"), ("/etc", "etc")]
                .iter()
                .filter_map(|(p, l)| disk_used(p).map(|u| (*l, u)))
                .collect();
            m.governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
                .unwrap_or_default()
                .trim()
                .to_string();
        }
    }

    fn cpu(&mut self, m: &mut Model) {
        let Some(stat) = read("/proc/stat") else {
            return;
        };
        let mut lines = stat.lines();
        if let Some(total) = lines.next().and_then(parse_stat_line) {
            if self.prev_total.total > 0 {
                push(&mut m.cpu_hist, usage(self.prev_total, total));
            }
            self.prev_total = total;
        }
        let cores: Vec<CpuTimes> = stat
            .lines()
            .skip(1)
            .take_while(|l| l.starts_with("cpu"))
            .filter_map(parse_stat_line)
            .collect();
        if self.prev_cores.len() == cores.len() {
            m.cpu = self
                .prev_cores
                .iter()
                .zip(&cores)
                .map(|(a, b)| usage(*a, *b))
                .collect();
        } else {
            m.cpu = vec![0.0; cores.len()];
        }
        self.prev_cores = cores;
        m.freq_mhz = (0..m.cpu.len())
            .map(|i| {
                read_num::<u32>(&format!(
                    "/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"
                ))
                .unwrap_or(0)
                    / 1000
            })
            .collect();
        let field = |key: &str| {
            stat.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        };
        if let (Some(ctxt), Some(intr)) = (field("ctxt "), field("intr ")) {
            if let Some((pc, pi)) = self.prev_ctxt {
                m.ctxt_rate = ctxt.saturating_sub(pc) as f32;
                m.intr_rate = intr.saturating_sub(pi) as f32;
            }
            self.prev_ctxt = Some((ctxt, intr));
        }
    }

    fn memory(&mut self, m: &mut Model) {
        let Some(info) = read("/proc/meminfo") else {
            return;
        };
        let field = |k: &str| {
            info.lines()
                .find(|l| l.starts_with(k))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        m.mem_total_kb = field("MemTotal:");
        m.mem_avail_kb = field("MemAvailable:");
        m.mem_cached_kb = field("Cached:");
    }

    fn processes(&mut self, m: &mut Model) {
        let ncpu = m.cpu.len().max(1);
        let result = scan_procs(&self.procs, self.prev_total.total, ncpu);
        let first = self.procs.total == 0;
        self.procs = result.scan;
        m.tasks = result.tasks;
        m.services = SERVICES
            .iter()
            .map(|(label, comm)| (*label, result.names.contains(*comm)))
            .collect();
        let mut procs = result.procs;
        m.mihomo = procs.iter().find(|p| p.name == "mihomo").cloned();
        if first {
            return;
        }
        procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.rss_kb.cmp(&a.rss_kb)));
        procs.truncate(8);
        m.top = procs;
    }

    fn throughput(&mut self, m: &mut Model) {
        // The modem's own counters include IPA-offloaded traffic that the
        // rmnet netdev counters miss (see agent/src/system.rs SpeedTracker).
        let Some(v) = ubus(
            "zwrt_data",
            "get_wwandst",
            r#"{"source_module":"web","cid":1,"type":4}"#,
        ) else {
            m.dl = None;
            m.ul = None;
            return;
        };
        m.dl = num(&v, "real_rx_speed");
        m.ul = num(&v, "real_tx_speed");
        m.session_rx = num(&v, "real_rx_bytes").map(|n| n as u64);
        m.session_tx = num(&v, "real_tx_bytes").map(|n| n as u64);
        m.session_s = num(&v, "real_time").map(|n| n as u64);
        let total =
            |prefix: &str, dir: &str| num(&v, &format!("{prefix}_{dir}_bytes")).map(|n| n as u64);
        (m.day_rx, m.day_tx) = (total("day", "rx"), total("day", "tx"));
        (m.month_rx, m.month_tx) = (total("month", "rx"), total("month", "tx"));
        m.peak_dl = m
            .peak_dl
            .max(num(&v, "real_max_rx_speed").unwrap_or(0.0))
            .max(m.dl.unwrap_or(0.0));
        m.peak_ul = m
            .peak_ul
            .max(num(&v, "real_max_tx_speed").unwrap_or(0.0))
            .max(m.ul.unwrap_or(0.0));
        push(&mut m.dl_hist, m.dl.unwrap_or(0.0) as f32);
        push(&mut m.ul_hist, m.ul.unwrap_or(0.0) as f32);
    }

    /// Kernel netdev rates. With IPA offload these undercount forwarded
    /// traffic, which is why the WAN figure comes from the modem instead.
    fn interfaces(&mut self, m: &mut Model) {
        let Some(dev) = read("/proc/net/dev") else {
            return;
        };
        let mut counters = HashMap::new();
        for line in dev.lines().skip(2) {
            let Some((name, rest)) = line.split_once(':') else {
                continue;
            };
            let f: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            if f.len() >= 9 {
                counters.insert(name.trim().to_string(), (f[0], f[8]));
            }
        }
        m.ifaces = IFACES
            .iter()
            .filter_map(|(dev, label)| {
                let (rx, tx) = *counters.get(*dev)?;
                let (prx, ptx) = self.prev_dev.get(*dev).copied().unwrap_or((rx, tx));
                let up = read(&format!("/sys/class/net/{dev}/operstate"))
                    .is_some_and(|s| s.trim() != "down");
                Some(Iface {
                    label,
                    up,
                    rx_rate: rx.saturating_sub(prx) as f64,
                    tx_rate: tx.saturating_sub(ptx) as f64,
                })
            })
            .collect();
        self.prev_dev = counters;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sa_radio_uses_nr_fields() {
        let r = parse_radio(&json!({
            "network_type": "SA", "wan_active_band": "n41", "nr5g_pci": 843,
            "nr5g_action_channel": 504990, "nr5g_bandwidth": "100",
            "network_provider_fullname": "China Mobile", "nr5g_rsrp": -57,
            "nr5g_rsrq": -11, "nr5g_snr": "36.9", "nr5g_rssi": -51, "lte_rsrp": 0,
            "lte_snr": "", "signalbar": "5", "lteca": "", "nrca": "", "simcard_roam": "Home"
        }));
        assert_eq!(r.rat, "SA");
        assert_eq!(r.band, "n41");
        assert_eq!(r.pci, Some(843));
        assert_eq!(r.rsrp, Some(-57.0));
        assert_eq!(r.sinr, Some(36.9));
        assert_eq!(r.rssi, Some(-51.0));
        assert_eq!(r.bars, Some(5));
        assert!(!r.roaming);
        assert!(r.ca.is_empty());
    }

    #[test]
    fn lte_radio_falls_back_to_lte_fields() {
        let r = parse_radio(&json!({
            "network_type": "LTE", "lte_band": "3", "lte_pci": 12, "lte_rsrp": -101,
            "lte_rsrq": -14, "lte_snr": "3.5", "nr5g_rsrp": 0, "lteca": "3+40",
            "simcard_roam": "Roaming"
        }));
        assert_eq!(r.band, "3");
        assert_eq!(r.rsrp, Some(-101.0));
        assert_eq!(r.sinr, Some(3.5));
        assert_eq!(r.ca, "3+40");
        assert!(r.roaming);
    }

    #[test]
    fn cpu_usage_from_stat_deltas() {
        let a = parse_stat_line("cpu  100 0 100 800 0 0 0 0 0 0").unwrap();
        let b = parse_stat_line("cpu  150 0 150 900 0 0 0 0 0 0").unwrap();
        assert!((usage(a, b) - 50.0).abs() < 0.01);
        assert_eq!(usage(b, b), 0.0);
    }

    #[test]
    fn history_is_bounded() {
        let mut h = VecDeque::new();
        for i in 0..100 {
            push(&mut h, i as f32);
        }
        assert_eq!(h.len(), HISTORY);
        assert_eq!(h.front(), Some(&40.0));
    }

    #[test]
    fn stations_and_leases_join_by_mac() {
        let dump = "Station AA:BB:CC:00:11:22 (on wlan0)\n\tsignal:  \t-48 dBm\n\ttx bitrate:\t866.7 MBit/s VHT-MCS 9\n\tconnected time:\t1234 seconds\nStation aa:bb:cc:00:11:33 (on wlan0)\n\tsignal:  \t-70 [-70] dBm\n";
        let st = parse_stations(dump, "5G");
        assert_eq!(st.len(), 2);
        assert_eq!(st[0].mac, "aa:bb:cc:00:11:22");
        assert_eq!(st[0].signal, Some(-48));
        assert_eq!(st[0].rate_mbit, Some(866.7));
        assert_eq!(st[0].connected_s, Some(1234));
        assert_eq!(st[1].signal, Some(-70));
        let leases = "86400 aa:bb:cc:00:11:33 192.168.0.5 phone *\n86400 aa:bb:cc:00:11:22 192.168.0.9 laptop 01:aa\n86400 dd:ee:ff:00:00:01 192.168.0.7 * *\n";
        let clients = join_clients(leases, &st);
        assert_eq!(clients[0].host, "laptop");
        assert_eq!(clients[1].host, "phone");
        assert!(clients[2].station.is_none());
        assert_eq!(clients[2].host, "");
    }

    #[test]
    fn wifi_info_is_summarised() {
        let info = "Interface wlan2\n\tssid home\n\tchannel 44 (5220 MHz), width: 160 MHz, center1: 5250 MHz\n\ttxpower 18.00 dBm\n";
        let b = parse_wifi_info(info, "5G", 3);
        assert_eq!(
            (
                b.ssid.as_str(),
                b.channel.as_str(),
                b.width.as_str(),
                b.txpower.as_str()
            ),
            ("home", "44", "160M", "18dBm")
        );
    }

    #[test]
    fn zone_families_strip_indices() {
        assert_eq!(zone_family("cpuss-3"), "cpuss");
        assert_eq!(zone_family("xo-therm"), "xo-therm");
        assert_eq!(zone_family("sdr0"), "sdr0");
    }
}
