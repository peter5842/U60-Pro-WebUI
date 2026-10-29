//! The HOME companion: 咕嘎, DeepSeek's maid whale girl, at the top of HOME,
//! drawn from the owner's picture of her. Tapping is the only input: pat her
//! head, poke her, tap the scene to startle her, tap her speech bubble to
//! hurry or dismiss it, tap the title bar to take her somewhere else.
//! She goes about her day through six scenes (`scene.rs`) in the light of
//! the device clock, talks through an optional model (`brain.rs`), remarks
//! on what the router is doing, greets you after a while away and dozes off
//! late at night. There is nothing to feed, raise or level up.

mod brain;
mod draw;
mod lines;
mod scene;
pub mod sprite;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::stats::Model;
use brain::{Client, Config};
use lines::{cue, Ask, Trigger};
use scene::{Light, Place};
use sprite::{Part, Pose};

/// When she last saw you (unix seconds), for the greeting.
pub const STATE_PATH: &str = "/data/local/tmp/zte-pet-state.json";

/// Her scene on screen.
pub const STAGE_X: i32 = 4;
pub const STAGE_Y: i32 = 44;
pub const STAGE_W: i32 = 312;
pub const STAGE_H: i32 = 235;
/// First row below the room.
pub const BOTTOM: i32 = STAGE_Y + STAGE_H + 4;
/// The scene as (x, y, w, h).
pub const ROOM: (i32, i32, i32, i32) = (STAGE_X, STAGE_Y, STAGE_W, STAGE_H);
const TITLE_H: i32 = 20;
const FLOOR_Y: i32 = STAGE_Y + STAGE_H - 36;
/// Where her feet meet the floor.
const FEET_Y: i32 = FLOOR_Y + 24;
/// Her left edge at home; she wanders up to WANDER px right of it.
const HOME_X: i32 = STAGE_X + 8;
const WANDER: f32 = 14.0;

const CHAR_MS: i64 = 55;
const HOLD_MS: i64 = 6_500;
const THINK_TIMEOUT_MS: i64 = 32_000;
/// Taps come in bursts: the model hears about one at most this often.
const TAP_AI_GAP_MS: i64 = 10_000;
const CHATTER_IDLE_MS: i64 = 45_000;
const CHATTER_GAP_MS: i64 = 300_000;
const DOZE_IDLE_MS: i64 = 60_000;
const GREET_AWAY_S: u64 = 20 * 60;

const CD_BATTERY: usize = 0;
const CD_HOT: usize = 1;
const CD_CLIENT: usize = 2;
const CD_SURF: usize = 3;
const CD_SIGNAL: usize = 4;

/// Milliseconds since the process started: her animation clock.
pub fn clock() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
}

fn wall() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn night(hour: Option<u32>) -> bool {
    hour.is_some_and(|h| !(6..23).contains(&h))
}

fn deep_night(hour: Option<u32>) -> bool {
    hour.is_some_and(|h| (1..6).contains(&h))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Neutral,
    Happy,
    Shy,
    Smug,
    Excited,
    Sleepy,
    Sad,
    Angry,
    Worried,
    Surprised,
    Love,
}

impl Mood {
    fn parse(s: &str) -> Option<Mood> {
        Some(match s {
            "happy" => Mood::Happy,
            "shy" => Mood::Shy,
            "smug" => Mood::Smug,
            "excited" => Mood::Excited,
            "sleepy" => Mood::Sleepy,
            "sad" => Mood::Sad,
            "angry" => Mood::Angry,
            "worried" => Mood::Worried,
            "surprised" => Mood::Surprised,
            "love" => Mood::Love,
            _ => return None,
        })
    }

