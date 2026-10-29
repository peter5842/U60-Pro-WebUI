//! Where 咕嘎 spends her day. Each place is a painted background in three
//! lights (`assets/scenes.bin`, built by `tools/scenes.py` from pictures made
//! with Codex's image tool), with the little things that move in it drawn on
//! top: steam, dust in the sunbeams, blinking racks, the view from the train,
//! stars and string lights at night.
//!
//! Background coordinates are relative to the top-left of the stage's inside
//! (310x233); `life` takes that origin.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::OnceLock;

use crate::canvas::{rgb, Canvas};

static SCENES: &[u8] = include_bytes!("../../assets/scenes.bin");

/// Where she is, in the order a tap on the title bar cycles through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Bedroom,
    Train,
    Server,
    Cafe,
    Library,
    Rooftop,
}

impl Place {
    pub const ALL: [Place; 6] = [
        Place::Bedroom,
        Place::Train,
        Place::Server,
        Place::Cafe,
        Place::Library,
        Place::Rooftop,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Place::Bedroom => "bedroom",
            Place::Train => "train",
            Place::Server => "server",
            Place::Cafe => "cafe",
            Place::Library => "library",
            Place::Rooftop => "rooftop",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Place::Bedroom => "卧室小屋",
            Place::Train => "电车",
            Place::Server => "算力机房",
            Place::Cafe => "女仆咖啡厅",
            Place::Library => "图书馆",
            Place::Rooftop => "天台",
        }
    }

    /// Her day: up in her room, the train to work at the datacenter, a lunch
    /// shift at the café, reading in the afternoon, the train home, the
    /// rooftop at sunset, the café again, bed.
    pub fn at(hour: Option<u32>) -> Place {
        match hour {
            Some(7..=8) | Some(17) => Place::Train,
            Some(9..=11) => Place::Server,
            Some(12..=13) | Some(21..=22) => Place::Cafe,
            Some(14..=16) => Place::Library,
            Some(18..=20) => Place::Rooftop,
            _ => Place::Bedroom,
        }
    }

    pub fn next(self) -> Place {
        let i = Place::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Place::ALL[(i + 1) % Place::ALL.len()]
    }
}

/// The light, from the device clock. Sunrise and sunset share the golden
/// picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Light {
    Golden,
    Day,
    Night,
}

impl Light {
    pub fn at(hour: Option<u32>) -> Light {
        match hour {
            Some(5..=7) | Some(17..=18) => Light::Golden,
            Some(8..=16) | None => Light::Day,
            _ => Light::Night,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Light::Golden => "morning",
            Light::Day => "noon",
            Light::Night => "evening",
        }
    }
}

// ---- pictures ---------------------------------------------------------------

/// Width, height and where the pixels start in SCENES.
#[derive(Clone, Copy)]
struct Image {
    w: usize,
    h: usize,
    at: usize,
}

impl Image {
    fn px(&self, x: usize, y: usize) -> u16 {
        let o = self.at + (y * self.w + x) * 2;
        u16::from_le_bytes([SCENES[o], SCENES[o + 1]])
    }
}

fn parse(b: &[u8]) -> HashMap<String, Image> {
    let mut out = HashMap::new();
    if b.len() < 6 || &b[..4] != b"SCNS" {
        return out;
    }
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as usize;
    let mut o = 6;
    for _ in 0..u16_at(4) {
        let Some(&n) = b.get(o) else { break };
        let n = n as usize;
        if b.len() < o + 1 + n + 4 {
            break;
        }
        let name = String::from_utf8_lossy(&b[o + 1..o + 1 + n]).into_owned();
        o += 1 + n;
        let (w, h) = (u16_at(o), u16_at(o + 2));
        o += 4;
        if b.len() < o + w * h * 2 {
            break;
        }
        out.insert(name, Image { w, h, at: o });
        o += w * h * 2;
    }
    out
}

fn image(name: &str) -> Option<Image> {
    static INDEX: OnceLock<HashMap<String, Image>> = OnceLock::new();
    INDEX.get_or_init(|| parse(SCENES)).get(name).copied()
}

/// The background, w*h RGB565 pixels, or `None` if the picture is missing
/// or not `w` by `h`.
pub fn background(place: Place, light: Light, w: usize, h: usize) -> Option<Vec<u16>> {
    let img = image(&format!("{}.{}", place.key(), light.key()))?;
    if (img.w, img.h) != (w, h) {
        return None;
    }
    Some((0..w * h).map(|i| img.px(i % w, i / w)).collect())
}

