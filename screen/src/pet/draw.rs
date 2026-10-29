//! Drawing 咕嘎, her scene and her speech bubble. Everything here only reads
//! the pet; time moves in `Pet::tick`.

use crate::canvas::{mix, rgb, Canvas, W};
use crate::font::Size::{Large, Medium, Small};
use crate::ui::Palette;

use super::scene::{self, ellipse};
use super::sprite::{self, Pose};
use super::*;

const INK: u16 = rgb(0x1c, 0x20, 0x4c);
const PAPER: u16 = rgb(0xf6, 0xf8, 0xff);
const DEEPSEEK: u16 = rgb(0x4d, 0x6b, 0xfe);
const GOLD: u16 = rgb(0xff, 0xd8, 0x4d);
/// A change of scene or light dissolves over this long.
pub const FADE_MS: i64 = 600;

/// Every opaque pixel of a sprite in one colour.
fn silhouette(c: &mut Canvas, name: &str, x: i32, y: i32, scale: i32, col: u16) {
    let s = sprite::get(name);
    for (i, p) in s.px.iter().enumerate() {
        if p.is_some() {
            let (sx, sy) = ((i % s.w) as i32, (i / s.w) as i32);
            c.fill_rect(x + sx * scale, y + sy * scale, scale, scale, col);
        }
    }
}

impl Pet {
    pub fn render(&self, c: &mut Canvas, p: &Palette) {
        self.backdrop(c);
        scene::life(
            c,
            (STAGE_X + 1, STAGE_Y + 1),
            self.place,
            self.light,
            self.now,
        );
        let (gx, gy) = self.girl_screen();
        let (w, _) = sprite::size();
        let lift = self.hop();
        ellipse(
            c,
            gx + w / 2,
            FEET_Y,
            w * 2 / 5 - lift / 2,
            5,
            0,
            110 - lift as u32 * 3,
        );
        sprite::girl(c, gx, gy, self.pose());
        self.marks(c, gx, gy);
        self.draw_particles(c);
        self.draw_bubble(c);
        self.title(c, p);
        // Frame with bright corners, like the rest of the screen's boxes.
        c.frame(STAGE_X, STAGE_Y, STAGE_W, STAGE_H, p.faint);
        let (x1, y1) = (STAGE_X + STAGE_W - 1, STAGE_Y + STAGE_H - 1);
        c.hline(STAGE_X, STAGE_Y, 8, p.accent);
        c.vline(STAGE_X, STAGE_Y, 8, p.accent);
        c.hline(x1 - 7, STAGE_Y, 8, p.accent);
        c.vline(x1, STAGE_Y, 8, p.accent);
        c.hline(STAGE_X, y1, 8, p.accent);
        c.vline(STAGE_X, y1 - 7, 8, p.accent);
        c.hline(x1 - 7, y1, 8, p.accent);
        c.vline(x1, y1 - 7, 8, p.accent);
    }

