//! Drawing the PROXY page and its taps. Four screens: the overview (mode,
//! traffic, groups), one group's nodes, the busiest connections, and the
//! subscriptions with the service. Choosing a node takes effect at once;
//! anything that changes the agent's config asks first.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::canvas::{Canvas, W};
use crate::font::Size::Small;
use crate::stats::Model;
use crate::ui::{bits, bytes, truncate, Palette, TAB_Y, TOP};

use super::jobs::mode_name;
use super::{AgentOp, Group, Jobs, Live, Sub};

const BAR_Y: i32 = TAB_Y - 34;
const BAR_H: i32 = 30;
const TOAST_Y: i32 = BAR_Y - 20;
/// The header row of the inner screens, and where their lists start.
const HEAD_H: i32 = 26;
const LIST_Y: i32 = TOP + HEAD_H + 8;

const GROUP_Y: i32 = TOP + 110;
const GROUP_ROW: i32 = 28;
const GROUP_ROWS: usize = ((TOAST_Y - GROUP_Y) / GROUP_ROW) as usize;
const NODE_ROW: i32 = 26;
const NODE_ROWS: usize = ((TOAST_Y - LIST_Y) / NODE_ROW) as usize;
const CONN_ROW: i32 = 36;
const CONN_ROWS: usize = ((TOAST_Y - LIST_Y) / CONN_ROW) as usize;
const SUB_Y: i32 = LIST_Y + 62;
const SUB_ROW: i32 = 46;
const SUB_ROWS: usize = ((TOAST_Y - SUB_Y) / SUB_ROW) as usize;

// Enough rows for the lists, and the bar clear of the tabs.
const _: () = assert!(GROUP_ROWS >= 6 && NODE_ROWS >= 10 && CONN_ROWS >= 7 && SUB_ROWS >= 4);
const _: () = assert!(BAR_Y + BAR_H < TAB_Y);

const MODES: [&str; 3] = ["rule", "global", "direct"];

/// The confirmation box: (x, y, w, h), and its two buttons.
const ASK: (i32, i32, i32, i32) = (18, 140, W - 36, 160);
const ASK_NO: (i32, i32, i32, i32) = (34, 254, 112, 32);
const ASK_YES: (i32, i32, i32, i32) = (W - 34 - 112, 254, 112, 32);

#[derive(Clone, Debug, PartialEq)]
enum Screen {
    Main,
    Group(String),
    Conns,
    Subs,
}

struct Confirm {
    question: String,
    detail: String,
    op: AgentOp,
}

pub struct View {
    screen: Screen,
    page: usize,
    confirm: Option<Confirm>,
    pub jobs: Jobs,
}

impl Default for View {
    fn default() -> Self {
        View {
            screen: Screen::Main,
            page: 0,
            confirm: None,
            jobs: Jobs::default(),
        }
    }
}

// ---- small drawing helpers ---------------------------------------------------

fn inside(x: i32, y: i32, r: (i32, i32, i32, i32)) -> bool {
    (r.0..r.0 + r.2).contains(&x) && (r.1..r.1 + r.3).contains(&y)
}

