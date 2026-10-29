#!/usr/bin/env python3
"""Build screen/fonts/cjk16.bin, the 16 px CJK font of the pet's speech bubble.

The source font is not vendored. Fetch the pinned GNU Unifont release (dual
licensed SIL OFL 1.1 / GPLv2+ with the font embedding exception; the OFL text
is screen/fonts/LICENSE.unifont), unpack it, and run:

    curl -LO https://ftp.gnu.org/gnu/unifont/unifont-16.0.04/unifont-16.0.04.bdf.gz
    gunzip unifont-16.0.04.bdf.gz
    python3 screen/tools/cjk_subset.py unifont-16.0.04.bdf screen/fonts/cjk16.bin

Kept: every character of GB2312 (simplified Chinese, 7445 code points), CJK
punctuation, fullwidth forms and a short list of chat characters GB2312 lacks.
ASCII stays with Spleen 8x16.

Layout, little endian: b"CJK1", u32 count, then `count` u32 code points
(sorted), `count` u8 advance widths (8 or 16), and `count` glyphs of 16 rows,
each a big-endian u16 whose most significant bit is the leftmost pixel.
"""
import struct
import sys
from pathlib import Path

EXTRA_RANGES = [
    (0x2010, 0x2027),  # dashes, quotes, ellipsis
    (0x2190, 0x2199),  # arrows
    (0x2600, 0x266F),  # sun, stars, hearts, notes
    (0x3000, 0x303F),  # CJK punctuation
    (0xFF01, 0xFF5E),  # fullwidth forms
]
# Chat characters outside GB2312 that the model likes to use.
EXTRA_CHARS = "哒嘚喔哟咩嗷嗯嘤嘞啵噢咯吖叻呗呦怼撸薅嗨嘻叭哔喵咻唷嘿呐嚯噗咕嘟嘣溜啾喽瞅"


def gb2312():
    out = set()
    for hi in range(0xA1, 0xF8):
        for lo in range(0xA1, 0xFF):
            try:
                out.add(ord(bytes([hi, lo]).decode('gb2312')))
            except UnicodeDecodeError:
                pass
    return out


def parse(path):
    glyphs = {}
    lines = Path(path).read_text().splitlines()
    i = 0
    while i < len(lines):
        if not lines[i].startswith('STARTCHAR'):
            i += 1
            continue
        code, width, rows = None, None, []
        i += 1
        while not lines[i].startswith('ENDCHAR'):
            line = lines[i]
            if line.startswith('ENCODING '):
                code = int(line.split()[1])
            elif line.startswith('BBX '):
                w, h, _, _ = (int(v) for v in line.split()[1:])
                if h != 16:
                    raise ValueError(f'U+{code:04X}: height {h}')
                width = w
            elif line.strip() == 'BITMAP':  # Unifont writes "BITMAP "
                i += 1
                while not lines[i].startswith('ENDCHAR'):
                    rows.append(int(lines[i], 16))
                    i += 1
                break
            i += 1
        if code is not None and width in (8, 16):
            if len(rows) != 16:
                raise ValueError(f'U+{code:04X}: {len(rows)} rows')
            # Normalise to 16-bit rows, leftmost pixel in the top bit.
            glyphs[code] = (width, [r << 8 if width == 8 else r for r in rows])
        i += 1
    return glyphs


def main():
    src, dst = sys.argv[1], sys.argv[2]
    glyphs = parse(src)
    wanted = gb2312() | {ord(c) for c in EXTRA_CHARS}
    for lo, hi in EXTRA_RANGES:
        wanted |= set(range(lo, hi + 1))
    wanted = {c for c in wanted if c > 0x7E}
    codes = sorted(c for c in wanted if c in glyphs)
    missing = sorted(wanted - set(codes))
    out = bytearray(b'CJK1' + struct.pack('<I', len(codes)))
    out += b''.join(struct.pack('<I', c) for c in codes)
    out += bytes(glyphs[c][0] for c in codes)
    for c in codes:
        out += b''.join(struct.pack('>H', r) for r in glyphs[c][1])
    Path(dst).write_bytes(out)
    print(f'{len(codes)} glyphs, {len(out)} bytes, {len(missing)} wanted but absent', file=sys.stderr)


if __name__ == '__main__':
    main()
