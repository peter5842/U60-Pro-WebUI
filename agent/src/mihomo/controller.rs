//! Minimal HTTP/1.1 client for mihomo's controller on its unix socket.
//!
//! mihomo does not apply the `secret` on the unix socket, so the agent works
//! whatever secret the LAN-facing controller (metacubexd) uses. The agent
//! carries no HTTP client stack; a small std-only client with a deadline and a
//! response cap is enough.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::config::CONTROLLER_SOCKET;

const MAX_RESPONSE: usize = 4 * 1024 * 1024;

pub struct Controller;

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Result<Value, String> {
        if self.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&self.body).map_err(|e| format!("controller sent invalid JSON: {e}"))
    }

    /// mihomo reports failures as `{"message": "..."}`.
    pub fn error_message(&self) -> String {
        self.json()
            .ok()
            .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("controller returned HTTP {}", self.status))
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl Controller {
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<Reply, String> {
        let deadline = Instant::now() + timeout;
        let mut stream = UnixStream::connect(CONTROLLER_SOCKET)
            .map_err(|e| format!("mihomo controller unreachable: {e}"))?;
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: mihomo\r\n\
             Connection: close\r\nAccept: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        );
        if body.is_some() {
            head.push_str("Content-Type: application/json\r\n");
        }
        head.push_str("\r\n");
        set_deadline(&stream, deadline)?;
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(payload.as_bytes()))
            .map_err(|e| format!("controller write failed: {e}"))?;

        let mut raw = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            set_deadline(&stream, deadline)?;
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&buf[..n]);
                    if raw.len() > MAX_RESPONSE {
                        return Err("controller response too large".into());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("controller read failed: {e}")),
            }
        }
        parse_response(&raw)
    }

    pub fn get(&self, path: &str) -> Result<Value, String> {
        let reply = self.request("GET", path, None, Duration::from_secs(5))?;
        if !reply.ok() {
            return Err(reply.error_message());
        }
        reply.json()
    }
}

fn set_deadline(stream: &UnixStream, deadline: Instant) -> Result<(), String> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err("mihomo controller timed out".into());
    }
    stream
        .set_read_timeout(Some(left))
        .and_then(|_| stream.set_write_timeout(Some(left)))
        .map_err(|e| format!("controller socket error: {e}"))
}

fn parse_response(raw: &[u8]) -> Result<Reply, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("controller sent a truncated response")?;
    let head =
        std::str::from_utf8(&raw[..split]).map_err(|_| "controller sent non-UTF-8 headers")?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("controller sent a malformed status line")?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let rest = &raw[split + 4..];
    let body = if chunked {
        dechunk(rest)?
    } else {
        rest.to_vec()
    };
    Ok(Reply { status, body })
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("controller sent a truncated chunk")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk size")?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err("controller sent a truncated chunk".into());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

/// Percent-encode one path segment (proxy and group names may be any UTF-8,
/// including spaces, emoji flags and slashes).
pub fn segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length_and_chunked_bodies() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"meta\":true}";
        let r = parse_response(plain).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.json().unwrap()["meta"], true);

        let chunked = b"HTTP/1.1 503 Service Unavailable\r\nTransfer-Encoding: chunked\r\n\r\n\
                        7\r\n{\"messa\r\n8\r\nge\":\"x\"}\r\n0\r\n\r\n";
        let r = parse_response(chunked).unwrap();
        assert_eq!(r.status, 503);
        assert!(!r.ok());
        assert_eq!(r.error_message(), "x");
    }

    #[test]
    fn empty_204_is_null() {
        let r = parse_response(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
        assert!(r.ok());
        assert_eq!(r.json().unwrap(), Value::Null);
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n").is_err());
        assert!(
            parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nff\r\nabc")
                .is_err()
        );
    }

    #[test]
    fn segment_encodes_unicode_and_reserved() {
        assert_eq!(segment("PROXY"), "PROXY");
        assert_eq!(segment("HK 01/a"), "HK%2001%2Fa");
        assert_eq!(segment("🇭🇰"), "%F0%9F%87%AD%F0%9F%87%B0");
    }
}