// ---- drawing helpers ----------------------------------------------------------

pub fn hash(n: i64) -> u32 {
    let mut x = n as u64 ^ 0x9e37_79b9_7f4a_7c15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x as u32
}

/// An ellipse of `col` mixed in at `alpha / 256`.
pub fn ellipse(c: &mut Canvas, cx: i32, cy: i32, rx: i32, ry: i32, col: u16, alpha: u32) {
    for dy in -ry..=ry {
        let t = 1.0 - (dy * dy) as f32 / (ry * ry).max(1) as f32;
        let dx = (rx as f32 * t.max(0.0).sqrt()) as i32;
        c.blend_rect(cx - dx, cy + dy, dx * 2 + 1, 1, col, alpha);
    }
}

/// A soft round glow: three nested ellipses.
fn glow(c: &mut Canvas, cx: i32, cy: i32, r: i32, col: u16, alpha: u32) {
    for k in 1..=3 {
        let rr = r * k / 3;
        ellipse(c, cx, cy, rr, rr * 4 / 5, col, alpha / 3);
    }
}

/// A slow 0..1 wave with its own phase.
fn wave(now: i64, period: i64, phase: u32) -> f32 {
    let t = (now + phase as i64 % period) as f32 / period as f32;
    0.5 + 0.5 * (t * TAU).sin()
}

/// Stars in a box that come and go one at a time.
fn stars(c: &mut Canvas, o: (i32, i32), area: (i32, i32, i32, i32), n: i64, now: i64, seed: i64) {
    let (x0, y0, w, h) = area;
    for i in 0..n {
        let k = hash(seed + i * 7919);
        if hash(seed + i * 31 + now / 700) % 4 == 0 {
            continue;
        }
        let (x, y) = (
            o.0 + x0 + (k % w as u32) as i32,
            o.1 + y0 + (k >> 12) as i32 % h,
        );
        c.pixel(x, y, rgb(0xff, 0xff, 0xf0));
        if k % 4 == 0 && (now / 350 + i) % 3 != 0 {
            // A brighter one, with a little cross.
            let dim = rgb(0x9c, 0xb4, 0xff);
            c.pixel(x - 1, y, dim);
            c.pixel(x + 1, y, dim);
            c.pixel(x, y - 1, dim);
            c.pixel(x, y + 1, dim);
        }
    }
}

/// Dust glinting in the light: rising slowly and swaying.
fn motes(c: &mut Canvas, o: (i32, i32), area: (i32, i32, i32, i32), n: i64, now: i64, col: u16) {
    let (x0, y0, w, h) = area;
    for i in 0..n {
        let k = hash(i * 104_729 + 3);
        let speed = 90 + (k % 90) as i64;
        let rise = ((now / speed + (k >> 8) as i64) % h as i64) as i32;
        let sway = ((now as f32 / (1800.0 + (k % 900) as f32) + i as f32).sin() * 6.0) as i32;
        let (x, y) = (
            o.0 + x0 + (k % w as u32) as i32 + sway,
            o.1 + y0 + h - 1 - rise,
        );
        // Fade in at the bottom and out at the top of the box.
        let edge = (rise.min(h - rise) * 256 / (h / 4).max(1)).min(256) as u32;
        let a = (wave(now, 1500 + (k % 1300) as i64, k) * 200.0) as u32 * edge / 256;
        c.blend(x, y, col, a);
        if k % 5 == 0 {
            c.blend(x + 1, y, col, a / 2);
            c.blend(x, y + 1, col, a / 2);
        }
    }
}

/// Wisps of steam rising from (x, y).
fn steam(c: &mut Canvas, o: (i32, i32), x: i32, y: i32, now: i64, seed: u32) {
    const PERIOD: i64 = 2400;
    for k in 0..4i64 {
        let t = ((now + k * PERIOD / 4 + seed as i64) % PERIOD) as f32 / PERIOD as f32;
        let sx = x + ((t * 7.0 + k as f32 + seed as f32).sin() * 3.0 * t) as i32;
        let sy = y - (t * 24.0) as i32;
        let r = 1 + (t * 3.0) as i32;
        let a = ((1.0 - t) * 70.0) as u32;
        ellipse(c, o.0 + sx, o.1 + sy, r, r + 1, 0xffff, a);
    }
}

