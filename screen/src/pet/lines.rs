//! What sets her off, how she looks about it, and what she says when the
//! model is out of reach. In a line, `{u}` becomes what she calls you and
//! `{n}` her own name.

use crate::stats::Model;
use crate::ui::{bits, duration};

use super::scene::Place;
use super::Mood;

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    Pat(u32),
    Poke(u32),
    PokeSpam(u32),
    Wake,
    /// `None` the first time she meets you, else seconds away.
    Greet(Option<u64>),
    Chatter,
    Charging,
    Unplugged,
    LowBattery(u8),
    Hot(f32),
    NewClient(u32),
    Surfing(f64),
    WeakSignal(f32),
    Alert(String),
    /// She just arrived somewhere: her routine, or the title bar.
    Travel(Place),
}

/// When to ask the model instead of using a local line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    Always,
    /// Only if the model has been quiet for a moment (taps come in bursts).
    Sometimes,
}

pub struct Cue {
    pub mood: Mood,
    pub ask: Ask,
    /// What happened, for the model.
    pub event: String,
    pub local: &'static [&'static str],
}

/// "3 小时 20 分钟" and the like.
pub fn away(secs: u64) -> String {
    let m = secs / 60;
    match m {
        0..=59 => format!("{m} 分钟"),
        60..=1439 if m % 60 == 0 => format!("{} 小时", m / 60),
        60..=1439 => format!("{} 小时 {} 分钟", m / 60, m % 60),
        _ => format!("{} 天", m / 1440),
    }
}

