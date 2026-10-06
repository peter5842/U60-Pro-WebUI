//! Persistent mihomo manager state (settings + subscriptions) and the pure
//! function that renders it into a mihomo config.
//!
//! The config is emitted as JSON, which mihomo's YAML loader accepts. Building
//! it from typed values means a subscription name or URL can never break out of
//! its field the way string-templated YAML could.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// mihomo's external controller. Loopback only: the agent proxies every call
/// behind its own auth, so the controller is never reachable from the LAN.
pub const CONTROLLER_ADDR: &str = "127.0.0.1:9097";
/// TUN interface name (also used in the runtime firewall rules).
pub const TUN_DEVICE: &str = "mihomo";
/// The only interface whose traffic the TUN captures. The router's own
/// traffic (ZTE daemons, DNS, NTP, the agent) never enters the tunnel.
pub const LAN_BRIDGE: &str = "br-lan";
pub const HEALTH_URL: &str = "https://www.gstatic.com/generate_204";
pub const DEFAULT_PORT: u16 = 7890;
const MAX_SUBSCRIPTIONS: usize = 16;
const MAX_NAME_CHARS: usize = 32;
const MAX_URL_LEN: usize = 2048;
const MAX_INTERVAL_HOURS: u32 = 720;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Rule,
    Global,
    Direct,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Rule => "rule",
            Mode::Global => "global",
            Mode::Direct => "direct",
        }
    }
}

/// Rule presets for `mode: rule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Mainland China domains/IPs go direct, everything else through PROXY.
    BypassCn,
    /// Only the GFW list goes through PROXY, everything else direct.
    Gfw,
    /// Everything except private ranges goes through PROXY.
    ProxyAll,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Desired running state; also means "start when the agent boots".
    pub enabled: bool,
    pub mode: Mode,
    pub preset: Preset,
    /// Capture LAN traffic transparently through the `mihomo` TUN device.
    pub tun: bool,
    pub mixed_port: u16,
    /// Subscription whose own full config (groups, rules, rule sets) is used.
    /// `None`: the managed config (subscriptions as node providers + `preset`).
    pub profile: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: Mode::Rule,
            preset: Preset::BypassCn,
            tun: false,
            mixed_port: DEFAULT_PORT,
            profile: None,
        }
    }
}

/// Traffic and expiry from the provider's `subscription-userinfo` header.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Usage {
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    pub expire: Option<u64>,
}

/// What the agent learnt the last time it downloaded a subscription itself
/// (only for subscriptions used as the full config).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fetched {
    /// Unix seconds.
    pub at: u64,
    /// Has `proxy-groups` and `rules`, i.e. usable as a full config.
    pub full: bool,
    #[serde(default)]
    pub proxies: u32,
    #[serde(default)]
    pub groups: u32,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    /// 8 hex chars; the mihomo provider is named `sub-<id>`.
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    /// Auto-update interval; 0 = manual updates only.
    pub interval_hours: u32,
    #[serde(default)]
    pub fetched: Option<Fetched>,
}

impl Subscription {
    pub fn provider(&self) -> String {
        provider_name(&self.id)
    }
}

pub fn provider_name(id: &str) -> String {
    format!("sub-{id}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub settings: Settings,
    pub subscriptions: Vec<Subscription>,
    /// Controller secret, generated once.
    pub secret: String,
}

// ── Validation ───────────────────────────────────────────────────────────────

pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is required".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("name must be at most {MAX_NAME_CHARS} characters"));
    }
    if name.chars().any(char::is_control) {
        return Err("name must not contain control characters".into());
    }
    Ok(name.to_string())
}

pub fn validate_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("subscription URL must start with http:// or https://")?;
    if url.len() > MAX_URL_LEN {
        return Err(format!(
            "subscription URL must be at most {MAX_URL_LEN} bytes"
        ));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("subscription URL must not contain spaces or control characters".into());
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.starts_with('@') || host.ends_with('@') {
        return Err("subscription URL has no host".into());
    }
    Ok(url.to_string())
}

pub fn validate_interval(hours: u64) -> Result<u32, String> {
    if hours > MAX_INTERVAL_HOURS as u64 {
        return Err(format!("interval must be 0-{MAX_INTERVAL_HOURS} hours"));
    }
    Ok(hours as u32)
}