/// Small LEDs scattered over a box, each blinking on its own.
fn leds(c: &mut Canvas, o: (i32, i32), area: (i32, i32, i32, i32), n: i64, now: i64, seed: i64) {
    let (x0, y0, w, h) = area;
    const COLS: [u16; 4] = [
        rgb(0x5c, 0xff, 0x9a),
        rgb(0x6c, 0xd8, 0xff),
        rgb(0x5c, 0xff, 0x9a),
        rgb(0xff, 0xc8, 0x5c),
    ];
    for i in 0..n {
        let k = hash(seed + i * 613);
        let rate = 120 + (k % 400) as i64;
        if hash(seed + i * 17 + now / rate) % 3 == 0 {
            continue;
        }
        let (x, y) = (
            o.0 + x0 + (k % w as u32) as i32,
            o.1 + y0 + (k >> 10) as i32 % h,
        );
        c.pixel(x, y, COLS[(k >> 20) as usize % 4]);
    }
}

// ---- the places -------------------------------------------------------------------

/// How far the train has bumped the picture down this frame: a soft
/// "ka-tank" twice every 1.5 s.
pub fn bump(place: Place, now: i64) -> i32 {
    let t = now % 1500;
    (place == Place::Train && ((0..70).contains(&t) || (190..260).contains(&t))) as i32
}

