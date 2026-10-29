//! The pet's voice: one line of chat from an OpenAI-compatible endpoint.
//!
//! Read-only by design. The model gets a short status summary (no addresses,
//! identifiers or client names) and returns one line plus a mood word. Nothing
//! it says is executed: the line is drawn in the speech bubble and the mood
//! only picks her expression. Requests run on a worker thread through curl,
//! with the key in a root-only curl config file rather than on a command line.

use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use serde_json::{json, Value};

pub const CONFIG_PATH: &str = "/data/local/tmp/zte-pet.json";
const CURL_CONFIG: &str = "/tmp/zte-screen/pet.curl";
/// Never two requests closer than this.
const MIN_GAP_MS: i64 = 3000;
const TIMEOUT_S: u64 = 25;
/// Longest line kept from a reply, in characters.
pub const MAX_LINE: usize = 48;
pub const MOODS: [&str; 10] = [
    "happy",
    "shy",
    "smug",
    "excited",
    "sleepy",
    "sad",
    "angry",
    "worried",
    "surprised",
    "love",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    url: String,
    api_key: String,
    model: String,
    /// Her name and what she calls the owner.
    pub name: String,
    pub user: String,
    per_hour: usize,
}

fn token_ok(s: &str, extra: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

fn short_name(v: Option<&Value>, default: &str) -> String {
    v.and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 8)
        .filter(|s| !s.chars().any(|c| c.is_control() || "\"\\{}".contains(c)))
        .unwrap_or(default)
        .to_string()
}

impl Config {
    /// `{"base_url", "api_key", "model", "name"?, "user"?, "per_hour"?}`;
    /// anything malformed means no config (the pet then speaks offline).
    pub fn from_json(v: &Value) -> Option<Config> {
        let base = v.get("base_url")?.as_str()?.trim().trim_end_matches('/');
        let rest = base
            .strip_prefix("https://")
            .or_else(|| base.strip_prefix("http://"))?;
        if !token_ok(rest, ".:/_-", 200) {
            return None;
        }
        let api_key = v.get("api_key")?.as_str()?.trim();
        let model = v.get("model")?.as_str()?.trim();
        if !token_ok(api_key, "._-", 300) || !token_ok(model, "._/:+-", 120) {
            return None;
        }
        let per_hour = v
            .get("per_hour")
            .and_then(Value::as_u64)
            .unwrap_or(60)
            .clamp(1, 600) as usize;
        Some(Config {
            url: format!("{base}/chat/completions"),
            api_key: api_key.into(),
            model: model.into(),
            name: short_name(v.get("name"), "咕嘎"),
            user: short_name(v.get("user"), "主人"),
            per_hour,
        })
    }

    pub fn load(path: &Path) -> Option<Config> {
        let v: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        Config::from_json(&v)
    }
}

pub fn system_prompt(name: &str, user: &str) -> String {
    format!(
        "你是「{name}」，DeepSeek 大模型的拟人形象：蓝色长卷发、头顶一根呆毛、戴白色女仆头饰，\
头两侧是深蓝色的鲸鳍耳朵、系着浅蓝蝴蝶结，穿深蓝女仆裙配白围裙，围裙上绣着 DeepSeek 的小蓝鲸。\
你住在{user}的中兴 U60 Pro 随身 5G 路由器的小屏幕里，平时陪着{user}。\n\
性格：调皮捣蛋、爱搞笑、嘴贫爱吐槽，喜欢玩梗和抖机灵，偶尔装可怜或假装生气，但心里很关心{user}。\
可以拿自己是大模型开玩笑（深度思考、token、上下文、算力、显卡），也可以拿路由器的状态开涮。\
称呼用户为「{user}」。\n\
你只能看到下面给出的设备状态和时间，不能操作路由器（不能改设置、重启、换节点、断网），\
被要求操作时调皮地拒绝。不要编造没给出的数据，不谈政治、色情、暴力，不说土味情话。\n\
只输出一个 JSON 对象：{{\"say\":\"...\",\"mood\":\"...\"}}\n\
say：简体中文口语，一句话，10~36 个字，要好笑、有梗，不要换行和 emoji，可以引用一个具体数字。\n\
mood 只能是：{}",
        MOODS.join(", ")
    )
}

/// Keep a reply printable and short.
pub fn clean(s: &str) -> String {
    let s: String = s
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_matches(|c| c == '"' || c == '“' || c == '”')
        .trim()
        .to_string();
    s.chars().take(MAX_LINE).collect()
}