pub fn validate_port(port: u64) -> Result<u16, String> {
    // Stay clear of privileged ports and the agent/dashboard/SSH listeners.
    match port {
        1024..=65535 if ![2222, 8080, 9090, 9097].contains(&port) => Ok(port as u16),
        _ => Err("mixed_port must be 1024-65535 and not 2222, 8080, 9090 or 9097".into()),
    }
}

pub fn can_add_subscription(state: &State) -> Result<(), String> {
    if state.subscriptions.len() >= MAX_SUBSCRIPTIONS {
        return Err(format!("at most {MAX_SUBSCRIPTIONS} subscriptions"));
    }
    Ok(())
}

/// `https://sub.example.com/…` — enough to recognise a subscription without
/// exposing the token most providers embed in the path or query.
pub fn mask_url(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some(parts) => parts,
        None => return "…".into(),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Drop any userinfo; it is a credential.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if rest.len() > authority.len() {
        format!("{scheme}://{host}/…")
    } else {
        format!("{scheme}://{host}")
    }
}

// ── Config rendering ─────────────────────────────────────────────────────────

fn preset_rules(preset: Preset) -> Vec<&'static str> {
    match preset {
        Preset::BypassCn => vec![
            "GEOSITE,private,DIRECT",
            "GEOIP,private,DIRECT,no-resolve",
            "GEOSITE,cn,DIRECT",
            "GEOIP,CN,DIRECT",
            "MATCH,PROXY",
        ],
        Preset::Gfw => vec![
            "GEOSITE,private,DIRECT",
            "GEOIP,private,DIRECT,no-resolve",
            "GEOSITE,gfw,PROXY",
            "MATCH,DIRECT",
        ],
        Preset::ProxyAll => vec![
            "GEOSITE,private,DIRECT",
            "GEOIP,private,DIRECT,no-resolve",
            "MATCH,PROXY",
        ],
    }
}

/// Render the full mihomo config for `state`. `lan_ip` is the router's LAN
/// address; the proxy listener binds only to it. With a subscription profile,
/// `profile` is that subscription's parsed config.
pub fn render(state: &State, lan_ip: &str, profile: Option<&Value>) -> Result<Value, String> {
    let mut config = match profile {
        Some(base) => sanitize_profile(base)?,
        None => managed(state),
    };
    apply_runtime(&mut config, state, lan_ip);
    Ok(config)
}

/// The agent's own config: subscriptions as node providers plus a rule preset.
fn managed(state: &State) -> Value {
    let s = &state.settings;
    let enabled: Vec<&Subscription> = state.subscriptions.iter().filter(|x| x.enabled).collect();

    let mut providers = Map::new();
    for sub in &enabled {
        providers.insert(
            sub.provider(),
            json!({
                "type": "http",
                "url": sub.url,
                "path": format!("./providers/{}.yaml", sub.provider()),
                "interval": sub.interval_hours * 3600,
                // Fetch the subscription directly: through PROXY it would
                // depend on nodes that only exist once it has been fetched.
                "proxy": "DIRECT",
                "health-check": {"enable": true, "url": HEALTH_URL, "interval": 600, "lazy": true},
            }),
        );
    }
    let uses: Vec<String> = enabled.iter().map(|x| x.provider()).collect();

    let groups = if uses.is_empty() {
        json!([{"name": "PROXY", "type": "select", "proxies": ["DIRECT"]}])
    } else {
        json!([
            {"name": "PROXY", "type": "select", "proxies": ["AUTO", "DIRECT"], "use": uses},
            {"name": "AUTO", "type": "url-test", "use": uses, "url": HEALTH_URL,
             "interval": 600, "tolerance": 50, "lazy": true},
        ])
    };

    json!({
        "ipv6": false,
        "unified-delay": true,
        "tcp-concurrent": true,
        "geodata-mode": false,
        "dns": {
            "enable": true,
            "ipv6": false,
            "enhanced-mode": "redir-host",
            "nameserver": ["223.5.5.5", "119.29.29.29"],
        },
        "proxy-providers": Value::Object(providers),
        "proxy-groups": groups,
        "rules": preset_rules(s.preset),
    })
}