    /// The background of her scene, decoded once per scene and light, and
    /// dissolving from the last one for a moment after a change.
    fn backdrop(&self, c: &mut Canvas) {
        let (x0, y0, w, h) = (STAGE_X + 1, STAGE_Y + 1, STAGE_W - 2, STAGE_H - 2);
        let key = (self.place, self.light);
        let mut cache = self.backdrop.borrow_mut();
        let mut fade = self.fade.borrow_mut();
        if cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let px = scene::background(key.0, key.1, w as usize, h as usize)
                .unwrap_or_else(|| vec![rgb(0x12, 0x16, 0x2e); (w * h) as usize]);
            *fade = cache.take().map(|(_, old)| old);
            *cache = Some((key, px));
        }
        let Some((_, px)) = cache.as_ref() else {
            return;
        };
        let since = self.now - self.moved_at;
        if since >= FADE_MS {
            *fade = None;
        }
        let alpha = (since.clamp(0, FADE_MS) * 256 / FADE_MS) as u32;
        // The train jolts the picture down a pixel now and then.
        let bump = scene::bump(self.place, self.now);
        for y in 0..h {
            let at = ((y0 + y) * W + x0) as usize;
            let sy = (y - bump).max(0);
            let row = &px[(sy * w) as usize..((sy + 1) * w) as usize];
            match fade.as_ref() {
                Some(old) => {
                    let was = &old[(y * w) as usize..((y + 1) * w) as usize];
                    for (i, (a, b)) in was.iter().zip(row).enumerate() {
                        c.px[at + i] = mix(*a, *b, alpha);
                    }
                }
                None => c.px[at..at + w as usize].copy_from_slice(row),
            }
        }
    }

    fn pose(&self) -> Pose {
        let now = self.now;
        if self.dozing {
            return Pose {
                face: "blink",
                blink: false,
                talk: false,
                blush: false,
            };
        }
        let mut f = if self.thinking.is_some() {
            Mood::Neutral.pose()
        } else {
            self.current_mood().pose()
        };
        f.talk = self.talking() && (now / 140) % 2 == 0;
        let open = matches!(f.face, "base" | "surprised" | "smug" | "sad" | "angry");
        f.blink = open && (self.blink_at..self.blink_at + 140).contains(&now);
        f
    }

    /// Manga marks beside her head.
    fn marks(&self, c: &mut Canvas, gx: i32, gy: i32) {
        if self.dozing || self.thinking.is_some() {
            return;
        }
        let (fx, fy, fw, _) = sprite::face();
        let (x, y) = (gx + fx + fw - 2, gy + fy - 14);
        match self.current_mood() {
            Mood::Angry if (self.now / 250) % 2 == 0 => sprite::draw(c, "anger", x, y, 2, false),
            Mood::Worried | Mood::Sad => sprite::draw(c, "sweat", x + 2, y + 8, 2, false),
            Mood::Surprised => {
                c.text(x + 4, y - 8, Large, GOLD, "!");
            }
            _ => {}
        }
    }

    fn draw_particles(&self, c: &mut Canvas) {
        for p in &self.particles {
            let (x, y) = (p.x as i32, p.y as i32);
            let age = (self.now - p.born) as f32 / p.life.max(1) as f32;
            match p.kind {
                Kind::Heart => sprite::draw(c, "heart", x - 7, y - 6, 2, false),
                Kind::Sparkle => {
                    if age < 0.7 || (self.now / 60) % 2 == 0 {
                        sprite::draw(c, "sparkle", x - 2, y - 2, 1, false);
                    }
                }
                Kind::Note => silhouette(c, "note", x - 6, y - 7, 2, GOLD),
                Kind::Z => {
                    let col = mix(0xffff, INK, (age * 160.0) as u32);
                    if age > 0.45 {
                        c.text(x, y, Medium, col, "Z");
                    } else {
                        c.text(x, y, Small, col, "z");
                    }
                }
            }
        }
    }

    fn bubble_frame(&self, c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        // Tail toward her cheek, drawn first so the box covers its root.
        let (gx, gy) = self.girl_screen();
        let (fx, fy, fw, fh) = sprite::face();
        let tip = (gx + fx + fw - 6, gy + fy + fh * 3 / 5);
        let (a0, a1) = ((y + h / 2 - 5).max(y + 3), (y + h / 2 + 3).min(y + h - 4));
        for ay in a0..=a1 {
            c.line(x + 2, ay, tip.0, tip.1, PAPER);
        }
        c.line(x, a0 - 1, tip.0, tip.1, INK);
        c.line(x, a1 + 1, tip.0, tip.1, INK);
        c.fill_rect(x + 1, y, w - 2, h, PAPER);
        c.fill_rect(x, y + 1, w, h - 2, PAPER);
        c.hline(x + 2, y - 1, w - 4, INK);
        c.hline(x + 2, y + h, w - 4, INK);
        c.vline(x - 1, y + 2, h - 4, INK);
        c.vline(x + w, y + 2, h - 4, INK);
        for (px, py) in [
            (x, y),
            (x + w - 1, y),
            (x, y + h - 1),
            (x + w - 1, y + h - 1),
        ] {
            c.pixel(px, py, INK);
        }
        // Soft drop shadow along the bottom.
        c.blend_rect(x + 3, y + h + 1, w - 4, 2, 0, 90);
    }

    fn draw_bubble(&self, c: &mut Canvas) {
        let Some((x, y, w, h)) = self.bubble_rect() else {
            return;
        };
        self.bubble_frame(c, x, y, w, h);
        if self.thinking.is_some() {
            // DeepSeek's own "deep thinking" label, with a turning spinner.
            let tx = c.text_cjk(x + 8, y + 5, INK, "深度思考中");
            let dots = ((self.now / 300) % 4) as usize;
            c.text(tx, y + 5, Small, INK, &".".repeat(dots));
            let a = self.now as f32 / 150.0;
            let (sx, sy) = (x + w - 13, y + h / 2);
            for k in 0..3 {
                let b = a - k as f32 * 0.7;
                let col = mix(DEEPSEEK, PAPER, k * 80);
                c.fill_rect(
                    sx + (b.cos() * 5.0) as i32 - 1,
                    sy + (b.sin() * 5.0) as i32 - 1,
                    3,
                    3,
                    col,
                );
            }
            return;
        }
        let Some(s) = &self.speech else { return };
        let mut left = self.revealed();
        let mut end = (x + 8, y + 5);
        for (i, line) in s.lines.iter().enumerate() {
            if left == 0 {
                break;
            }
            let shown: String = line.chars().take(left).collect();
            left -= shown.chars().count();
            let ly = y + 5 + i as i32 * 18;
            end = (c.text_cjk(x + 8, ly, INK, &shown), ly);
        }
        if self.talking() && (self.now / 250) % 2 == 0 {
            c.fill_rect(end.0 + 1, end.1 + 2, 6, 12, DEEPSEEK);
        }
        if s.ai {
            // The little whale: this line came from the model.
            sprite::draw(c, "whale", x + w - 15, y + h - 9, 1, false);
        }
    }

    fn title(&self, c: &mut Canvas, p: &Palette) {
        let (x, y) = (STAGE_X + 1, STAGE_Y + 1);
        c.blend_rect(x, y, STAGE_W - 2, TITLE_H - 2, 0, 210);
        sprite::draw(c, "whale", x + 5, y + 5, 1, false);
        let tx = c.text_cjk(x + 24, y + 1, p.bright, self.brain.name());
        // Where she is; tapping the bar takes her somewhere else.
        c.text_cjk(tx + 6, y + 1, p.dim, &format!("@{}", self.place.name()));
        let (dot, label) = if !self.brain.online() {
            (p.dim, "offline")
        } else if self.brain.last_error.is_some() {
            (p.warn, "no link")
        } else if self.dozing {
            (p.dim, "zzz")
        } else {
            (p.ok, "online")
        };
        let right = c.text_right(STAGE_X + STAGE_W - 6, y + 1, Small, p.dim, label);
        let pulse = if self.brain.busy() && (self.now / 200) % 2 == 0 {
            p.bright
        } else {
            dot
        };
        c.fill_rect(right - 10, y + 6, 6, 6, pulse);
    }
}