/// Keep what the 16 px fonts can draw (node names often carry emoji flags),
/// then cut to `max` pixels with a `~`.
fn fit(s: &str, max: i32) -> String {
    let clean: String = s
        .chars()
        .filter(|c| crate::cjk::has(*c))
        .collect::<String>()
        .trim()
        .to_string();
    if crate::cjk::width(&clean) <= max {
        return clean;
    }
    let mut out = String::new();
    let mut w = 8;
    for c in clean.chars() {
        let cw = crate::cjk::char_width(c);
        if w + cw > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('~');
    out
}

/// Mixed text ending at `right`; returns where it starts.
fn text_right(c: &mut Canvas, right: i32, y: i32, col: u16, s: &str) -> i32 {
    let x = right - crate::cjk::width(s);
    c.text_cjk(x, y, col, s);
    x
}

fn button(
    c: &mut Canvas,
    r: (i32, i32, i32, i32),
    label: &str,
    on: bool,
    enabled: bool,
    p: &Palette,
) {
    let (x, y, w, h) = r;
    let (frame, text) = if on {
        c.fill_rect(x, y, w, h, p.fg);
        (p.fg, p.bg)
    } else if enabled {
        (p.dim, p.bright)
    } else {
        (p.faint, p.faint)
    };
    c.frame(x, y, w, h, frame);
    let tw = crate::cjk::width(label);
    c.text_cjk(x + (w - tw) / 2, y + (h - 16) / 2, text, label);
}

/// A row of equal buttons along the bottom; returns the rectangles.
fn bar_rects(n: usize) -> Vec<(i32, i32, i32, i32)> {
    let gap = 4;
    let w = (W - 8 - gap * (n as i32 - 1)) / n as i32;
    (0..n as i32)
        .map(|i| (4 + i * (w + gap), BAR_Y, w, BAR_H))
        .collect()
}

fn bar(c: &mut Canvas, labels: &[(&str, bool)], p: &Palette) {
    for (r, (label, enabled)) in bar_rects(labels.len()).into_iter().zip(labels) {
        button(c, r, label, false, *enabled, p);
    }
}

fn tapped_bar(x: i32, y: i32, n: usize) -> Option<usize> {
    bar_rects(n).iter().position(|r| inside(x, y, *r))
}

fn back_rect() -> (i32, i32, i32, i32) {
    (4, TOP + 2, 56, HEAD_H)
}

fn header(c: &mut Canvas, title: &str, extra: &str, p: &Palette) {
    button(c, back_rect(), "返回", false, true, p);
    let x = c.text_cjk(68, TOP + 7, p.bright, &fit(title, 140));
    c.text_cjk(x + 8, TOP + 7, p.dim, &fit(extra, W - x - 12));
    c.dotted(0, TOP + HEAD_H + 4, W, 2, p.faint);
}

fn delay_text(d: Option<u32>, p: &Palette) -> (String, u16) {
    match d {
        None => ("--".into(), p.faint),
        Some(0) => ("超时".into(), p.crit),
        Some(ms) if ms < 200 => (format!("{ms}ms"), p.ok),
        Some(ms) if ms < 500 => (format!("{ms}ms"), p.warn),
        Some(ms) => (format!("{ms}ms"), p.crit),
    }
}

fn kind_name(kind: &str) -> &'static str {
    match kind {
        "Selector" => "手选",
        "URLTest" => "自动测速",
        "Fallback" => "故障转移",
        "LoadBalance" => "负载均衡",
        "Relay" => "链式",
        _ => "",
    }
}

fn pages(len: usize, per: usize) -> usize {
    len.div_ceil(per.max(1)).max(1)
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of an RFC 3339 time ("2026-10-29T09:20:46.26+08:00").
pub fn epoch_of(t: &str) -> Option<i64> {
    let num = |a: usize, b: usize| t.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, s) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let rest = t.get(19..)?;
    let zone = rest
        .find(['Z', '+', '-'])
        .map(|i| &rest[i..])
        .unwrap_or("Z");
    let offset = match zone.as_bytes().first() {
        Some(b'+') | Some(b'-') => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let oh = zone.get(1..3)?.parse::<i64>().ok()?;
            let om = zone.get(4..6)?.parse::<i64>().ok()?;
            sign * (oh * 3600 + om * 60)
        }
        _ => 0,
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s - offset)
}

fn ago(secs: i64) -> String {
    match secs.max(0) {
        s if s < 3600 => format!("{}分钟前", s / 60),
        s if s < 86_400 => format!("{}小时前", s / 3600),
        s => format!("{}天前", s / 86_400),
    }
}

/// "12-01" for a unix time.
fn month_day(t: i64) -> String {
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{m:02}-{d:02}")
}

// ---- the view ------------------------------------------------------------------

