//! 咕嘎's art. She is a raster cut from the owner's reference picture
//! (`assets/guga.bin`, built by `tools/guga_sprite.py`): the base image, a
//! face per expression, and the blink eyes and talking mouth on their own so
//! they can go over any face. The little props (hearts, notes, the whale)
//! are palette pixel art in `assets/pet.txt`.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::canvas::{rgb, Canvas};

static GUGA: &[u8] = include_bytes!("../../assets/guga.bin");
static PROPS: &str = include_str!("../../assets/pet.txt");

/// One RGB565 picture with 8-bit alpha, placed at (x, y) in the base.
pub struct Raster {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    px: Vec<(u16, u8)>,
}

impl Raster {
    fn draw(&self, c: &mut Canvas, ox: i32, oy: i32) {
        for (i, (col, a)) in self.px.iter().enumerate() {
            if *a == 0 {
                continue;
            }
            let (x, y) = (
                ox + self.x + i as i32 % self.w,
                oy + self.y + i as i32 / self.w,
            );
            if *a == 255 {
                c.pixel(x, y, *col);
            } else {
                c.blend(x, y, *col, *a as u32 + 1);
            }
        }
    }
}

fn parse_guga(b: &[u8]) -> HashMap<String, Raster> {
    let mut out = HashMap::new();
    if b.len() < 6 || &b[..4] != b"GUGA" {
        return out;
    }
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let count = u16_at(4) as usize;
    let mut o = 6;
    for _ in 0..count {
        let Some(&n) = b.get(o) else { break };
        let n = n as usize;
        if b.len() < o + 1 + n + 8 {
            break;
        }
        let name = String::from_utf8_lossy(&b[o + 1..o + 1 + n]).into_owned();
        o += 1 + n;
        let (x, y) = (u16_at(o) as i16 as i32, u16_at(o + 2) as i16 as i32);
        let (w, h) = (u16_at(o + 4) as i32, u16_at(o + 6) as i32);
        o += 8;
        let len = (w * h) as usize;
        if b.len() < o + len * 3 {
            break;
        }
        let px = (0..len)
            .map(|i| (u16_at(o + i * 3), b[o + i * 3 + 2]))
            .collect();
        o += len * 3;
        out.insert(name, Raster { x, y, w, h, px });
    }
    out
}

fn guga() -> &'static HashMap<String, Raster> {
    static SHEET: OnceLock<HashMap<String, Raster>> = OnceLock::new();
    SHEET.get_or_init(|| parse_guga(GUGA))
}

fn base() -> Option<&'static Raster> {
    guga().get("base")
}

/// Her size on screen.
pub fn size() -> (i32, i32) {
    base().map_or((114, 186), |b| (b.w, b.h))
}

/// What her face does this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pose {
    /// An expression with art (`face.<name>`), or "base".
    pub face: &'static str,
    pub blink: bool,
    /// Mouth open mid-word.
    pub talk: bool,
    pub blush: bool,
}

/// Draw her with the top-left of her box at (x, y). An expression without
/// art keeps the base face.
pub fn girl(c: &mut Canvas, x: i32, y: i32, pose: Pose) {
    let Some(b) = base() else { return };
    b.draw(c, x, y);
    let sheet = guga();
    if pose.face != "base" {
        if let Some(p) = sheet.get(&format!("face.{}", pose.face)) {
            p.draw(c, x, y);
        }
    }
    if pose.blush {
        let (fx, fy, fw, fh) = face();
        let (cy, rx, ry) = (fy + fh * 3 / 5, (fw / 10).max(3), (fh / 14).max(2));
        for cx in [fx + fw * 3 / 10, fx + fw * 7 / 10] {
            for dy in -ry..=ry {
                let t = 1.0 - (dy * dy) as f32 / (ry * ry) as f32;
                let dx = (rx as f32 * t.sqrt()) as i32;
                c.blend_rect(
                    x + cx - dx,
                    y + cy + dy,
                    dx * 2 + 1,
                    1,
                    rgb(0xff, 0x7a, 0x9a),
                    70,
                );
            }
        }
    }
    if pose.blink {
        if let Some(p) = sheet.get("eyes.blink") {
            p.draw(c, x, y);
        }
    }
    if pose.talk {
        if let Some(p) = sheet.get("mouth.talk") {
            p.draw(c, x, y);
        }
    }
}