    fn pose(self) -> Pose {
        let f = |face, blush| Pose {
            face,
            blink: false,
            talk: false,
            blush,
        };
        match self {
            Mood::Neutral => f("base", false),
            Mood::Happy | Mood::Excited => f("happy", false),
            Mood::Shy => f("shy", false),
            Mood::Smug => f("smug", false),
            Mood::Sleepy => f("sleepy", false),
            Mood::Sad | Mood::Worried => f("sad", false),
            Mood::Angry => f("angry", false),
            Mood::Surprised => f("surprised", false),
            Mood::Love => f("happy", true),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Heart,
    Sparkle,
    Note,
    Z,
}

#[derive(Clone, Debug)]
struct Particle {
    kind: Kind,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    born: i64,
    life: i64,
}

#[derive(Clone, Debug)]
struct Speech {
    text: String,
    lines: Vec<String>,
    start: i64,
    ai: bool,
}

#[derive(Clone, Debug)]
struct Thinking {
    id: u64,
    since: i64,
    mood: Mood,
    fallback: String,
}

/// What the router looked like last time, so changes can be remarked on.
#[derive(Clone, Debug, Default)]
struct Seen {
    charging: Option<bool>,
    clients: Option<u32>,
    crit: Option<usize>,
    cooldown: [i64; 5],
}

/// The background without her, decoded per scene and light.
type Backdrop = Option<((Place, Light), Vec<u16>)>;

pub struct Pet {
    brain: Client,
    rng: u64,
    now: i64,
    hour: Option<u32>,
    /// Whether the deep-night check has run since the screen lit.
    looked: bool,
    x: f32,
    target_x: f32,
    mood: Mood,
    mood_until: i64,
    blink_at: i64,
    hop_at: i64,
    shake_until: i64,
    hum_until: i64,
    dozing: bool,
    particles: Vec<Particle>,
    speech: Option<Speech>,
    thinking: Option<Thinking>,
    pats: VecDeque<i64>,
    pokes: VecDeque<i64>,
    recent: VecDeque<String>,
    last_ai: i64,
    last_chatter: i64,
    last_touch: i64,
    next_idle: i64,
    next_watch: i64,
    next_puff: i64,
    seen: Seen,
    /// A reaction waiting for her to be free (the greeting).
    pending: Option<Trigger>,
    place: Place,
    light: Light,
    /// A place picked from the title bar, and the routine's place when it
    /// was picked: it holds until the routine moves on.
    chosen: Option<(Place, Place)>,
    /// When the scene or its light last changed, for the dissolve.
    moved_at: i64,
    backdrop: RefCell<Backdrop>,
    /// The previous background while it dissolves.
    fade: RefCell<Option<Vec<u16>>>,
}

impl Pet {
    fn new(brain: Client, now: i64, seed: u64) -> Pet {
        Pet {
            brain,
            rng: seed | 1,
            now,
            hour: None,
            looked: false,
            x: 0.0,
            target_x: 0.0,
            mood: Mood::Neutral,
            mood_until: 0,
            blink_at: now + 2500,
            hop_at: i64::MIN / 2,
            shake_until: 0,
            hum_until: 0,
            dozing: false,
            particles: Vec::new(),
            speech: None,
            thinking: None,
            pats: VecDeque::new(),
            pokes: VecDeque::new(),
            recent: VecDeque::new(),
            last_ai: i64::MIN / 2,
            last_chatter: now,
            last_touch: now,
            next_idle: now + 4000,
            next_watch: now,
            next_puff: now,
            seen: Seen::default(),
            pending: None,
            place: Place::at(None),
            light: Light::at(None),
            chosen: None,
            moved_at: i64::MIN / 2,
            backdrop: RefCell::new(None),
            fade: RefCell::new(None),
        }
    }

    /// Her model config, and when she last saw you, from /data.
    pub fn load() -> Pet {
        let config = Config::load(Path::new(brain::CONFIG_PATH));
        let seen = fs::read_to_string(STATE_PATH)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("seen").and_then(Value::as_u64));
        let mut pet = Pet::new(Client::new(config), clock(), wall() ^ 0x9e37_79b9_7f4a_7c15);
        pet.pending = match seen {
            None => Some(Trigger::Greet(None)),
            Some(s) => {
                let away = wall().saturating_sub(s);
                (away >= GREET_AWAY_S).then_some(Trigger::Greet(Some(away)))
            }
        };
        pet
    }

    /// Note that she saw you just now.
    pub fn save(&self) {
        let w = wall();
        if w == 0 {
            return;
        }
        let tmp = format!("{STATE_PATH}.tmp");
        if fs::write(&tmp, json!({ "seen": w }).to_string()).is_ok() {
            let _ = fs::rename(&tmp, STATE_PATH);
        }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn uniform(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.rand() >> 40) as f32 / (1u64 << 24) as f32 * (hi - lo)
    }

    fn chance(&mut self, p: f32) -> bool {
        self.uniform(0.0, 1.0) < p
    }

    fn set_mood(&mut self, mood: Mood, ms: i64) {
        self.mood = mood;
        self.mood_until = self.now + ms;
    }

    fn current_mood(&self) -> Mood {
        if self.now < self.mood_until {
            self.mood
        } else if self.now < self.hum_until {
            Mood::Happy
        } else if deep_night(self.hour) {
            Mood::Sleepy
        } else {
            Mood::Neutral
        }
    }

    fn spawn(&mut self, kind: Kind, x: f32, y: f32, vx: f32, vy: f32, life: i64) {
        if self.particles.len() < 80 {
            self.particles.push(Particle {
                kind,
                x,
                y,
                vx,
                vy,
                born: self.now,
                life,
            });
        }
    }

    // ---- geometry -------------------------------------------------------

    fn hop(&self) -> i32 {
        let t = (self.now - self.hop_at) as f32 / 420.0;
        if (0.0..1.0).contains(&t) {
            (t * (1.0 - t) * 40.0) as i32
        } else {
            0
        }
    }

    /// Top-left of her bounding box on screen.
    fn girl_screen(&self) -> (i32, i32) {
        let shake = if self.now < self.shake_until {
            [-2, 2, -1, 1][(self.now / 40 % 4) as usize]
        } else {
            0
        };
        let period = if self.dozing { 3400.0 } else { 1800.0 };
        let breath = ((self.now as f32 / period * TAU).sin() * 1.4).round() as i32;
        let (_, h) = sprite::size();
        (
            HOME_X + self.x.round() as i32 + shake,
            FEET_Y - h + breath - self.hop() + scene::bump(self.place, self.now),
        )
    }

    fn head_top(&self) -> (i32, i32) {
        let (x, y) = self.girl_screen();
        (x + sprite::size().0 / 2, y + 22)
    }

    fn part_at(&self, x: i32, y: i32) -> Option<Part> {
        let (gx, gy) = self.girl_screen();
        sprite::part_at(x - gx, y - gy)
    }

    fn inside(x: f32, y: f32) -> bool {
        x >= (STAGE_X + 2) as f32
            && x < (STAGE_X + STAGE_W - 2) as f32
            && y >= (STAGE_Y + TITLE_H) as f32
            && y < (STAGE_Y + STAGE_H - 2) as f32
    }

    // ---- speech -----------------------------------------------------------

    fn local_line(&mut self, lines: &[&str]) -> String {
        let n = lines.len().max(1);
        let start = (self.rand() % n as u64) as usize;
        let fill = |l: &str| {
            l.replace("{u}", self.brain.user())
                .replace("{n}", self.brain.name())
        };
        let fresh = (0..n)
            .map(|i| fill(lines[(start + i) % n]))
            .find(|l| !self.recent.contains(l));
        fresh.unwrap_or_else(|| fill(lines.first().copied().unwrap_or("")))
    }

    fn bubble_x(&self) -> i32 {
        HOME_X + self.x.round() as i32 + sprite::size().0 + 2
    }

    /// Width text may wrap to: one glyph short of the room, so closing
    /// punctuation can hang without leaving it.
    fn bubble_text_width(&self) -> i32 {
        (STAGE_X + STAGE_W - 6 - self.bubble_x() - 32).max(64)
    }

    fn say(&mut self, text: String, mood: Mood, ai: bool) {
        let lines = crate::cjk::wrap(&text, self.bubble_text_width());
        let ms = text.chars().count() as i64 * CHAR_MS + HOLD_MS;
        self.set_mood(mood, ms);
        self.recent.push_back(text.clone());
        while self.recent.len() > 4 {
            self.recent.pop_front();
        }
        self.speech = Some(Speech {
            text,
            lines,
            start: self.now,
            ai,
        });
    }

    fn prompt(&self, event: &str, m: &Model) -> String {
        let mut s = lines::device(m);
        s += &format!("\n[场景] 你现在在{}", self.place.name());
        if !self.recent.is_empty() {
            let recent: Vec<&str> = self.recent.iter().map(String::as_str).collect();
            s += &format!("\n[你最近说过] {}", recent.join(" / "));
        }
        s += &format!("\n[事件] {}", event.replace("{u}", self.brain.user()));
        s
    }

    fn react(&mut self, t: Trigger, m: &Model) {
        let c = cue(&t);
        if self.thinking.is_some() {
            // A line is already on its way; just pull the face.
            self.set_mood(c.mood, 3000);
            return;
        }
        let ask = match c.ask {
            Ask::Always => true,
            Ask::Sometimes => self.now - self.last_ai >= TAP_AI_GAP_MS,
        };
        if ask && self.brain.ready(self.now) {
            let msg = self.prompt(&c.event, m);
            if let Some(id) = self.brain.ask(self.now, msg) {
                let fallback = self.local_line(c.local);
                self.thinking = Some(Thinking {
                    id,
                    since: self.now,
                    mood: c.mood,
                    fallback,
                });
                self.last_ai = self.now;
                self.speech = None;
                self.set_mood(c.mood, 3000);
                return;
            }
        }
        let line = self.local_line(c.local);
        self.say(line, c.mood, false);
    }

    fn poll_brain(&mut self) {
        while let Some(reply) = self.brain.poll() {
            let Some(th) = self.thinking.take_if(|t| t.id == reply.id) else {
                continue;
            };
            match reply.line {
                Ok((text, mood)) => {
                    let mood = Mood::parse(&mood).unwrap_or(th.mood);
                    self.say(text, mood, true);
                }
                Err(e) => {
                    // stderr is /tmp/zte-screen/panel.log; the error never holds the key.
                    eprintln!("pet: model call failed: {e}");
                    self.say(th.fallback, th.mood, false);
                }
            }
        }
        if self
            .thinking
            .as_ref()
            .is_some_and(|t| self.now - t.since > THINK_TIMEOUT_MS)
        {
            if let Some(th) = self.thinking.take() {
                self.say(th.fallback, th.mood, false);
            }
        }
    }

    fn speech_done(&self) -> bool {
        self.speech
            .as_ref()
            .is_none_or(|s| self.now - s.start > s.text.chars().count() as i64 * CHAR_MS + HOLD_MS)
    }

    /// Characters of the current line revealed so far.
    fn revealed(&self) -> usize {
        self.speech
            .as_ref()
            .map_or(0, |s| ((self.now - s.start) / CHAR_MS).max(0) as usize)
    }

    fn talking(&self) -> bool {
        self.speech
            .as_ref()
            .is_some_and(|s| self.revealed() < s.text.chars().count())
    }

    fn quiet(&self) -> bool {
        self.thinking.is_none() && self.speech_done() && !self.dozing
    }

    /// The bubble beside her face: thinking dots, or the current line.
    fn bubble_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let (w, h) = if self.thinking.is_some() {
            (crate::cjk::width("深度思考中") + 40, 26)
        } else {
            let s = self.speech.as_ref().filter(|_| !self.speech_done())?;
            let w = s.lines.iter().map(|l| crate::cjk::width(l)).max();
            (w.unwrap_or(0).max(32) + 16, s.lines.len() as i32 * 18 + 10)
        };
        let (_, fy, _, fh) = sprite::face();
        let face = FEET_Y - sprite::size().1 + fy + fh / 2;
        let y = (face - h / 2)
            .min(STAGE_Y + STAGE_H - 6 - h)
            .max(STAGE_Y + TITLE_H + 6);
        Some((self.bubble_x(), y, w, h))
    }

