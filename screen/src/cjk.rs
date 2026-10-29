//! 16 px CJK glyphs for the pet's speech (a GB2312 subset of GNU Unifont,
//! built by tools/cjk_subset.py). ASCII keeps using Spleen 8x16, so mixed
//! text lines up on the same 16 px grid.

static DATA: &[u8] = include_bytes!("../fonts/cjk16.bin");

/// Stand-in for characters the subset lacks.
const MISSING: char = '\u{25a1}';

fn count() -> usize {
    u32::from_le_bytes([DATA[4], DATA[5], DATA[6], DATA[7]]) as usize
}

fn code_at(i: usize) -> u32 {
    let o = 8 + i * 4;
    u32::from_le_bytes([DATA[o], DATA[o + 1], DATA[o + 2], DATA[o + 3]])
}

fn index(c: char) -> Option<usize> {
    let (mut lo, mut hi) = (0, count());
    let want = c as u32;
    while lo < hi {
        let mid = (lo + hi) / 2;
        match code_at(mid).cmp(&want) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Advance width and 16 rows (bit 15 = leftmost pixel) of a non-ASCII
/// character, falling back to a hollow box.
pub fn glyph(c: char) -> (i32, [u16; 16]) {
    let i = index(c).or_else(|| index(MISSING)).unwrap_or(0);
    let n = count();
    let width = DATA[8 + n * 4 + i] as i32;
    let base = 8 + n * 5 + i * 32;
    let mut rows = [0u16; 16];
    for (r, row) in rows.iter_mut().enumerate() {
        *row = u16::from_be_bytes([DATA[base + r * 2], DATA[base + r * 2 + 1]]);
    }
    (width, rows)
}

/// Whether `c` has a real glyph (ASCII text or the CJK subset).
pub fn has(c: char) -> bool {
    c.is_ascii_graphic() || c == ' ' || index(c).is_some()
}

/// Width in pixels of `c` in mixed text.
pub fn char_width(c: char) -> i32 {
    if c.is_ascii() {
        8
    } else {
        glyph(c).0
    }
}

pub fn width(s: &str) -> i32 {
    s.chars().map(char_width).sum()
}

/// Closing punctuation that must not start a line.
fn closing(c: char) -> bool {
    "，。！？、；：）」』】～…,.!?;:)~".contains(c)
}

/// Break `s` into lines at most `max` pixels wide. Closing punctuation may
/// hang one glyph past the edge rather than start a line.
pub fn wrap(s: &str, max: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > max && !line.is_empty() && !closing(c) {
            lines.push(std::mem::take(&mut line));
            w = 0;
            if c == ' ' {
                continue;
            }
        }
        line.push(c);
        w += cw;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_holds_common_chinese_and_punctuation() {
        for c in "鲸鱼娘咕嘎主人深度思考摸头信号，。！？～…喵哒".chars() {
            assert!(index(c).is_some(), "{c} missing");
        }
        assert!(count() > 7000);
    }

    #[test]
    fn missing_characters_fall_back_to_a_box() {
        assert_eq!(glyph('\u{1f600}'), glyph(MISSING));
        assert_ne!(glyph('鲸'), glyph(MISSING));
        assert_eq!(glyph('鲸').0, 16);
    }

    #[test]
    fn wrap_respects_width_and_never_starts_with_closing_punctuation() {
        let lines = wrap("船长别戳啦，肚子要被你戳出10MB浪花啦！", 16 * 6);
        assert!(lines.len() >= 3);
        for l in &lines {
            assert!(width(l) <= 16 * 7, "{l}");
            assert!(!closing(l.chars().next().unwrap()), "{l}");
        }
        assert_eq!(lines.concat(), "船长别戳啦，肚子要被你戳出10MB浪花啦！");
        assert_eq!(width("ab中"), 32);
    }
}