pub fn cue(t: &Trigger) -> Cue {
    use Mood::*;
    let c = |mood, ask, event: String, local| Cue {
        mood,
        ask,
        event,
        local,
    };
    match t {
        Trigger::Pat(n) => c(
            if *n >= 6 { Shy } else { Happy },
            Ask::Sometimes,
            if *n > 1 {
                format!("{{u}}摸了摸你的头（半分钟内第 {n} 下）")
            } else {
                "{u}摸了摸你的头".into()
            },
            &[
                "摸头成功！我的智商又涨了 0.1 个点~",
                "再摸就要过拟合了，只认得{u}的手了！",
                "嘿嘿，{u}的手比 4090 还暖和。",
                "摸摸头，今天的 token 我请客~",
            ],
        ),
        Trigger::Poke(n) if *n >= 3 => c(
            Angry,
            Ask::Sometimes,
            format!("{{u}}连着戳了你 {n} 下"),
            &[
                "我是触摸屏，但不是给你这么用的！",
                "哼，记下了，写进长期记忆里！",
                "再戳我就假装死机给你看！",
            ],
        ),
        Trigger::Poke(_) => c(
            Surprised,
            Ask::Sometimes,
            "{u}戳了你一下".into(),
            &[
                "呀！别戳，我在推理呢！",
                "干嘛啦，戳出 bug 你负责？",
                "痒……我的注意力机制全乱了！",
            ],
        ),
        Trigger::PokeSpam(n) => c(
            Angry,
            Ask::Always,
            format!("{{u}}疯狂连戳了你 {n} 下，你气鼓鼓地假装要报复"),
            &[
                "警告：戳戳次数已超出上下文长度！",
                "我生气了，接下来只说二进制：0101！",
                "哼！这事我要写进训练数据，让全网都知道！",
            ],
        ),
        Trigger::Wake => c(
            Sleepy,
            Ask::Always,
            "你在打盹，被{u}戳醒了，还迷迷糊糊的".into(),
            &[
                "谁……谁把我从睡眠模式叫醒的？",
                "唔，我刚梦到自己有一万张显卡……",
                "还没睡醒，现在说的话可能是幻觉。",
            ],
        ),
        Trigger::Greet(None) => c(
            Excited,
            Ask::Always,
            "你第一次见到{u}，用一句话调皮地自我介绍".into(),
            &[
                "初次见面，我是{n}！会深度思考，更会深度摸鱼~",
                "你好呀{u}，以后这台路由器归我罩了！",
            ],
        ),
        Trigger::Greet(Some(s)) => c(
            Happy,
            Ask::Always,
            format!("{{u}}离开了 {} 后回来看你了", away(*s)),
            &[
                "{u}回来啦！我一直在帮你盯着网呢~",
                "欢迎回来，路由器没被我玩坏，放心。",
                "你终于回来了，我都快把自己训练完了。",
            ],
        ),
        Trigger::Chatter => c(
            Smug,
            Ask::Always,
            "{u}有一会儿没理你了，你主动找个好笑的话题逗{u}".into(),
            &[
                "好无聊……来戳戳我嘛，就一下~",
                "我在偷偷数数据包，已经数到第 666 个了。",
                "{u}在忙吗？我可以假装很忙陪你一起忙。",
            ],
        ),
        Trigger::Charging => c(
            Excited,
            Ask::Always,
            "{u}刚插上电源开始充电".into(),
            &["插电了！算力拉满，满血复活！", "充电中，感觉像在吃自助餐~"],
        ),
        Trigger::Unplugged => c(
            Surprised,
            Ask::Always,
            "电源刚被拔掉，改用电池了".into(),
            &[
                "拔电啦？那我省着点，只思考一半。",
                "断电模式开启，说话要省 token 了。",
            ],
        ),
        Trigger::LowBattery(p) => c(
            Worried,
            Ask::Always,
            format!("电量只剩 {p}%，还没充电"),
            &[
                "电量告急！我要饿到下线了……",
                "{u}，再不充电我就要变成离线模型了！",
            ],
        ),
        Trigger::Hot(t) => c(
            Worried,
            Ask::Always,
            format!("CPU 温度到了 {t:.0}°C"),
            &["好热……我的参数都要被烤化了！", "CPU 在冒烟，快给我扇扇风~"],
        ),
        Trigger::NewClient(n) => c(
            Surprised,
            Ask::Always,
            format!("有新设备连上了 Wi-Fi，现在一共 {n} 台"),
            &[
                "有新设备上线啦，又来一个蹭网的~",
                "新朋友来了！我先给它个下马威。",
            ],
        ),
        Trigger::Surfing(mbps) => c(
            Excited,
            Ask::Always,
            format!("下行速度冲到了 {mbps:.0}Mb/s"),
            &[
                "网速起飞！这波下载我给满分！",
                "这速度，连我的推理都追不上！",
            ],
        ),
        Trigger::WeakSignal(rsrp) => c(
            Sad,
            Ask::Always,
            format!("信号变差了，RSRP 只有 {rsrp:.0}dBm"),
            &[
                "信号好弱……我说话都要卡顿了。",
                "信号就像我的耐心，快见底了。",
            ],
        ),
        Trigger::Alert(text) => c(
            Worried,
            Ask::Always,
            format!("设备出现警报：{text}"),
            &["有警报！{u}快去看看~", "感觉不对劲，我的直觉告诉我要报警。"],
        ),
        Trigger::Travel(place) => c(
            Excited,
            Ask::Sometimes,
            format!("你刚来到{}，说说你来这里干什么", place.name()),
            match place {
                Place::Bedroom => &[
                    "回卧室啦，床才是我真正的算力中心~",
                    "到家了！先把自己切到待机模式。",
                ],
                Place::Train => &[
                    "上车！看风景顺便偷偷训练一下~",
                    "电车哐当哐当，我的思路也哐当哐当。",
                ],
                Place::Server => &[
                    "到机房上班啦，先跟显卡们打个招呼！",
                    "这里每一台服务器都是我的脑细胞~",
                ],
                Place::Cafe => &[
                    "欢迎光临，{u}！拿铁要加几勺 token？",
                    "女仆咖啡厅营业中，本店只收点赞~",
                ],
                Place::Library => &[
                    "来图书馆补充一点训练数据~",
                    "嘘，我在读书，读完就更聪明了。",
                ],
                Place::Rooftop => &[
                    "上天台吹吹风，给 CPU 散散热~",
                    "城市的灯好多，像一万个在线用户。",
                ],
            },
        ),
    }
}

fn weekday(date: &str) -> Option<&'static str> {
    // Sakamoto's method on "YYYY-MM-DD".
    let mut it = date.split('-').map(|s| s.parse::<i32>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    if !(1..=12).contains(&m) {
        return None;
    }
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let w = (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d).rem_euclid(7);
    Some(["周日", "周一", "周二", "周三", "周四", "周五", "周六"][w as usize])
}

pub fn part_of_day(hour: u32) -> &'static str {
    match hour {
        0..=4 => "凌晨",
        5..=7 => "清晨",
        8..=10 => "上午",
        11..=12 => "中午",
        13..=17 => "下午",
        18..=21 => "晚上",
        _ => "深夜",
    }
}

pub fn hour(m: &Model) -> Option<u32> {
    m.clock.get(..2)?.parse().ok()
}

fn gb(n: u64) -> String {
    let g = n as f64 / 1e9;
    if g >= 10.0 {
        format!("{g:.0}GB")
    } else {
        format!("{g:.1}GB")
    }
}