    /// A tap on the bubble: show the whole line, or put it away.
    fn hurry(&mut self) {
        let now = self.now;
        if let Some(s) = &mut self.speech {
            let full = s.text.chars().count() as i64 * CHAR_MS;
            if now - s.start < full {
                s.start = now - full;
            } else {
                self.speech = None;
            }
        }
    }

    // ---- input --------------------------------------------------------------

    /// A tap on HOME. Returns true when it landed in her room.
    pub fn tap(&mut self, x: i32, y: i32, m: &Model) -> bool {
        self.now = clock().max(self.now);
        if !(STAGE_X..STAGE_X + STAGE_W).contains(&x) || !(STAGE_Y..STAGE_Y + STAGE_H).contains(&y)
        {
            return false;
        }
        self.last_touch = self.now;
        if y < STAGE_Y + TITLE_H {
            self.travel(m);
            return true;
        }
        if let Some((bx, by, bw, bh)) = self.bubble_rect() {
            if (bx..bx + bw).contains(&x) && (by..by + bh).contains(&y) {
                self.hurry();
                return true;
            }
        }
        match self.part_at(x, y) {
            Some(_) if self.dozing => self.wake(m),
            Some(Part::Head) => self.pat(m),
            Some(Part::Body) => self.poke(m),
            None => self.glance(x, y),
        }
        true
    }