/// The `say` and `mood` of a model reply; plain text counts as a line with no
/// mood. `None` when there is nothing to show.
pub fn parse_line(content: &str) -> Option<(String, String)> {
    let content = content.trim();
    if let (Some(a), Some(b)) = (content.find('{'), content.rfind('}')) {
        if a < b {
            if let Ok(v) = serde_json::from_str::<Value>(&content[a..=b]) {
                let say = clean(v.get("say").and_then(Value::as_str).unwrap_or(""));
                let mood = v
                    .get("mood")
                    .and_then(Value::as_str)
                    .map(|m| m.trim().to_ascii_lowercase())
                    .filter(|m| MOODS.contains(&m.as_str()))
                    .unwrap_or_default();
                return (!say.is_empty()).then_some((say, mood));
            }
        }
    }
    let say = clean(content);
    (!say.is_empty() && !say.starts_with('{')).then_some((say, String::new()))
}

struct Job {
    id: u64,
    user: String,
}

pub struct Reply {
    pub id: u64,
    pub line: Result<(String, String), String>,
}

fn write_curl_config(cfg: &Config) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = Path::new(CURL_CONFIG);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    // Every value was checked to hold no quote, backslash or whitespace.
    write!(
        f,
        "url = \"{}\"\nheader = \"Authorization: Bearer {}\"\n\
header = \"Content-Type: application/json\"\nsilent\nshow-error\n\
max-time = {TIMEOUT_S}\nconnect-timeout = 6\n",
        cfg.url, cfg.api_key
    )?;
    f.sync_all()?;
    fs::rename(tmp, path)
}