impl View {
    /// A screen of the page for `--preview`.
    pub fn demo(screen: &str) -> View {
        let mut v = View::default();
        let sub = |id: &str, name: &str, nodes, error: Option<&str>| Sub {
            id: id.into(),
            name: name.into(),
            enabled: true,
            nodes: Some(nodes),
            updated: Some("2026-10-29T01:00:00Z".into()),
            used: Some(126_000_000_000),
            total: Some(500_000_000_000),
            expire: Some(1_798_000_000),
            error: error.map(str::to_string),
        };
        v.jobs.show(
            vec![
                sub("a1", "主力机场", 31, None),
                sub("b2", "备用机场", 12, Some("timeout")),
            ],
            if screen == "group" {
                "节点选择 → HK-IEPL-01"
            } else {
                ""
            },
        );
        v.screen = match screen {
            "group" => Screen::Group("节点选择".into()),
            "conns" => Screen::Conns,
            "subs" => Screen::Subs,
            _ => Screen::Main,
        };
        if screen == "confirm" {
            v.ask(
                "切换到全局模式？".into(),
                "所有流量都走 GLOBAL 组，现在选的是「节点选择」。",
                AgentOp::Mode("global"),
            );
        }
        v
    }

    /// mihomo as the previews show it.
    pub fn sample_live() -> Live {
        let g = |name: &str, kind: &str, now: &str, all: &[&str]| Group {
            name: name.into(),
            kind: kind.into(),
            now: now.into(),
            all: all.iter().map(|s| s.to_string()).collect(),
        };
        let nodes = [
            "HK-IEPL-01",
            "HK-IEPL-02",
            "SG-01",
            "JP-Tokyo-03",
            "US-West-02",
            "US-AI-01",
        ];
        let mut live = Live {
            up: true,
            version: "v1.19.32".into(),
            mode: "rule".into(),
            global_now: "节点选择".into(),
            groups: vec![
                g(
                    "节点选择",
                    "Selector",
                    "自动选择",
                    &[
                        "自动选择",
                        "香港节点",
                        "美国节点",
                        nodes[0],
                        nodes[2],
                        "DIRECT",
                    ],
                ),
                g(
                    "自动选择",
                    "Fallback",
                    "香港节点",
                    &["香港节点", "美国节点", "DIRECT"],
                ),
                g("香港节点", "URLTest", "HK-IEPL-01", &nodes[..2]),
                g("美国节点", "URLTest", "US-West-02", &nodes[4..]),
                g("Claude", "Selector", "US-AI-01", &nodes[4..]),
                g(
                    "国外媒体",
                    "Selector",
                    "香港节点",
                    &["香港节点", "美国节点", "节点选择"],
                ),
                g("苹果服务", "Selector", "DIRECT", &["DIRECT", "节点选择"]),
                g("国内直连", "Selector", "DIRECT", &["DIRECT", "节点选择"]),
                g("漏网之鱼", "Selector", "节点选择", &["节点选择", "DIRECT"]),
            ],
            connections: 177,
            down_rate: 1_250_000.0,
            up_rate: 310_000.0,
            ..Live::default()
        };
        for (n, d) in nodes.iter().zip([81, 96, 142, 0, 187, 287]) {
            live.delays.insert(n.to_string(), d);
        }
        for i in 0..60 {
            let t = i as f32 / 6.0;
            crate::stats::push(&mut live.down_hist, 600_000.0 + 500_000.0 * t.sin().abs());
            crate::stats::push(
                &mut live.up_hist,
                150_000.0 + 120_000.0 * (t * 1.7).cos().abs(),
            );
        }
        let c = |host: &str, node: &str, group: &str, rule: &str, d: f64| super::Conn {
            host: host.into(),
            node: node.into(),
            group: group.into(),
            rule: rule.into(),
            down_rate: d,
            up_rate: d / 5.0,
        };
        live.conns = vec![
            c(
                "rr3---sn-a5mekn6s.googlevideo.com",
                "HK-IEPL-01",
                "国外媒体",
                "RuleSet(YouTube)",
                980_000.0,
            ),
            c(
                "api.anthropic.com",
                "US-AI-01",
                "Claude",
                "RuleSet(Claude)",
                42_000.0,
            ),
            c(
                "github.com",
                "HK-IEPL-01",
                "节点选择",
                "RuleSet(Github)",
                12_400.0,
            ),
            c(
                "mmbiz.qpic.cn",
                "DIRECT",
                "DIRECT",
                "RuleSet(ChinaMax)",
                8_100.0,
            ),
            c(
                "time.apple.com",
                "DIRECT",
                "苹果服务",
                "RuleSet(Apple)",
                0.0,
            ),
        ];
        live
    }