    fn window(q: &mut VecDeque<i64>, now: i64, ms: i64) -> u32 {
        q.push_back(now);
        while q.front().is_some_and(|t| now - t > ms) {
            q.pop_front();
        }
        q.len() as u32
    }

    fn pat(&mut self, m: &Model) {
        let n = Self::window(&mut self.pats, self.now, 30_000);
        self.hop_at = self.now;
        let (hx, hy) = self.head_top();
        for _ in 0..2 {
            let x = hx as f32 + self.uniform(-36.0, 36.0);
            let vx = self.uniform(-20.0, 20.0);
            let vy = self.uniform(-60.0, -35.0);
            self.spawn(Kind::Heart, x, hy as f32, vx, vy, 1300);
        }
        self.react(Trigger::Pat(n), m);
    }

    fn poke(&mut self, m: &Model) {
        let n = Self::window(&mut self.pokes, self.now, 2500);
        if n >= 6 {
            self.pokes.clear();
            self.shake_until = self.now + 700;
            self.react(Trigger::PokeSpam(n), m);
            return;
        }
        if n >= 3 {
            self.shake_until = self.now + 350;
        } else {
            self.hop_at = self.now;
        }
        self.react(Trigger::Poke(n), m);
    }

    fn wake(&mut self, m: &Model) {
        self.dozing = false;
        self.hop_at = self.now;
        self.react(Trigger::Wake, m);
    }