/// Keys a subscription config may carry that must not reach the router:
/// extra listeners, public controllers/UIs, its own TUN/redirect plumbing,
/// proxy auth the PAC cannot supply, and interface pinning.
const PROFILE_DROP: &[&str] = &[
    "port",
    "socks-port",
    "redir-port",
    "tproxy-port",
    "mixed-port",
    "listeners",
    "tunnels",
    "external-controller",
    "external-controller-tls",
    "external-controller-unix",
    "external-controller-pipe",
    "external-controller-cors",
    "external-ui",
    "external-ui-url",
    "external-ui-name",
    "external-doh-server",
    "secret",
    "authentication",
    "skip-auth-prefixes",
    "lan-allowed-ips",
    "lan-disallowed-ips",
    "bind-address",
    "interface-name",
    "routing-mark",
    "tun",
    "iptables",
    "ebpf",
    "auto-redir",
];

/// A subscription's own config, minus everything in `PROFILE_DROP` and any
/// DNS listener (port 53 belongs to dnsmasq). Groups, rules, rule sets, DNS
/// upstreams and sniffer settings are kept as the provider wrote them.
pub fn sanitize_profile(base: &Value) -> Result<Value, String> {
    let mut config = base.clone();
    let obj = config
        .as_object_mut()
        .ok_or("the subscription is not a YAML mapping")?;
    if !obj.get("proxy-groups").is_some_and(Value::is_array)
        || !obj.get("rules").is_some_and(Value::is_array)
    {
        return Err(
            "the subscription has no proxy-groups/rules, so it cannot be used as a full config"
                .into(),
        );
    }
    for key in PROFILE_DROP {
        obj.remove(*key);
    }
    if let Some(dns) = obj.get_mut("dns").and_then(Value::as_object_mut) {
        dns.remove("listen");
    }
    Ok(config)
}