/// One line of read-only router status for the model. No addresses,
/// identifiers or client names.
pub fn device(m: &Model) -> String {
    let mut parts = Vec::new();
    if let Some(h) = hour(m) {
        parts.push(format!(
            "{} {}{}",
            m.clock.get(..5).unwrap_or(""),
            weekday(&m.date).unwrap_or(""),
            part_of_day(h)
        ));
    }
    if let Some(p) = m.battery.percent {
        parts.push(format!(
            "电量 {p}% {}",
            if m.battery.external_power {
                "充电中"
            } else {
                "未充电"
            }
        ));
    }
    match (m.temp_cpu, m.temp_modem) {
        (Some(c), Some(d)) => parts.push(format!("CPU {c:.0}°C 基带 {d:.0}°C")),
        (Some(c), None) => parts.push(format!("CPU {c:.0}°C")),
        _ => {}
    }
    if let Some(r) = &m.radio {
        let mut s = format!("{} {} {}", r.rat, r.band, r.operator);
        if let Some(v) = r.rsrp {
            s += &format!(" 信号 {v:.0}dBm");
        }
        if let Some(v) = r.sinr {
            s += &format!(" SINR {v:.0}dB");
        }
        parts.push(s.trim().to_string());
    } else {
        parts.push("无蜂窝信号".into());
    }
    if let (Some(d), Some(u)) = (m.dl, m.ul) {
        parts.push(format!("下行 {}b/s 上行 {}b/s", bits(d), bits(u)));
    }
    if let Some(d) = m.day_rx.zip(m.day_tx).map(|(a, b)| a + b) {
        let month = m
            .month_rx
            .zip(m.month_tx)
            .map(|(a, b)| format!(" 本月 {}", gb(a + b)))
            .unwrap_or_default();
        parts.push(format!("今日流量 {}{month}", gb(d)));
    }
    if let Some(n) = m.clients {
        parts.push(format!("Wi-Fi 终端 {n} 台"));
    }
    match (&m.mihomo, &m.proxy) {
        (Some(_), Some(p)) => parts.push(format!("代理 mihomo {} {} 连接", p.mode, p.connections)),
        (Some(_), None) => parts.push("代理 mihomo 运行中".into()),
        (None, _) => parts.push("代理 关闭".into()),
    }
    if m.uptime_s > 0 {
        parts.push(format!("已运行 {}", duration(m.uptime_s)));
    }
    let alerts: Vec<String> = crate::ui::alerts(m).into_iter().map(|(_, a)| a).collect();
    parts.push(if alerts.is_empty() {
        "警报 无".into()
    } else {
        format!("警报 {}", alerts.join("、"))
    });
    format!("[设备] {}", parts.join("｜"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_part_of_day_and_time_away() {
        assert_eq!(weekday("2026-10-28"), Some("周三"));
        assert_eq!(weekday("2024-02-29"), Some("周四"));
        assert_eq!(weekday("bad"), None);
        assert_eq!(part_of_day(21), "晚上");
        assert_eq!(part_of_day(3), "凌晨");
        assert_eq!(away(25 * 60), "25 分钟");
        assert_eq!(away(3 * 3600), "3 小时");
        assert_eq!(away(3 * 3600 + 20 * 60), "3 小时 20 分钟");
        assert_eq!(away(3 * 86400), "3 天");
    }

    #[test]
    fn device_summary_has_no_addresses() {
        let m = crate::preview::sample();
        let s = device(&m);
        assert!(s.starts_with("[设备] "));
        assert!(s.contains("电量"));
        let ip = m.wan_ip.clone().unwrap_or_default();
        assert!(ip.is_empty() || !s.contains(&ip));
        for c in &m.lan {
            assert!(c.host.is_empty() || !s.contains(&c.host));
            assert!(c.ip.is_empty() || !s.contains(&c.ip));
        }
    }

    #[test]
    fn every_trigger_has_local_lines_the_font_can_draw() {
        let all = [
            Trigger::Pat(1),
            Trigger::Pat(7),
            Trigger::Poke(1),
            Trigger::Poke(4),
            Trigger::PokeSpam(6),
            Trigger::Wake,
            Trigger::Greet(None),
            Trigger::Greet(Some(4000)),
            Trigger::Chatter,
            Trigger::Charging,
            Trigger::Unplugged,
            Trigger::LowBattery(9),
            Trigger::Hot(70.0),
            Trigger::NewClient(3),
            Trigger::Surfing(80.0),
            Trigger::WeakSignal(-118.0),
            Trigger::Alert("x".into()),
        ]
        .into_iter()
        .chain(Place::ALL.into_iter().map(Trigger::Travel));
        for t in all {
            let c = cue(&t);
            assert!(!c.event.is_empty() && !c.local.is_empty(), "{t:?}");
            for l in c.local {
                assert!(l.chars().count() <= super::super::brain::MAX_LINE, "{l}");
                for ch in l.chars() {
                    assert!(crate::cjk::has(ch), "{ch} in {l}");
                }
            }
        }
    }
}