    fn delay(&self, live: &Live, name: &str) -> Option<u32> {
        self.jobs
            .fresh_delay(name)
            .or_else(|| live.delays.get(name).copied())
    }

    /// Once a second: collect agent replies, and load the subscription list
    /// when its screen is showing and the list is missing or stale.
    pub fn tick(&mut self, showing: bool) {
        self.jobs.poll();
        if showing
            && self.screen == Screen::Subs
            && self.jobs.subs().is_none()
            && self.jobs.busy().is_none()
        {
            self.jobs.ask(AgentOp::Subs);
        }
    }

    fn go(&mut self, screen: Screen) {
        if screen == Screen::Subs {
            self.jobs.forget_subs();
        }
        self.screen = screen;
        self.page = 0;
    }

    pub fn render(&self, c: &mut Canvas, m: &Model, p: &Palette) {
        let live = &m.live;
        match &self.screen {
            Screen::Main => self.main(c, live, p),
            Screen::Group(name) => self.group(c, live, name, p),
            Screen::Conns => self.conns(c, live, p),
            Screen::Subs => self.subs(c, m, p),
        }
        // The status line: work in progress, then the last result.
        if let Some(busy) = self.jobs.busy() {
            let spin = ["|", "/", "-", "\\"][(crate::pet::clock() / 250 % 4) as usize];
            let x = c.text(4, TOAST_Y, Small, p.accent, spin);
            c.text_cjk(x + 6, TOAST_Y, p.accent, &fit(&busy, W - x - 10));
        } else if let Some((text, ok)) = self.jobs.toast() {
            c.text_cjk(
                4,
                TOAST_Y,
                if ok { p.ok } else { p.crit },
                &fit(&text, W - 8),
            );
        }
        if let Some(ask) = &self.confirm {
            let (x, y, w, h) = ASK;
            c.fill_rect(x, y, w, h, p.bg);
            c.frame(x, y, w, h, p.warn);
            c.frame(x + 2, y + 2, w - 4, h - 4, p.faint);
            let mut ty = y + 14;
            for line in crate::cjk::wrap(&ask.question, w - 28) {
                c.text_cjk(x + 14, ty, p.bright, &line);
                ty += 18;
            }
            ty += 4;
            for line in crate::cjk::wrap(&ask.detail, w - 28) {
                c.text_cjk(x + 14, ty, p.dim, &line);
                ty += 18;
            }
            button(c, ASK_NO, "取消", false, true, p);
            let (bx, by, bw, bh) = ASK_YES;
            c.fill_rect(bx, by, bw, bh, p.warn);
            let tw = crate::cjk::width("确认");
            c.text_cjk(bx + (bw - tw) / 2, by + (bh - 16) / 2, p.bg, "确认");
        }
    }