/// Where her face is in her box: (x, y, w, h).
pub fn face() -> (i32, i32, i32, i32) {
    guga()
        .get("face")
        .map_or((30, 50, 54, 46), |r| (r.x, r.y, r.w, r.h))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Head,
    Body,
}

/// Which part of her is at (x, y) in her box, with a little slack.
pub fn part_at(x: i32, y: i32) -> Option<Part> {
    let (w, h) = size();
    if !(-4..w + 4).contains(&x) || !(-4..h + 4).contains(&y) {
        return None;
    }
    let (_, fy, _, fh) = face();
    Some(if y < fy + fh { Part::Head } else { Part::Body })
}

// ---- props: palette pixel art ----------------------------------------------

fn colour(ch: char) -> Option<u16> {
    Some(match ch {
        'K' => rgb(0x1c, 0x20, 0x4c),
        'H' => rgb(0x4c, 0x76, 0xee),
        'W' => rgb(0xfb, 0xfc, 0xff),
        'Y' => rgb(0xff, 0xd8, 0x4d),
        'R' => rgb(0xff, 0x4f, 0x7a),
        'w' => rgb(0xd8, 0xf2, 0xff),
        _ => return None,
    })
}

pub struct Sprite {
    pub w: usize,
    pub px: Vec<Option<u16>>,
}

fn parse_props(src: &'static str) -> HashMap<&'static str, Sprite> {
    let mut out = HashMap::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let Some(head) = line.strip_prefix("= ") else {
            continue;
        };
        let mut it = head.split_whitespace();
        let (Some(name), Some(size)) = (it.next(), it.next()) else {
            continue;
        };
        let Some((w, h)) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<usize>().ok()?, h.parse::<usize>().ok()?)))
        else {
            continue;
        };
        let mut px = Vec::with_capacity(w * h);
        for _ in 0..h {
            let row = lines.next().unwrap_or("");
            let before = px.len();
            px.extend(row.chars().take(w).map(colour));
            px.extend(std::iter::repeat_n(None, w - (px.len() - before)));
        }
        out.insert(name, Sprite { w, px });
    }
    out
}

pub fn get(name: &str) -> &'static Sprite {
    static SHEET: OnceLock<HashMap<&'static str, Sprite>> = OnceLock::new();
    static EMPTY: Sprite = Sprite {
        w: 0,
        px: Vec::new(),
    };
    SHEET
        .get_or_init(|| parse_props(PROPS))
        .get(name)
        .unwrap_or(&EMPTY)
}

pub fn draw(c: &mut Canvas, name: &str, x: i32, y: i32, scale: i32, flip: bool) {
    let s = get(name);
    c.sprite(x, y, s.w, &s.px, scale, flip);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raster_sheet_has_her_base_face_and_every_expression() {
        let b = base().expect("base");
        assert_eq!((b.x, b.y), (0, 0));
        assert!(b.w > 60 && b.h > 120);
        let mut names: Vec<String> = ["face", "eyes.blink", "mouth.talk"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        names.extend(
            [
                "blink",
                "happy",
                "talk",
                "angry",
                "surprised",
                "smug",
                "sleepy",
            ]
            .iter()
            .map(|e| format!("face.{e}")),
        );
        for n in names {
            let p = guga().get(&n).unwrap_or_else(|| panic!("{n} missing"));
            assert!(
                p.x >= 0 && p.y >= 0 && p.x + p.w <= b.w && p.y + p.h <= b.h,
                "{n}"
            );
        }
    }

    #[test]
    fn props_parse_with_their_declared_size() {
        for name in ["heart", "sparkle", "anger", "sweat", "note", "whale"] {
            let s = get(name);
            assert!(
                s.w > 0 && !s.px.is_empty() && s.px.len() % s.w == 0,
                "{name}"
            );
        }
    }

    #[test]
    fn the_head_is_above_the_body() {
        let (w, h) = size();
        assert_eq!(part_at(w / 2, 10), Some(Part::Head));
        assert_eq!(part_at(w / 2, h - 20), Some(Part::Body));
        assert_eq!(part_at(w + 30, 10), None);
    }
}