/// The moving things of `place`, on a background whose top-left is `o`.
pub fn life(c: &mut Canvas, o: (i32, i32), place: Place, light: Light, now: i64) {
    let night = light == Light::Night;
    let sun = if light == Light::Golden {
        rgb(0xff, 0xe0, 0xa8)
    } else {
        rgb(0xff, 0xfa, 0xe8)
    };
    let warm = rgb(0xff, 0xc8, 0x70);
    match place {
        Place::Bedroom => {
            if night {
                stars(c, o, (220, 28, 46, 44), 9, now, 11);
                let lamp = 30 + (wave(now, 5000, 0) * 14.0) as u32;
                glow(c, o.0 + 279, o.1 + 78, 30, warm, lamp);
                glow(
                    c,
                    o.0 + 193,
                    o.1 + 88,
                    10,
                    rgb(0xff, 0xe8, 0x80),
                    20 + (wave(now, 3200, 9) * 50.0) as u32,
                );
            } else {
                motes(c, o, (20, 70, 180, 130), 16, now, sun);
            }
            // The pocket router on her desk, blinking like the real one.
            let on = (now / 900) % 4 != 3 || (now / 150) % 2 == 0;
            if on {
                c.pixel(o.0 + 241, o.1 + 93, rgb(0x4c, 0xb0, 0xff));
            }
        }
        Place::Cafe => {
            steam(c, o, 216, 86, now, 0);
            steam(c, o, 232, 85, now, 900);
            if night {
                for (x, y, r) in [(229, 30, 26), (267, 22, 26), (129, 42, 18)] {
                    glow(
                        c,
                        o.0 + x,
                        o.1 + y,
                        r,
                        warm,
                        30 + (wave(now, 4200, x as u32) * 20.0) as u32,
                    );
                }
            } else {
                motes(c, o, (15, 40, 120, 120), 10, now, sun);
            }
            // Something sparkles in the cake case now and then.
            let k = hash(now / 1100);
            if k % 3 == 0 {
                let (x, y) = (
                    o.0 + 258 + (k % 46) as i32,
                    o.1 + 112 + (k >> 8) as i32 % 28,
                );
                let a = (wave(now, 1100, 0) * 230.0) as u32;
                for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                    c.blend(
                        x + dx,
                        y + dy,
                        0xffff,
                        if dx == 0 && dy == 0 { a } else { a / 2 },
                    );
                }
            }
        }
        Place::Library => {
            if night {
                stars(c, o, (130, 26, 48, 64), 10, now, 23);
                for (i, (x, y, r)) in [(14, 38, 16), (96, 61, 16), (210, 58, 16), (253, 120, 24)]
                    .into_iter()
                    .enumerate()
                {
                    // Lamp flames flicker, the reading lamp holds steady.
                    let f = if i == 3 {
                        34
                    } else {
                        24 + hash(i as i64 * 97 + now / 160) % 18
                    };
                    glow(c, o.0 + x, o.1 + y, r, warm, f);
                }
            } else {
                motes(c, o, (90, 30, 170, 160), 24, now, sun);
            }
        }
        Place::Server => {
            // Six cabinets on the back wall, then the racks on the right.
            for (i, x) in [53, 76, 100, 124, 147, 171].into_iter().enumerate() {
                leds(c, o, (x + 2, 92, 15, 36), 7, now, i as i64 * 50);
            }
            for (i, area) in [(232, 48, 26, 56), (276, 36, 22, 50), (206, 66, 14, 40)]
                .into_iter()
                .enumerate()
            {
                leds(c, o, area, 8, now, 400 + i as i64 * 50);
            }
            // The left monitor draws a live trace, the right one bars.
            let trace = rgb(0x7c, 0xe8, 0xff);
            let mut prev = None;
            for x in 0..36 {
                let t = (x as f32 + now as f32 / 90.0) / 6.0;
                let y = 102 + (t.sin() * 4.0 + (t * 2.3).sin() * 2.0) as i32;
                if let Some(py) = prev {
                    c.line(o.0 + 242 + x - 1, o.1 + py, o.0 + 242 + x, o.1 + y, trace);
                }
                prev = Some(y);
            }
            for b in 0..6 {
                let h = 3 + hash(b * 5 + now / 400) % 12;
                c.blend_rect(
                    o.0 + 287 + b as i32 * 4,
                    o.1 + 109 - h as i32,
                    3,
                    h as i32,
                    trace,
                    150,
                );
            }
            // The whale emblem breathes.
            let a = if night { 34 } else { 18 } + (wave(now, 3600, 0) * 26.0) as u32;
            glow(c, o.0 + 121, o.1 + 67, 34, rgb(0x6c, 0xb4, 0xff), a);
        }
        Place::Train => train(c, o, light, now),
        Place::Rooftop => {
            if night {
                stars(c, o, (0, 24, 240, 56), 26, now, 37);
                // String lights, each glowing on its own beat.
                for (i, (x, y)) in [
                    (21, 16),
                    (46, 21),
                    (87, 23),
                    (112, 20),
                    (128, 5),
                    (148, 10),
                    (180, 16),
                    (209, 20),
                    (236, 22),
                ]
                .into_iter()
                .enumerate()
                {
                    let a = 20 + (wave(now, 2600, i as u32 * 811) * 40.0) as u32;
                    glow(c, o.0 + x, o.1 + y, 9, warm, a);
                }
                glow(c, o.0 + 208, o.1 + 160, 14, warm, 24 + hash(now / 180) % 20);
                // Windows in the city switch on and off.
                for i in 0..14i64 {
                    let k = hash(i * 7717 + now / 2600);
                    if k % 3 != 0 {
                        let (x, y) = (o.0 + (k % 230) as i32, o.1 + 94 + (k >> 9) as i32 % 30);
                        c.fill_rect(x, y, 2, 1, rgb(0xff, 0xd8, 0x8c));
                    }
                }
                // A plane crossing, its light blinking; now and then a
                // shooting star.
                let t = now % 40_000;
                if t < 24_000 {
                    let x = o.0 + 250 - (t * 260 / 24_000) as i32;
                    let y = o.1 + 32 + (t / 3000) as i32;
                    let col = if (now / 500) % 2 == 0 {
                        rgb(0xff, 0x50, 0x50)
                    } else {
                        rgb(0xff, 0xff, 0xff)
                    };
                    c.fill_rect(x, y, 2, 1, col);
                }
                let s = now % 23_000;
                if s < 600 {
                    let p = s as f32 / 600.0;
                    let (x, y) = (o.0 + 200 - (p * 90.0) as i32, o.1 + 26 + (p * 30.0) as i32);
                    for k in 0..12 {
                        let a = 230 - k as u32 * 18;
                        c.blend(x + k * 3 / 2, y - k / 2, 0xffff, a);
                    }
                }
            } else {
                // Birds crossing the sky in a loose V.
                let t = now % 26_000;
                if t < 14_000 {
                    for (k, (dx, dy)) in [(0, 0), (9, -5), (12, 6)].into_iter().enumerate() {
                        let x = o.0 - 20 + (t * 300 / 14_000) as i32 + dx;
                        let y = o.1 + 42 + dy + ((t as f32 / 900.0 + k as f32).sin() * 3.0) as i32;
                        let up = (now / 160 + k as i64) % 2 == 0;
                        let ink = rgb(0x3a, 0x40, 0x58);
                        let wing = if up { -1 } else { 1 };
                        c.pixel(x, y, ink);
                        c.pixel(x - 1, y + wing, ink);
                        c.pixel(x - 2, y + wing, ink);
                        c.pixel(x + 1, y + wing, ink);
                        c.pixel(x + 2, y + wing, ink);
                    }
                }
            }
        }
    }
}