    fn main(&self, c: &mut Canvas, live: &Live, p: &Palette) {
        // Mode switch.
        let mw = (W - 8 - 8) / 3;
        for (i, mode) in MODES.iter().enumerate() {
            let r = (4 + i as i32 * (mw + 4), TOP + 2, mw, HEAD_H);
            button(c, r, mode_name(mode), live.mode == *mode, live.up, p);
        }
        let mut y = TOP + 36;
        if !live.up {
            c.text_cjk(4, y, p.crit, "mihomo 没有运行，或还没读到");
            c.text_cjk(4, y + 20, p.dim, "可以在「订阅」里查看服务状态");
            bar(c, &[("订阅", true)], p);
            return;
        }
        // Traffic, then its last minute.
        let x = c.text(4, y, Small, p.accent, "↓ ");
        let x = c.text(
            x,
            y,
            Small,
            p.bright,
            &format!("{}b/s", bits(live.down_rate)),
        );
        let x = c.text(x + 10, y, Small, p.accent2, "↑ ");
        c.text(x, y, Small, p.bright, &format!("{}b/s", bits(live.up_rate)));
        c.text_right(
            W - 4,
            y,
            Small,
            p.dim,
            &format!("{} conn >", live.connections),
        );
        y += 20;
        c.frame(4, y, W - 8, 36, p.faint);
        let max = live
            .down_hist
            .iter()
            .chain(live.up_hist.iter())
            .fold(1.0f32, |a, b| a.max(*b));
        let dl: Vec<f32> = live.down_hist.iter().copied().collect();
        let ul: Vec<f32> = live.up_hist.iter().copied().collect();
        c.plot(5, y + 2, W - 10, 32, &dl, max, p.accent, p.grid);
        c.plot(5, y + 2, W - 10, 32, &ul, max, p.accent2, p.accent2);
        // Groups, a page at a time.
        y = GROUP_Y - 20;
        let n = pages(live.groups.len(), GROUP_ROWS);
        let x = c.text(4, y, Small, p.dim, "# ");
        let x = c.text(
            x,
            y,
            Small,
            p.bright,
            &format!("groups ({})", live.groups.len()),
        );
        let right = c.text_right(
            W - 4,
            y,
            Small,
            p.dim,
            &format!("{}/{n}", self.page.min(n - 1) + 1),
        );
        c.dotted(x + 6, y + 8, right - x - 12, 3, p.faint);
        let start = self.page.min(n - 1) * GROUP_ROWS;
        for (i, g) in live.groups.iter().skip(start).take(GROUP_ROWS).enumerate() {
            let ry = GROUP_Y + i as i32 * GROUP_ROW;
            c.text_cjk(4, ry + 5, p.bright, &fit(&g.name, 84));
            let leaf = live.leaf(&g.name);
            let (d, dc) = delay_text(self.delay(live, leaf), p);
            let dx = text_right(c, W - 14, ry + 5, dc, &d);
            let now = if g.now == leaf || g.now.is_empty() {
                g.now.clone()
            } else {
                format!("{} > {leaf}", g.now)
            };
            c.text_cjk(
                96,
                ry + 5,
                if g.manual() { p.fg } else { p.dim },
                &fit(&now, dx - 104),
            );
            c.text(W - 10, ry + 5, Small, p.faint, ">");
            c.dotted(4, ry + GROUP_ROW - 2, W - 8, 4, p.grid);
        }
        bar(
            c,
            &[
                ("上页", self.page > 0),
                ("下页", self.page + 1 < n),
                ("测速", self.jobs.busy().is_none()),
                ("连接", true),
                ("订阅", true),
            ],
            p,
        );
    }

    fn group(&self, c: &mut Canvas, live: &Live, name: &str, p: &Palette) {
        let Some(g) = live.group(name) else {
            header(c, name, "", p);
            c.text_cjk(4, LIST_Y, p.dim, "这个组已经不在配置里了");
            bar(c, &[("返回", true)], p);
            return;
        };
        header(
            c,
            &g.name,
            &format!("{} · {}", kind_name(&g.kind), g.all.len()),
            p,
        );
        let n = pages(g.all.len(), NODE_ROWS);
        let start = self.page.min(n - 1) * NODE_ROWS;
        for (i, node) in g.all.iter().skip(start).take(NODE_ROWS).enumerate() {
            let y = LIST_Y + i as i32 * NODE_ROW;
            let chosen = *node == g.now;
            if chosen {
                c.fill_rect(4, y + 3, 3, NODE_ROW - 6, p.accent);
            }
            // A member that is a group shows where it leads.
            let (d, dc) = delay_text(self.delay(live, live.leaf(node)), p);
            let dx = text_right(c, W - 6, y + 5, dc, &d);
            let label = match live.group(node) {
                Some(sub) => format!("{node} > {}", live.leaf(&sub.name)),
                None => node.clone(),
            };
            c.text_cjk(
                14,
                y + 5,
                if chosen { p.bright } else { p.fg },
                &fit(&label, dx - 22),
            );
            c.dotted(14, y + NODE_ROW - 2, W - 20, 4, p.grid);
        }
        bar(
            c,
            &[
                ("上页", self.page > 0),
                ("下页", self.page + 1 < n),
                ("测速", self.jobs.busy().is_none()),
                ("返回", true),
            ],
            p,
        );
    }