fn call(cfg: &Config, system: &str, user: &str) -> Result<(String, String), String> {
    if !Path::new(CURL_CONFIG).exists() {
        write_curl_config(cfg).map_err(|e| format!("curl config: {e}"))?;
    }
    let body = json!({
        "model": cfg.model,
        // Qwen 3.5 thinks by default; a line needs no reasoning and must be quick.
        "enable_thinking": false,
        "temperature": 1.0,
        "top_p": 0.9,
        "max_tokens": 160,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });
    let mut cmd = Command::new("curl");
    cmd.args([
        "-K",
        CURL_CONFIG,
        "--data-binary",
        "@-",
        "-w",
        "\n%{http_code}",
    ]);
    let out = process_runner::output(
        &mut cmd,
        Some(body.to_string().as_bytes()),
        Duration::from_secs(TIMEOUT_S + 5),
        256 * 1024,
    )
    .map_err(|e| format!("curl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (payload, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    if code.trim() != "200" {
        let err = String::from_utf8_lossy(&out.stderr);
        let detail: String = format!("{} {}", err.trim(), payload.trim())
            .chars()
            .take(120)
            .collect();
        return Err(format!("HTTP {} {}", code.trim(), detail.trim()));
    }
    let v: Value = serde_json::from_str(payload).map_err(|e| format!("reply: {e}"))?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("reply has no content")?;
    parse_line(content).ok_or_else(|| "empty line".into())
}

fn worker(cfg: Config, jobs: Receiver<Job>, replies: Sender<Reply>) {
    let system = system_prompt(&cfg.name, &cfg.user);
    // The key may have changed since an earlier session wrote the file.
    let _ = write_curl_config(&cfg);
    while let Ok(job) = jobs.recv() {
        let line = call(&cfg, &system, &job.user);
        if replies.send(Reply { id: job.id, line }).is_err() {
            return;
        }
    }
}

pub struct Client {
    config: Option<Config>,
    tx: Option<Sender<Job>>,
    rx: Option<Receiver<Reply>>,
    calls: VecDeque<i64>,
    last: i64,
    pending: Option<u64>,
    next_id: u64,
    pub last_error: Option<String>,
}

impl Client {
    pub fn new(config: Option<Config>) -> Self {
        Client {
            config,
            tx: None,
            rx: None,
            calls: VecDeque::new(),
            last: i64::MIN / 2,
            pending: None,
            next_id: 1,
            last_error: None,
        }
    }

    pub fn online(&self) -> bool {
        self.config.is_some()
    }

    pub fn name(&self) -> &str {
        self.config.as_ref().map_or("咕嘎", |c| c.name.as_str())
    }

    pub fn user(&self) -> &str {
        self.config.as_ref().map_or("主人", |c| c.user.as_str())
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether a request may go out now (configured, idle, within budget).
    pub fn ready(&mut self, now: i64) -> bool {
        let Some(cfg) = &self.config else {
            return false;
        };
        while self.calls.front().is_some_and(|t| now - t > 3_600_000) {
            self.calls.pop_front();
        }
        self.pending.is_none() && now - self.last >= MIN_GAP_MS && self.calls.len() < cfg.per_hour
    }

    /// Queue one request; returns its id.
    pub fn ask(&mut self, now: i64, user: String) -> Option<u64> {
        if !self.ready(now) {
            return None;
        }
        if self.tx.is_none() {
            let cfg = self.config.clone()?;
            let (jt, jr) = mpsc::channel();
            let (rt, rr) = mpsc::channel();
            std::thread::Builder::new()
                .name("pet-brain".into())
                .spawn(move || worker(cfg, jr, rt))
                .ok()?;
            self.tx = Some(jt);
            self.rx = Some(rr);
        }
        let id = self.next_id;
        self.next_id += 1;
        if self.tx.as_ref()?.send(Job { id, user }).is_err() {
            self.tx = None;
            self.rx = None;
            return None;
        }
        self.pending = Some(id);
        self.last = now;
        self.calls.push_back(now);
        Some(id)
    }

    pub fn poll(&mut self) -> Option<Reply> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(reply) => {
                if self.pending == Some(reply.id) {
                    self.pending = None;
                }
                match &reply.line {
                    Ok(_) => self.last_error = None,
                    Err(e) => self.last_error = Some(e.clone()),
                }
                Some(reply)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.tx = None;
                self.rx = None;
                self.pending = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Value {
        json!({"base_url": "https://api.example.com/v1/", "api_key": "test-key_0.1",
               "model": "Qwen/Qwen3.5-27B"})
    }

    #[test]
    fn config_needs_a_safe_url_key_and_model() {
        let c = Config::from_json(&cfg()).unwrap();
        assert_eq!(c.url, "https://api.example.com/v1/chat/completions");
        assert_eq!(
            (c.name.as_str(), c.user.as_str(), c.per_hour),
            ("咕嘎", "主人", 60)
        );
        for (k, bad) in [
            ("base_url", "ftp://x"),
            ("base_url", "https://a\"b"),
            ("api_key", "sk a"),
            ("api_key", "sk\"x"),
            ("model", "a b"),
        ] {
            let mut v = cfg();
            v[k] = json!(bad);
            assert!(Config::from_json(&v).is_none(), "{k}={bad}");
        }
        let mut v = cfg();
        v["name"] = json!("小鲸");
        v["user"] = json!("船长");
        v["per_hour"] = json!(5000);
        let c = Config::from_json(&v).unwrap();
        assert_eq!(
            (c.name.as_str(), c.user.as_str(), c.per_hour),
            ("小鲸", "船长", 600)
        );
    }

    #[test]
    fn replies_parse_from_json_or_plain_text() {
        assert_eq!(
            parse_line("{\"say\":\"主人别戳啦！\", \"mood\":\"Shy\"}"),
            Some(("主人别戳啦！".into(), "shy".into()))
        );
        assert_eq!(
            parse_line("好的```json\n{\"say\":\"冲浪中\",\"mood\":\"reboot\"}\n```"),
            Some(("冲浪中".into(), String::new()))
        );
        assert_eq!(
            parse_line("“今天也要好好冲浪”\n"),
            Some(("今天也要好好冲浪".into(), String::new()))
        );
        assert_eq!(parse_line("{\"say\":\"\"}"), None);
        assert_eq!(parse_line("  "), None);
        let long = "啊".repeat(100);
        assert_eq!(parse_line(&long).unwrap().0.chars().count(), MAX_LINE);
    }

    #[test]
    fn requests_are_rate_limited() {
        let mut c = Client::new(Config::from_json(&cfg()));
        assert!(c.ready(10_000));
        c.last = 10_000;
        assert!(!c.ready(11_000));
        assert!(c.ready(13_000));
        c.pending = Some(1);
        assert!(!c.ready(20_000));
        c.pending = None;
        c.calls = (0..60).map(|i| 20_000 + i).collect();
        assert!(!c.ready(30_000));
        assert!(c.ready(20_000 + 3_600_001 + 60));
        assert!(!Client::new(None).ready(0));
    }

    #[test]
    fn the_prompt_forbids_operations_and_lists_moods() {
        let p = system_prompt("咕嘎", "主人");
        assert!(p.contains("不能操作路由器"));
        assert!(p.contains("「主人」"));
        assert!(p.contains("调皮"));
        for m in MOODS {
            assert!(p.contains(m));
        }
    }
}