    /// Off to the next place; she stays there until her routine moves on.
    fn travel(&mut self, m: &Model) {
        let next = self.place.next();
        self.chosen = Some((next, Place::at(self.hour)));
        self.go(next, self.light);
        self.dozing = false;
        self.hop_at = self.now;
        self.react(Trigger::Travel(next), m);
    }

    fn go(&mut self, place: Place, light: Light) {
        if (place, light) != (self.place, self.light) {
            self.place = place;
            self.light = light;
            self.moved_at = self.now;
        }
    }

    fn glance(&mut self, x: i32, y: i32) {
        self.spawn(Kind::Sparkle, x as f32, y as f32, 0.0, -12.0, 600);
        if !self.dozing && self.speech_done() && self.thinking.is_none() {
            self.set_mood(Mood::Surprised, 700);
        }
    }

    /// The screen went dark (idle or power key).
    pub fn on_dark(&mut self) {
        self.speech = None;
        self.thinking = None;
        self.save();
    }

    /// The screen lit again: greet after a long absence, doze at night.
    pub fn on_wake(&mut self) {
        self.now = clock().max(self.now);
        let away = (self.now - self.last_touch).max(0) as u64 / 1000;
        self.last_touch = self.now;
        self.looked = false;
        if away >= GREET_AWAY_S {
            self.pending = Some(Trigger::Greet(Some(away)));
        }
    }

    /// Test hook: `echo pat > /tmp/zte-screen/pet.request` and so on.
    pub fn request(&mut self, action: &str, m: &Model) {
        self.now = clock().max(self.now);
        self.last_touch = self.now;
        let action = action.trim();
        if self.dozing && matches!(action, "pat" | "poke" | "spam") {
            // Like a real tap: the first one only wakes her.
            self.wake(m);
            return;
        }
        match action {
            "pat" => self.pat(m),
            "poke" => self.poke(m),
            "spam" => {
                for _ in 0..6 {
                    self.poke(m);
                }
            }
            "wake" => self.wake(m),
            "doze" => self.dozing = true,
            "greet" => self.react(Trigger::Greet(Some(3 * 3600)), m),
            "chatter" => self.react(Trigger::Chatter, m),
            "travel" => self.travel(m),
            _ => {}
        }
    }

    // ---- time -----------------------------------------------------------------

    pub fn tick(&mut self, now: i64, m: &Model) {
        let dt = (now - self.now).clamp(0, 250) as f32 / 1000.0;
        self.now = now;
        let knew = self.hour.is_some();
        if let Some(h) = lines::hour(m) {
            self.hour = Some(h);
        }
        self.follow_routine(knew, m);
        if !self.looked && self.hour.is_some() {
            self.looked = true;
            if deep_night(self.hour) {
                // Opened in the small hours: she is asleep, and a tap wakes her.
                self.dozing = true;
                self.pending = None;
            }
        }
        if now >= self.next_watch {
            self.next_watch = now + 10_000;
            self.watch(m);
            self.routine(m);
        }
        self.poll_brain();
        if self.pending.is_some() && self.hour.is_some() && self.quiet() {
            if let Some(t) = self.pending.take() {
                self.react(t, m);
            }
        }
        self.animate(dt);
    }