    fn conns(&self, c: &mut Canvas, live: &Live, p: &Palette) {
        header(
            c,
            &format!("连接 {}", live.connections),
            &format!("↓{} ↑{}", bits(live.down_rate), bits(live.up_rate)),
            p,
        );
        let n = pages(live.conns.len(), CONN_ROWS);
        let start = self.page.min(n - 1) * CONN_ROWS;
        if live.conns.is_empty() {
            c.text_cjk(4, LIST_Y, p.dim, "现在没有连接");
        }
        for (i, k) in live.conns.iter().skip(start).take(CONN_ROWS).enumerate() {
            let y = LIST_Y + i as i32 * CONN_ROW;
            let rate = k.down_rate + k.up_rate;
            let rx = c.text_right(
                W - 4,
                y,
                Small,
                if rate > 0.0 { p.fg } else { p.faint },
                &format!("{}b/s", bits(rate)),
            );
            c.text(
                4,
                y,
                Small,
                p.bright,
                &truncate(&k.host, ((rx - 12) / 8) as usize),
            );
            let route = if k.group.is_empty() || k.group == k.node {
                k.node.clone()
            } else {
                format!("{} > {}", k.group, k.node)
            };
            let x = c.text_cjk(4, y + 17, p.accent, &fit(&route, 170));
            c.text_cjk(x + 8, y + 17, p.dim, &fit(&k.rule, W - x - 12));
        }
        bar(
            c,
            &[
                ("上页", self.page > 0),
                ("下页", self.page + 1 < n),
                ("返回", true),
            ],
            p,
        );
    }

    fn restart_rect() -> (i32, i32, i32, i32) {
        (W - 76, LIST_Y, 72, 34)
    }

    fn update_rect(i: usize) -> (i32, i32, i32, i32) {
        (W - 64, SUB_Y + i as i32 * SUB_ROW + 4, 60, 34)
    }

    fn subs(&self, c: &mut Canvas, m: &Model, p: &Palette) {
        let live = &m.live;
        header(c, "订阅与服务", "", p);
        let y = LIST_Y;
        let x = c.text(4, y, Small, p.bright, "mihomo ");
        c.text(
            x,
            y,
            Small,
            p.fg,
            if live.version.is_empty() {
                "?"
            } else {
                &live.version
            },
        );
        let (state, col) = match (&m.mihomo, live.up) {
            (Some(_), true) => ("运行中", p.ok),
            (Some(_), false) => ("启动中", p.warn),
            (None, _) => ("未运行", p.crit),
        };
        c.text_cjk(4, y + 18, col, state);
        if let Some(proc_) = &m.mihomo {
            c.text(
                60,
                y + 18,
                Small,
                p.dim,
                &format!("cpu {:.0}% rss {}", proc_.cpu, bytes(proc_.rss_kb * 1024)),
            );
        }
        button(
            c,
            Self::restart_rect(),
            "重启",
            false,
            self.jobs.busy().is_none(),
            p,
        );
        let sy = SUB_Y - 20;
        let x = c.text(4, sy, Small, p.dim, "# ");
        let x = c.text(x, sy, Small, p.bright, "subscriptions");
        c.dotted(x + 6, sy + 8, W - x - 10, 3, p.faint);
        let subs: Vec<Sub> = match self.jobs.subs() {
            None => {
                c.text_cjk(4, SUB_Y + 4, p.dim, "正在向 agent 读取…");
                Vec::new()
            }
            Some(Err(e)) => {
                c.text_cjk(4, SUB_Y + 4, p.crit, &fit(&format!("读取失败：{e}"), W - 8));
                Vec::new()
            }
            Some(Ok(list)) => {
                if list.is_empty() {
                    c.text_cjk(4, SUB_Y + 4, p.dim, "还没有订阅（在网页后台添加）");
                }
                list
            }
        };
        let n = pages(subs.len(), SUB_ROWS);
        let start = self.page.min(n - 1) * SUB_ROWS;
        let now = now_epoch();
        for (i, s) in subs.iter().skip(start).take(SUB_ROWS).enumerate() {
            let y = SUB_Y + i as i32 * SUB_ROW;
            let x = c.text_cjk(
                4,
                y + 4,
                if s.enabled { p.bright } else { p.dim },
                &fit(&s.name, 140),
            );
            if let Some(n) = s.nodes {
                c.text_cjk(x + 8, y + 4, p.dim, &format!("{n}节点"));
            }
            let detail = if let Some(e) = &s.error {
                (format!("错误：{e}"), p.crit)
            } else {
                let mut parts = Vec::new();
                if !s.enabled {
                    parts.push("已停用".to_string());
                }
                if let Some(t) = s.updated.as_deref().and_then(epoch_of) {
                    parts.push(format!("{}更新", ago(now - t)));
                }
                match (s.used, s.total) {
                    (Some(u), Some(t)) => parts.push(format!("{}/{}", bytes(u), bytes(t))),
                    (Some(u), None) => parts.push(format!("已用{}", bytes(u))),
                    _ => {}
                }
                if let Some(e) = s.expire {
                    parts.push(format!("到{}", month_day(e as i64)));
                }
                (parts.join(" "), p.dim)
            };
            c.text_cjk(4, y + 22, detail.1, &fit(&detail.0, W - 76));
            button(
                c,
                Self::update_rect(i),
                "更新",
                false,
                s.enabled && self.jobs.busy().is_none(),
                p,
            );
            c.dotted(4, y + SUB_ROW - 3, W - 8, 4, p.grid);
        }
        bar(
            c,
            &[
                ("全部更新", self.jobs.busy().is_none()),
                ("刷新", self.jobs.busy().is_none()),
                ("返回", true),
            ],
            p,
        );
    }