/// The train windows: glass as (x, y, w, h). The strip of near trees runs
/// along their bottom, utility poles and their wires sweep past in front.
const GLASS: [(i32, i32, i32, i32); 4] = [
    (0, 42, 6, 68),
    (17, 42, 84, 68),
    (108, 42, 83, 68),
    (201, 42, 63, 68),
];
const TREES_PX_S: i64 = 55;
const POLE_PX_S: i64 = 170;
const POLE_GAP: i64 = 420;

fn train(c: &mut Canvas, o: (i32, i32), light: Light, now: i64) {
    let Some(view) = image(&format!("view.{}", light.key())) else {
        return;
    };
    let shift = (now * TREES_PX_S / 1000) as usize;
    let top = 42 + 68 - view.h as i32;
    for (gx, gy, gw, gh) in GLASS {
        for vy in 0..view.h {
            let y = top + vy as i32;
            if y < gy || y >= gy + gh {
                continue;
            }
            // The top rows feather into the painted landscape.
            let a = ((vy as u32 + 1) * 256 / 7).min(256);
            for x in gx..gx + gw {
                let sx = (x as usize + shift) % view.w;
                let col = view.px(sx, vy);
                let (px, py) = (o.0 + x, o.1 + y);
                if a >= 256 {
                    c.pixel(px, py, col);
                } else {
                    c.blend(px, py, col, a);
                }
            }
        }
    }
    let ink = match light {
        Light::Night => rgb(0x0c, 0x0e, 0x18),
        _ => rgb(0x38, 0x3a, 0x44),
    };
    let travelled = now * POLE_PX_S / 1000;
    let first = -(travelled % POLE_GAP) as i32;
    let in_glass = |x: i32| {
        GLASS
            .iter()
            .any(|(gx, _, gw, _)| (*gx..gx + gw).contains(&x))
    };
    // Wires sag between poles, then the poles themselves.
    for wire in [48, 53] {
        for x in 0..264 {
            if !in_glass(x) {
                continue;
            }
            let t = (x - first).rem_euclid(POLE_GAP as i32) as f32 / POLE_GAP as f32;
            let sag = (4.0 * t * (1.0 - t) * 7.0) as i32;
            c.pixel(o.0 + x, o.1 + wire + sag, ink);
        }
    }
    let mut px = first;
    while px < 264 {
        for x in px..px + 4 {
            if in_glass(x) {
                c.vline(o.0 + x, o.1 + 42, 68, ink);
            }
        }
        for x in px - 5..px + 9 {
            if in_glass(x) {
                c.hline(o.0 + x, o.1 + 47, 1, ink);
            }
        }
        px += POLE_GAP as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_has_three_lights_and_the_train_view() {
        for place in Place::ALL {
            for light in [Light::Golden, Light::Day, Light::Night] {
                assert!(
                    background(place, light, 310, 233).is_some(),
                    "{place:?} {light:?}"
                );
            }
        }
        for light in [Light::Golden, Light::Day, Light::Night] {
            let v = image(&format!("view.{}", light.key())).expect("view");
            assert_eq!(v.h, 34);
            assert!(v.w >= 264, "the loop is longer than the windows");
        }
    }

    #[test]
    fn her_day_and_the_title_cycle_visit_every_place() {
        let mut seen: Vec<Place> = (0..24).map(|h| Place::at(Some(h))).collect();
        seen.dedup();
        for p in Place::ALL {
            assert!(seen.contains(&p), "{p:?} never on the routine");
        }
        let mut p = Place::Bedroom;
        for _ in 0..Place::ALL.len() {
            p = p.next();
        }
        assert_eq!(p, Place::Bedroom);
        assert_eq!(Place::at(Some(3)), Place::Bedroom);
        assert_eq!(Light::at(Some(3)), Light::Night);
        assert_eq!(Light::at(Some(18)), Light::Golden);
    }

    #[test]
    fn every_place_draws_in_every_light_inside_the_canvas() {
        let mut c = Canvas::new();
        for place in Place::ALL {
            for light in [Light::Golden, Light::Day, Light::Night] {
                for now in [0, 777, 12_345, 40_000 + 333] {
                    life(&mut c, (5, 45), place, light, now);
                }
            }
        }
    }
}