    /// Keep her where her day says, unless she was sent somewhere and the
    /// routine has not moved on since. `knew` is whether the clock was known
    /// before this tick: the first reading moves her without a fuss.
    fn follow_routine(&mut self, knew: bool, m: &Model) {
        let routine = Place::at(self.hour);
        if self.chosen.is_some_and(|(_, then)| then != routine) {
            self.chosen = None;
        }
        let (place, light) = (
            self.chosen.map_or(routine, |(p, _)| p),
            Light::at(self.hour),
        );
        let moved = place != self.place;
        let changed = moved || light != self.light;
        self.go(place, light);
        if changed && !knew {
            self.moved_at = i64::MIN / 2;
        } else if moved && self.quiet() {
            self.react(Trigger::Travel(place), m);
        }
    }

    fn cool(&mut self, k: usize, ms: i64) -> bool {
        if self.now >= self.seen.cooldown[k] {
            self.seen.cooldown[k] = self.now + ms;
            true
        } else {
            false
        }
    }

    /// Router events, checked every 10 s with per-kind cooldowns. Changes
    /// count only after the first look.
    fn watch(&mut self, m: &Model) {
        let mut fire: Option<Trigger> = None;
        let charging = m.battery.percent.map(|_| m.battery.external_power);
        if let (Some(before), Some(after)) = (self.seen.charging, charging) {
            if before != after {
                fire = Some(if after {
                    Trigger::Charging
                } else {
                    Trigger::Unplugged
                });
            }
        }
        self.seen.charging = charging.or(self.seen.charging);
        if let (Some(before), Some(after)) = (self.seen.clients, m.clients) {
            if after > before && self.cool(CD_CLIENT, 120_000) {
                fire = fire.or(Some(Trigger::NewClient(after)));
            }
        }
        self.seen.clients = m.clients.or(self.seen.clients);
        let crit: Vec<String> = crate::ui::alerts(m)
            .into_iter()
            .filter(|(l, _)| *l == crate::ui::Level::Crit)
            .map(|(_, t)| t)
            .collect();
        if self.seen.crit.is_some_and(|n| crit.len() > n) {
            fire = fire.or(Some(Trigger::Alert(crit.join("、"))));
        }
        self.seen.crit = Some(crit.len());
        if let Some(p) = m.battery.percent {
            if p <= 20 && !m.battery.external_power && self.cool(CD_BATTERY, 900_000) {
                fire = fire.or(Some(Trigger::LowBattery(p)));
            }
        }
        if let Some(t) = m.temp_cpu {
            if t >= 68.0 && self.cool(CD_HOT, 900_000) {
                fire = fire.or(Some(Trigger::Hot(t)));
            }
        }
        if let Some(d) = m.dl {
            let mbps = d * 8.0 / 1e6;
            if mbps >= 50.0 && self.cool(CD_SURF, 600_000) {
                fire = fire.or(Some(Trigger::Surfing(mbps)));
            }
        }
        if let Some(rsrp) = m.radio.as_ref().and_then(|r| r.rsrp) {
            if rsrp < -112.0 && self.cool(CD_SIGNAL, 1_200_000) {
                fire = fire.or(Some(Trigger::WeakSignal(rsrp)));
            }
        }
        if let Some(t) = fire {
            if self.quiet() {
                self.react(t, m);
            }
        }
    }

    /// Doze late at night, wake with the morning, chat when left alone.
    fn routine(&mut self, m: &Model) {
        let idle = self.now - self.last_touch;
        if self.dozing {
            if !night(self.hour) {
                self.dozing = false;
            }
            return;
        }
        if night(self.hour) && idle > DOZE_IDLE_MS && self.quiet() {
            self.dozing = true;
            return;
        }
        if self.quiet()
            && idle > CHATTER_IDLE_MS
            && self.now - self.last_chatter > CHATTER_GAP_MS
            && self.now - self.last_ai > 60_000
        {
            self.last_chatter = self.now;
            self.react(Trigger::Chatter, m);
        }
    }

