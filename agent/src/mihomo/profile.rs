//! Subscriptions used as the full config: download with the device's curl,
//! keep the raw YAML in /data/mihomo/profiles (0700), parse it to JSON.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::config::{mask_url, Fetched, Subscription, Usage};

const DIR: &str = "/data/mihomo/profiles";
const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Providers return a Clash/mihomo config only to a Clash-like client.
const USER_AGENT: &str = "clash.meta/mihomo";

pub fn path(id: &str) -> String {
    format!("{DIR}/sub-{id}.yaml")
}

pub fn remove(id: &str) {
    let _ = fs::remove_file(path(id));
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Download `sub`, check it parses as a mihomo config and install it as the
/// cached profile. The URL (it carries the account token) never appears in
/// errors or logs.
pub fn fetch(sub: &Subscription) -> Result<Fetched, String> {
    fs::create_dir_all(DIR).map_err(|e| format!("cannot create {DIR}: {e}"))?;
    let _ = fs::set_permissions(DIR, fs::Permissions::from_mode(0o700));
    let target = path(&sub.id);
    let part = format!("{target}.part");
    let headers = format!("{target}.headers");
    let max = MAX_BYTES.to_string();
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--max-redirs",
        "5",
        "--connect-timeout",
        "10",
        "--max-time",
        "40",
        "--max-filesize",
        &max,
        "--user-agent",
        USER_AGENT,
        "--dump-header",
        &headers,
        "--output",
        &part,
        "--",
        &sub.url,
    ]);
    let out = process_runner::output(&mut cmd, None, Duration::from_secs(45), 64 * 1024)
        .map_err(|e| format!("could not run curl: {e}"))?;
    let header_text = fs::read_to_string(&headers).unwrap_or_default();
    let _ = fs::remove_file(&headers);
    if !out.status.success() {
        let _ = fs::remove_file(&part);
        let msg = String::from_utf8_lossy(&out.stderr).replace(&sub.url, &mask_url(&sub.url));
        let msg = msg.trim().trim_start_matches("curl: ");
        return Err(format!(
            "download failed: {}",
            if msg.is_empty() { "unknown error" } else { msg }
        ));
    }
    let parsed = fs::read(&part)
        .map_err(|e| format!("cannot read the download: {e}"))
        .and_then(|bytes| parse_yaml(&bytes));
    let config = match parsed {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
    };
    let proxies = config["proxies"].as_array().map_or(0, Vec::len) as u32;
    let groups = config["proxy-groups"].as_array().map_or(0, Vec::len) as u32;
    let full = groups > 0 && config["rules"].is_array();
    if !full && proxies == 0 && !config["proxy-providers"].is_object() {
        let _ = fs::remove_file(&part);
        return Err("the subscription did not return a Clash/mihomo config".into());
    }
    fs::rename(&part, &target).map_err(|e| format!("cannot save the profile: {e}"))?;
    let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o600));
    Ok(Fetched {
        at: now_secs(),
        full,
        proxies,
        groups,
        usage: parse_userinfo(&header_text),
    })
}

pub fn load(id: &str) -> Result<Value, String> {
    let bytes = fs::read(path(id))
        .map_err(|_| "the subscription has not been downloaded yet".to_string())?;
    parse_yaml(&bytes)
}

/// YAML → JSON, with `<<` merge keys expanded (common in provider configs).
pub fn parse_yaml(bytes: &[u8]) -> Result<Value, String> {
    let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_slice(bytes)
        .map_err(|e| format!("the subscription is not valid YAML: {e}"))?;
    yaml.apply_merge()
        .map_err(|e| format!("invalid YAML merge key: {e}"))?;
    let value = serde_json::to_value(yaml)
        .map_err(|e| format!("unsupported YAML in the subscription: {e}"))?;
    if !value.is_object() {
        return Err("the subscription did not return a Clash/mihomo config".into());
    }
    Ok(value)
}

/// `subscription-userinfo: upload=1; download=2; total=3; expire=4` from the
/// last response (after redirects).
pub fn parse_userinfo(headers: &str) -> Option<Usage> {
    let line = headers
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case("subscription-userinfo"))
        .map(|(_, v)| v)
        .next_back()?;
    let mut usage = Usage::default();
    for part in line.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let v = v
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64);
        match k.trim().to_ascii_lowercase().as_str() {
            "upload" => usage.upload = v,
            "download" => usage.download = v,
            "total" => usage.total = v,
            "expire" => usage.expire = v,
            _ => {}
        }
    }
    (usage != Usage::default()).then_some(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userinfo_takes_the_last_response() {
        let h = "HTTP/1.1 302 Found\r\nLocation: x\r\n\r\nHTTP/2 200\r\n\
                 Subscription-Userinfo: upload=10; download=2e3; total=1073741824; expire=1900000000\r\n";
        let u = parse_userinfo(h).unwrap();
        assert_eq!(u.upload, Some(10));
        assert_eq!(u.download, Some(2000));
        assert_eq!(u.total, Some(1_073_741_824));
        assert_eq!(u.expire, Some(1_900_000_000));
        assert!(parse_userinfo("HTTP/2 200\r\ncontent-type: text/yaml\r\n").is_none());
    }

    #[test]
    fn yaml_merge_keys_and_unicode_names_survive() {
        let y = "base: &b {type: select, url: x}\nproxy-groups:\n  - {<<: *b, name: 节点选择}\nrules: [MATCH,节点选择]\n";
        let v = parse_yaml(y.as_bytes()).unwrap();
        assert_eq!(v["proxy-groups"][0]["type"], "select");
        assert_eq!(v["proxy-groups"][0]["name"], "节点选择");
        assert!(parse_yaml(b"- just\n- a list\n").is_err());
        assert!(parse_yaml(b"key: [unclosed").is_err());
    }
}