    // ---- taps ----------------------------------------------------------------------

    fn ask(&mut self, question: String, detail: &str, op: AgentOp) {
        self.confirm = Some(Confirm {
            question,
            detail: detail.to_string(),
            op,
        });
    }

    /// A tap on the page. Returns true when something changed.
    pub fn tap(&mut self, x: i32, y: i32, m: &Model) -> bool {
        if let Some(ask) = self.confirm.take() {
            if inside(x, y, ASK_YES) {
                self.jobs.ask(ask.op);
            } else if !inside(x, y, ASK_NO) && inside(x, y, ASK) {
                // A tap inside the box but on neither button keeps it open.
                self.confirm = Some(ask);
                return false;
            }
            return true;
        }
        let live = &m.live;
        match self.screen.clone() {
            Screen::Main => self.tap_main(x, y, live),
            Screen::Group(name) => self.tap_group(x, y, live, &name),
            Screen::Conns => {
                let n = pages(live.conns.len(), CONN_ROWS);
                self.tap_list(x, y, n, 3)
            }
            Screen::Subs => self.tap_subs(x, y),
        }
    }

    /// Back, and the paging buttons shared by the inner screens. `n` pages,
    /// `buttons` in the bar with back last.
    fn tap_list(&mut self, x: i32, y: i32, n: usize, buttons: usize) -> bool {
        if inside(x, y, back_rect()) || tapped_bar(x, y, buttons) == Some(buttons - 1) {
            self.go(Screen::Main);
            return true;
        }
        match tapped_bar(x, y, buttons) {
            Some(0) if self.page > 0 => self.page -= 1,
            Some(1) if self.page + 1 < n => self.page += 1,
            _ => return false,
        }
        true
    }

    fn tap_main(&mut self, x: i32, y: i32, live: &Live) -> bool {
        if !live.up {
            if tapped_bar(x, y, 1) == Some(0) {
                self.go(Screen::Subs);
                return true;
            }
            return false;
        }
        let mw = (W - 8 - 8) / 3;
        for (i, mode) in MODES.iter().enumerate() {
            if inside(x, y, (4 + i as i32 * (mw + 4), TOP + 2, mw, HEAD_H)) {
                if live.mode == *mode {
                    return false;
                }
                let detail = match *mode {
                    "rule" => "按规则分流：国内直连，其余按策略组走。".to_string(),
                    "global" => format!(
                        "所有流量都走 GLOBAL 组，现在选的是「{}」。",
                        live.global_now
                    ),
                    _ => "所有流量直连，不经过任何节点。".to_string(),
                };
                self.ask(
                    format!("切换到{}模式？", mode_name(mode)),
                    &detail,
                    AgentOp::Mode(mode),
                );
                return true;
            }
        }
        if (TOP + 34..GROUP_Y - 22).contains(&y) {
            self.go(Screen::Conns);
            return true;
        }
        let n = pages(live.groups.len(), GROUP_ROWS);
        match tapped_bar(x, y, 5) {
            Some(0) if self.page > 0 => self.page -= 1,
            Some(1) if self.page + 1 < n => self.page += 1,
            Some(2) => self.jobs.test("GLOBAL"),
            Some(3) => self.go(Screen::Conns),
            Some(4) => self.go(Screen::Subs),
            Some(_) => return false,
            None => {
                if !(GROUP_Y..GROUP_Y + GROUP_ROWS as i32 * GROUP_ROW).contains(&y) {
                    return false;
                }
                let i = self.page.min(n - 1) * GROUP_ROWS + ((y - GROUP_Y) / GROUP_ROW) as usize;
                let Some(g) = live.groups.get(i) else {
                    return false;
                };
                self.go(Screen::Group(g.name.clone()));
            }
        }
        true
    }