    fn animate(&mut self, dt: f32) {
        let now = self.now;
        if now >= self.blink_at + 140 {
            self.blink_at = now + 2200 + (self.rand() % 4000) as i64;
        }
        let busy = self.thinking.is_some() || !self.speech_done();
        // Little things she does on her own.
        if now >= self.next_idle {
            self.next_idle = now + 4500 + (self.rand() % 7000) as i64;
            if !self.dozing && !busy {
                match self.rand() % 6 {
                    0 | 1 => self.target_x = self.uniform(0.0, WANDER),
                    2 => self.set_mood(Mood::Smug, 1400),
                    3 => self.hop_at = now,
                    _ => self.hum_until = now + 3200,
                }
            }
        }
        if !busy {
            let step = 12.0 * dt;
            self.x += (self.target_x - self.x).clamp(-step, step);
        }
        if now >= self.next_puff {
            self.next_puff = now + 450;
            let (hx, hy) = self.head_top();
            let (hx, hy) = (hx as f32, hy as f32);
            if self.dozing {
                if self.chance(0.35) {
                    self.spawn(Kind::Z, hx + 40.0, hy + 10.0, 9.0, -13.0, 2600);
                }
            } else if now < self.hum_until {
                let vx = self.uniform(8.0, 22.0);
                self.spawn(Kind::Note, hx + 44.0, hy + 36.0, vx, -26.0, 1600);
            } else if self.current_mood() == Mood::Love && self.chance(0.5) {
                let x = hx + self.uniform(-40.0, 40.0);
                self.spawn(Kind::Heart, x, hy, 0.0, -28.0, 1400);
            } else if self.current_mood() == Mood::Excited && self.chance(0.6) {
                let x = hx + self.uniform(-60.0, 60.0);
                let y = hy + self.uniform(-10.0, 60.0);
                self.spawn(Kind::Sparkle, x, y, 0.0, -8.0, 700);
            }
        }
        for p in &mut self.particles {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            if matches!(p.kind, Kind::Note | Kind::Z) {
                p.x += ((now - p.born) as f32 / 260.0).sin() * 0.4;
            }
        }
        self.particles
            .retain(|p| now - p.born < p.life && Pet::inside(p.x, p.y));
    }

    /// Redraw interval: smooth while something moves, slower at rest.
    pub fn frame_ms(&self) -> i64 {
        let now = self.now;
        let lively = self.thinking.is_some()
            || self.talking()
            || now - self.last_touch < 3000
            || now < self.shake_until
            || now - self.hop_at < 500
            || (self.x - self.target_x).abs() > 0.5
            || !self.particles.is_empty();
        if now - self.moved_at < draw::FADE_MS || lively {
            66
        } else if self.place == Place::Train && !self.dozing {
            // The view from the window has to scroll smoothly.
            83
        } else if self.dozing {
            200
        } else {
            125
        }
    }