/// Settings the agent always owns, whatever the config source.
fn apply_runtime(config: &mut Value, state: &State, lan_ip: &str) {
    let s = &state.settings;
    let obj = config.as_object_mut().expect("config is an object");
    obj.insert("mixed-port".into(), json!(s.mixed_port));
    obj.insert("allow-lan".into(), json!(true));
    obj.insert("bind-address".into(), json!(lan_ip));
    obj.insert("mode".into(), json!(s.mode.as_str()));
    obj.insert("log-level".into(), json!("warning"));
    obj.insert("find-process-mode".into(), json!("off"));
    obj.insert("external-controller".into(), json!(CONTROLLER_ADDR));
    obj.insert("secret".into(), json!(state.secret));
    // Rule data is installed and updated by scripts/deploy-mihomo.sh.
    obj.insert("geo-auto-update".into(), json!(false));
    let profile = obj.entry("profile").or_insert_with(|| json!({}));
    if let Some(p) = profile.as_object_mut() {
        p.insert("store-selected".into(), json!(true));
    }

    obj.remove("tun");
    if s.tun {
        obj.insert(
            "tun".into(),
            json!({
                "enable": true,
                "stack": "mixed",
                "device": TUN_DEVICE,
                "auto-route": true,
                "auto-detect-interface": true,
                "include-interface": [LAN_BRIDGE],
                "route-exclude-address": [
                    "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16",
                    "fc00::/7", "fe80::/10",
                ],
                // DNS stays with dnsmasq; domains come from the sniffer.
                "dns-hijack": [],
            }),
        );
        // Without DNS hijack, LAN clients resolve through dnsmasq and may get
        // poisoned addresses: the sniffed domain must replace the destination
        // so mihomo (or the node) connects to the real host. A provider's
        // sniffer keeps its skip list and ports.
        let sniffing = obj
            .get("sniffer")
            .and_then(|v| v["enable"].as_bool())
            .unwrap_or(false);
        if !sniffing {
            obj.insert(
                "sniffer".into(),
                json!({
                    "enable": true,
                    "sniff": {
                        "HTTP": {"ports": [80, "8080-8880"]},
                        "TLS": {"ports": [443, 8443]},
                        "QUIC": {"ports": [443]},
                    },
                }),
            );
        }
        if let Some(sniffer) = obj.get_mut("sniffer").and_then(Value::as_object_mut) {
            sniffer.insert("override-destination".into(), json!(true));
            if let Some(protocols) = sniffer.get_mut("sniff").and_then(Value::as_object_mut) {
                for proto in protocols.values_mut().filter_map(Value::as_object_mut) {
                    proto.insert("override-destination".into(), json!(true));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(id: &str, enabled: bool) -> Subscription {
        Subscription {
            id: id.into(),
            name: format!("Sub {id}"),
            url: format!("https://example.com/{id}?token=secret"),
            enabled,
            interval_hours: 24,
            fetched: None,
        }
    }

    #[test]
    fn no_subscriptions_renders_direct_only_proxy_group() {
        let state = State::default();
        let c = render(&state, "192.168.0.1", None).unwrap();
        assert_eq!(c["proxy-groups"].as_array().unwrap().len(), 1);
        assert_eq!(c["proxy-groups"][0]["proxies"], json!(["DIRECT"]));
        assert!(c["proxy-providers"].as_object().unwrap().is_empty());
        assert!(c.get("tun").is_none());
        assert_eq!(c["bind-address"], "192.168.0.1");
        assert_eq!(c["external-controller"], CONTROLLER_ADDR);
    }

    #[test]
    fn only_enabled_subscriptions_become_providers_fetched_direct() {
        let state = State {
            subscriptions: vec![sub("aaaa0001", true), sub("aaaa0002", false)],
            ..Default::default()
        };
        let c = render(&state, "192.168.0.1", None).unwrap();
        let providers = c["proxy-providers"].as_object().unwrap();
        assert_eq!(providers.len(), 1);
        let p = &providers["sub-aaaa0001"];
        assert_eq!(p["proxy"], "DIRECT");
        assert_eq!(p["interval"], 24 * 3600);
        assert_eq!(c["proxy-groups"][0]["use"], json!(["sub-aaaa0001"]));
        assert_eq!(c["proxy-groups"][1]["name"], "AUTO");
    }

    #[test]
    fn tun_is_lan_only_and_leaves_dns_alone() {
        let state = State {
            settings: Settings {
                tun: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let c = render(&state, "192.168.0.1", None).unwrap();
        assert_eq!(c["tun"]["device"], TUN_DEVICE);
        assert_eq!(c["tun"]["include-interface"], json!([LAN_BRIDGE]));
        assert_eq!(c["tun"]["dns-hijack"], json!([]));
        assert_eq!(c["sniffer"]["enable"], true);
    }

    fn airport() -> Value {
        json!({
            "mixed-port": 7890, "socks-port": 7891, "port": 7892,
            "allow-lan": true, "bind-address": "*", "authentication": ["u:p"],
            "external-controller": "127.0.0.1:9090", "secret": "theirs", "external-ui": "ui",
            "log-level": "error", "find-process-mode": "always", "geodata-mode": true,
            "tun": {"enable": true, "stack": "system", "dns-hijack": ["any:53"]},
            "dns": {"enable": true, "listen": "0.0.0.0:53", "enhanced-mode": "fake-ip", "nameserver": ["https://dns.example/dns-query"]},
            "sniffer": {"enable": true, "sniff": {"TLS": {"ports": [443]}}},
            "proxies": [{"name": "HK", "type": "ss"}],
            "proxy-groups": [{"name": "节点选择", "type": "select", "proxies": ["HK"]}],
            "rule-providers": {"apple": {"type": "http", "behavior": "classical"}},
            "rules": ["RULE-SET,apple,节点选择", "MATCH,节点选择"],
        })
    }

    #[test]
    fn profile_keeps_groups_rules_and_dns_upstreams() {
        let state = State {
            secret: "s".into(),
            ..Default::default()
        };
        let c = render(&state, "192.168.0.1", Some(&airport())).unwrap();
        assert_eq!(c["proxy-groups"][0]["name"], "节点选择");
        assert_eq!(c["rules"][1], "MATCH,节点选择");
        assert!(c["rule-providers"]["apple"].is_object());
        assert_eq!(c["dns"]["enhanced-mode"], "fake-ip");
        assert_eq!(c["dns"]["nameserver"][0], "https://dns.example/dns-query");
        assert_eq!(c["geodata-mode"], true);
    }

    #[test]
    fn profile_cannot_open_listeners_or_take_over_the_router() {
        let state = State {
            secret: "s".into(),
            ..Default::default()
        };
        let c = render(&state, "192.168.0.1", Some(&airport())).unwrap();
        for key in ["socks-port", "port", "authentication", "external-ui", "tun"] {
            assert!(c.get(key).is_none(), "{key} must be dropped");
        }
        assert!(c["dns"].get("listen").is_none());
        assert_eq!(c["mixed-port"], DEFAULT_PORT);
        assert_eq!(c["bind-address"], "192.168.0.1");
        assert_eq!(c["external-controller"], CONTROLLER_ADDR);
        assert_eq!(c["secret"], "s");
        assert_eq!(c["find-process-mode"], "off");
        assert_eq!(c["geo-auto-update"], false);
        assert_eq!(c["profile"]["store-selected"], true);
    }

    #[test]
    fn profile_tun_is_ours_and_keeps_their_sniffer() {
        let state = State {
            settings: Settings {
                tun: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let c = render(&state, "192.168.0.1", Some(&airport())).unwrap();
        assert_eq!(c["tun"]["stack"], "mixed");
        assert_eq!(c["tun"]["include-interface"], json!([LAN_BRIDGE]));
        assert_eq!(c["tun"]["dns-hijack"], json!([]));
        assert_eq!(c["sniffer"]["sniff"]["TLS"]["ports"], json!([443]));
        // No DNS hijack on the router: sniffed domains must override poisoned IPs.
        assert_eq!(c["sniffer"]["override-destination"], true);
        assert_eq!(c["sniffer"]["sniff"]["TLS"]["override-destination"], true);
    }

    #[test]
    fn without_tun_the_provider_sniffer_is_untouched() {
        let state = State::default();
        let c = render(&state, "192.168.0.1", Some(&airport())).unwrap();
        assert!(c["sniffer"].get("override-destination").is_none());
        assert!(c["sniffer"]["sniff"]["TLS"]
            .get("override-destination")
            .is_none());
    }

    #[test]
    fn profile_without_groups_or_rules_is_rejected() {
        let state = State::default();
        let nodes_only = json!({"proxies": [{"name": "HK", "type": "ss"}]});
        assert!(render(&state, "192.168.0.1", Some(&nodes_only)).is_err());
        assert!(render(&state, "192.168.0.1", Some(&json!(["not", "a", "map"]))).is_err());
    }

    #[test]
    fn presets_end_in_a_match_rule() {
        for preset in [Preset::BypassCn, Preset::Gfw, Preset::ProxyAll] {
            let rules = preset_rules(preset);
            assert!(rules.last().unwrap().starts_with("MATCH,"));
            assert!(rules.iter().any(|r| r.starts_with("GEOIP,private,DIRECT")));
        }
    }

    #[test]
    fn url_validation() {
        assert!(validate_url("https://a.example/sub?token=x").is_ok());
        assert!(validate_url(" http://a.example ").is_ok());
        assert!(validate_url("ftp://a.example").is_err());
        assert!(validate_url("https://").is_err());
        assert!(validate_url("https://a.example/x y").is_err());
        assert!(validate_url("https://a.example/\nx").is_err());
        assert!(validate_url(&format!("https://a.example/{}", "x".repeat(MAX_URL_LEN))).is_err());
    }

    #[test]
    fn name_and_port_validation() {
        assert_eq!(validate_name("  Airport  ").unwrap(), "Airport");
        assert!(validate_name("").is_err());
        assert!(validate_name("a\u{7}b").is_err());
        assert!(validate_name(&"x".repeat(33)).is_err());
        assert!(validate_port(7890).is_ok());
        assert!(validate_port(9090).is_err());
        assert!(validate_port(80).is_err());
        assert!(validate_port(70000).is_err());
        assert!(validate_interval(721).is_err());
    }

    #[test]
    fn mask_url_hides_path_query_and_userinfo() {
        assert_eq!(
            mask_url("https://sub.example.com/api/v1?token=abc"),
            "https://sub.example.com/…"
        );
        assert_eq!(
            mask_url("https://user:pw@sub.example.com/x"),
            "https://sub.example.com/…"
        );
        assert_eq!(
            mask_url("https://sub.example.com"),
            "https://sub.example.com"
        );
        assert_eq!(mask_url("garbage"), "…");
    }

    #[test]
    fn state_round_trips_and_tolerates_missing_fields() {
        let state: State = serde_json::from_str(r#"{"secret":"s"}"#).unwrap();
        assert_eq!(state.settings, Settings::default());
        let text = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<State>(&text).unwrap(), state);
    }
}