    fn tap_group(&mut self, x: i32, y: i32, live: &Live, name: &str) -> bool {
        let Some(g): Option<&Group> = live.group(name) else {
            return self.tap_list(x, y, 1, 1);
        };
        let n = pages(g.all.len(), NODE_ROWS);
        if tapped_bar(x, y, 4) == Some(2) {
            self.jobs.test(name);
            return true;
        }
        if self.tap_list(x, y, n, 4) {
            return true;
        }
        if !(LIST_Y..LIST_Y + NODE_ROWS as i32 * NODE_ROW).contains(&y) {
            return false;
        }
        let i = self.page.min(n - 1) * NODE_ROWS + ((y - LIST_Y) / NODE_ROW) as usize;
        let Some(node) = g.all.get(i) else {
            return false;
        };
        if !g.manual() {
            self.jobs.note(format!(
                "「{}」是{}组，会自己选节点",
                g.name,
                kind_name(&g.kind)
            ));
        } else if *node != g.now {
            // The one change that needs no confirmation.
            self.jobs.select(&g.name, node);
        }
        true
    }

    fn tap_subs(&mut self, x: i32, y: i32) -> bool {
        if inside(x, y, Self::restart_rect()) {
            self.ask(
                "重启 mihomo？".into(),
                "所有连接会断开，几秒后恢复。",
                AgentOp::Restart,
            );
            return true;
        }
        let subs = match self.jobs.subs() {
            Some(Ok(list)) => list,
            _ => Vec::new(),
        };
        let n = pages(subs.len(), SUB_ROWS);
        let start = self.page.min(n - 1) * SUB_ROWS;
        for (i, s) in subs.iter().skip(start).take(SUB_ROWS).enumerate() {
            if inside(x, y, Self::update_rect(i)) && s.enabled {
                self.ask(
                    format!("更新订阅「{}」？", s.name),
                    "重新下载这个订阅的节点，可能要几十秒。",
                    AgentOp::Update(Some((s.id.clone(), s.name.clone()))),
                );
                return true;
            }
        }
        match tapped_bar(x, y, 3) {
            Some(0) => {
                self.ask(
                    "更新全部订阅？".into(),
                    "逐个重新下载已启用的订阅，可能要一两分钟。",
                    AgentOp::Update(None),
                );
                true
            }
            Some(1) => {
                self.jobs.forget_subs();
                true
            }
            _ => self.tap_list(x, y, n, 3),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_times_and_dates() {
        assert_eq!(epoch_of("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_of("2026-10-29T08:00:00+08:00"), Some(1_793_232_000));
        assert_eq!(
            epoch_of("2026-10-29T00:00:00.123456789Z"),
            Some(1_793_232_000)
        );
        assert_eq!(epoch_of("bad"), None);
        assert_eq!(month_day(1_793_232_000), "10-29");
        assert_eq!(ago(90), "1分钟前");
        assert_eq!(ago(7200), "2小时前");
    }

    #[test]
    fn names_fit_and_lose_what_the_font_lacks() {
        assert_eq!(fit("🇭🇰 香港 01", 200), "香港 01");
        let f = fit("US-West-Premium-v6-very-long-name", 80);
        assert!(crate::cjk::width(&f) <= 80 && f.ends_with('~'), "{f}");
    }

    #[test]
    fn the_bar_and_lists_fit_above_the_tabs() {
        for n in 1..=5 {
            let r = bar_rects(n);
            assert!(r.last().unwrap().0 + r.last().unwrap().2 <= W - 4);
        }
    }
}