    /// Her posed for `--preview`, run forward on her own clock. `scene` is a
    /// moment ("talk", "pat", "doze" …) or a place at an hour ("cafe@12").
    pub fn demo(scene: &str, m: &Model) -> Pet {
        let mut m = m.clone();
        let at = scene.split_once('@').and_then(|(place, hour)| {
            let place = Place::ALL.into_iter().find(|p| p.key() == place)?;
            Some((place, hour.parse::<u32>().ok()?))
        });
        let time = match (scene, at) {
            (_, Some((_, h))) => format!("{h:02}:30:00"),
            ("pat" | "angry" | "think" | "day", _) => "14:20:00".into(),
            ("dusk", _) => "17:40:00".into(),
            ("doze", _) => "02:30:00".into(),
            _ => String::new(),
        };
        if !time.is_empty() {
            m.clock = time;
        }
        let mut p = Pet::new(Client::new(None), 0, 7);
        p.looked = true;
        if let Some((place, h)) = at {
            p.chosen = Some((place, Place::at(Some(h))));
        }
        let run = |p: &mut Pet, ms: i64| {
            for _ in 0..ms / 50 {
                let now = p.now + 50;
                p.tick(now, &m);
            }
        };
        run(&mut p, 3000);
        match scene {
            "talk" => {
                p.say(
                    "主人，CPU 才 49 度，我的脑子还很冷静，可以陪你聊一整晚~".into(),
                    Mood::Smug,
                    true,
                );
                run(&mut p, 2000);
            }
            "think" => {
                p.thinking = Some(Thinking {
                    id: 1,
                    since: p.now,
                    mood: Mood::Happy,
                    fallback: String::new(),
                });
                run(&mut p, 3400);
            }
            "pat" => {
                p.pat(&m);
                run(&mut p, 450);
            }
            "angry" => {
                for _ in 0..4 {
                    p.poke(&m);
                    run(&mut p, 100);
                }
                run(&mut p, 900);
            }
            "doze" => {
                p.dozing = true;
                run(&mut p, 3600);
            }
            "day" | "dusk" => {
                p.hum_until = p.now + 3000;
                run(&mut p, 1500);
            }
            _ => {}
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pet() -> Pet {
        Pet::new(Client::new(None), 1_000, 42)
    }

    fn head(p: &Pet) -> (i32, i32) {
        let (x, y) = p.girl_screen();
        let (fx, fy, fw, fh) = sprite::face();
        (x + fx + fw / 2, y + fy + fh / 2)
    }

    fn body(p: &Pet) -> (i32, i32) {
        let (x, y) = p.girl_screen();
        let (w, h) = sprite::size();
        (x + w / 2, y + h - 40)
    }

    #[test]
    fn pats_and_pokes_get_an_answer() {
        let m = Model::default();
        let mut p = pet();
        let (x, y) = head(&p);
        assert!(p.tap(x, y, &m));
        assert!(p.speech.is_some(), "offline she answers with a local line");
        assert!(p.hop_at == p.now, "she bounces");
        p.speech = None;
        let (x, y) = body(&p);
        assert!(p.tap(x, y, &m));
        assert!(p.speech.is_some());
        assert!(
            !p.tap(10, BOTTOM + 40, &m),
            "taps below the room are not hers"
        );
    }

    #[test]
    fn a_burst_of_pokes_makes_her_cross() {
        let m = Model::default();
        let mut p = pet();
        for _ in 0..6 {
            p.speech = None;
            let (x, y) = body(&p);
            p.tap(x, y, &m);
        }
        assert!(p.shake_until > p.now);
        assert_eq!(p.current_mood(), Mood::Angry);
    }

    #[test]
    fn tapping_the_bubble_hurries_then_dismisses_it() {
        let m = Model::default();
        let mut p = pet();
        p.say("一二三四五六七八九十".into(), Mood::Happy, false);
        assert!(p.talking());
        let (bx, by, _, _) = p.bubble_rect().unwrap();
        p.tap(bx + 4, by + 4, &m);
        assert!(!p.talking() && p.speech.is_some());
        p.tap(bx + 4, by + 4, &m);
        assert!(p.speech.is_none());
    }

    #[test]
    fn long_lines_stay_inside_the_room() {
        let mut p = pet();
        p.say("啊".repeat(brain::MAX_LINE), Mood::Happy, false);
        let (x, y, w, h) = p.bubble_rect().unwrap();
        assert!(x + w <= STAGE_X + STAGE_W - 2, "{x}+{w}");
        assert!(y >= STAGE_Y + TITLE_H && y + h <= STAGE_Y + STAGE_H);
    }

    #[test]
    fn she_dozes_in_the_small_hours_and_wakes_on_a_tap() {
        let m = Model {
            clock: "02:10:00".into(),
            ..Model::default()
        };
        let mut p = pet();
        p.pending = Some(Trigger::Greet(Some(5000)));
        p.tick(p.now + 50, &m);
        assert!(p.dozing);
        assert!(p.pending.is_none(), "no greeting while she sleeps");
        let (x, y) = head(&p);
        p.tap(x, y, &m);
        assert!(!p.dozing);
        assert!(p.speech.is_some());
    }

    #[test]
    fn the_greeting_waits_for_the_clock() {
        let mut m = Model::default();
        let mut p = pet();
        p.pending = Some(Trigger::Greet(Some(3600)));
        p.tick(p.now + 50, &m);
        assert!(p.pending.is_some(), "no clock yet");
        m.clock = "10:00:00".into();
        p.tick(p.now + 50, &m);
        assert!(p.pending.is_none());
        assert!(p.speech.is_some());
    }

    #[test]
    fn router_changes_are_remarked_on_after_the_first_look() {
        let mut m = Model::default();
        m.battery.percent = Some(80);
        m.clients = Some(2);
        let mut p = pet();
        p.watch(&m);
        assert!(p.speech.is_none(), "the first look only records");
        m.battery.external_power = true;
        p.watch(&m);
        assert!(p.speech.is_some());
    }
}
