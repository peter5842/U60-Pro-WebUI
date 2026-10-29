#!/usr/bin/env python3
"""Build screen/assets/guga.bin, 咕嘎's raster art for the HOME companion.

Inputs are not vendored: the owner's reference picture (a chibi maid whale
girl on a plain white background) and one face-only edit of it per
expression, generated with Codex's image tool from the reference ("keep the
exact same character, pose, outfit, framing and white background; change
only her face"): blink, happy, talk, angry, surprised, smug, sleepy, sad, shy.

    python3 screen/tools/guga_sprite.py REF.jpg VARIANTS_DIR screen/assets/guga.bin [PREVIEW_DIR]

Every picture is cut from its background, cropped to the character and
scaled to HEIGHT px; edits are re-aligned to the base on the hair around the
face. The base keeps the reference itself. Each expression contributes its
face (feathered), `blink` also its eyes alone and `talk` its mouth alone, so
a blink or a spoken syllable can go over any expression.

Layout, little endian: b"GUGA", u16 count, then per image: u8 name length,
name, i16 x, i16 y, u16 w, u16 h, and w*h pixels of (u16 RGB565, u8 alpha).
Needs Pillow and NumPy (SciPy optional).
"""
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

HEIGHT = 186
# Regions on the HEIGHT-px base, as (x, y, w, h).
FACE = (27, 46, 60, 38)
EYES = (30, 54, 52, 18)
MOUTH = (46, 72, 22, 10)
# Hair around the face, for aligning an edit to the base.
RING = (18, 30, 78, 60)
EXPRESSIONS = ["blink", "happy", "talk", "angry", "surprised", "smug", "sleepy", "sad", "shy"]


def cutout(path):
    img = Image.open(path).convert("RGB")
    a = np.asarray(img).astype(np.int16)
    lo, hi = a.min(axis=2), a.max(axis=2)
    light = (lo > 226) & (hi - lo < 28)
    try:
        from scipy import ndimage

        lab, _ = ndimage.label(light)
        edge = np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]]))
        bg = np.isin(lab, edge[edge != 0])
    except ImportError:
        bg = np.zeros_like(light)
        bg[0], bg[-1], bg[:, 0], bg[:, -1] = light[0], light[-1], light[:, 0], light[:, -1]
        while True:
            grow = bg.copy()
            grow[1:] |= bg[:-1]
            grow[:-1] |= bg[1:]
            grow[:, 1:] |= bg[:, :-1]
            grow[:, :-1] |= bg[:, 1:]
            grow &= light
            if (grow == bg).all():
                break
            bg = grow
    alpha = (~bg).astype(np.float32)
    near = np.zeros_like(bg)
    near[1:] |= bg[:-1]
    near[:-1] |= bg[1:]
    near[:, 1:] |= bg[:, :-1]
    near[:, :-1] |= bg[:, 1:]
    rim = near & ~bg
    white = np.clip((lo.astype(np.float32) - 150) / 90, 0, 1)
    alpha[rim] = 1 - white[rim] * 0.85
    ys, xs = np.nonzero(alpha > 0.05)
    y0, y1, x0, x1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
    rgba = np.dstack([a.astype(np.uint8), (alpha * 255).astype(np.uint8)])[y0:y1, x0:x1]
    im = Image.fromarray(rgba, "RGBA").convert("RGBa")
    w = round((x1 - x0) * HEIGHT / (y1 - y0))
    return np.asarray(im.resize((w, HEIGHT), Image.LANCZOS).convert("RGBA")).astype(np.float32)


def fit(v, width):
    """Pad or trim an edit to the base width, keeping it centred."""
    if v.shape[1] == width:
        return v
    out = np.zeros((v.shape[0], width, 4), np.float32)
    d = (width - v.shape[1]) // 2
    if d >= 0:
        out[:, d : d + v.shape[1]] = v
    else:
        out[:] = v[:, -d : -d + width]
    return out


def align(base, v):
    x, y, w, h = RING
    fx, fy, fw, fh = FACE
    mask = np.zeros(base.shape[:2], bool)
    mask[y : y + h, x : x + w] = True
    mask[fy : fy + fh, fx : fx + fw] = False
    best = None
    for dy in range(-3, 4):
        for dx in range(-3, 4):
            s = np.roll(np.roll(v, dy, 0), dx, 1)
            err = np.abs(s[..., :3] - base[..., :3])[mask].mean()
            if best is None or err < best[0]:
                best = (err, dx, dy, s)
    return best


def feather(w, h, edge):
    yy, xx = np.mgrid[0:h, 0:w]
    d = np.minimum.reduce([xx + 1, yy + 1, w - xx, h - yy]).astype(np.float32)
    return np.clip(d / edge, 0, 1)


def patch(src, rect, edge):
    x, y, w, h = rect
    px = src[y : y + h, x : x + w].copy()
    px[..., 3] *= feather(w, h, edge)
    return x, y, px


def rgb565(px):
    r, g, b = px[..., 0].astype(np.uint16), px[..., 1].astype(np.uint16), px[..., 2].astype(np.uint16)
    return ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)


def encode(name, x, y, px):
    h, w = px.shape[:2]
    a = np.clip(px[..., 3] + 0.5, 0, 255).astype(np.uint8)
    col = rgb565(np.clip(px[..., :3] + 0.5, 0, 255).astype(np.uint8))
    body = bytearray()
    for c, al in zip(col.ravel(), a.ravel()):
        body += struct.pack("<HB", int(c), int(al))
    n = name.encode()
    return bytes([len(n)]) + n + struct.pack("<hhHH", x, y, w, h) + bytes(body)


def main():
    ref, vdir, out = sys.argv[1], Path(sys.argv[2]), sys.argv[3]
    preview = Path(sys.argv[4]) if len(sys.argv) > 4 else None
    base = cutout(ref)
    images = [("base", 0, 0, base), ("face", *patch(base, FACE, 1))]
    for name in EXPRESSIONS:
        path = vdir / f"{name}.png"
        if not path.exists():
            print(f"{name}: missing, skipped", file=sys.stderr)
            continue
        err, dx, dy, v = align(base, fit(cutout(path), base.shape[1]))
        print(f"{name}: aligned by ({dx},{dy}), ring error {err:.1f}", file=sys.stderr)
        images.append((f"face.{name}", *patch(v, FACE, 5)))
        if name == "blink":
            images.append(("eyes.blink", *patch(v, EYES, 3)))
        if name == "talk":
            images.append(("mouth.talk", *patch(v, MOUTH, 3)))
        if preview:
            preview.mkdir(parents=True, exist_ok=True)
            show = base.copy()
            x, y, px = patch(v, FACE, 5)
            a = px[..., 3:4] / 255
            region = show[y : y + px.shape[0], x : x + px.shape[1]]
            region[..., :3] = region[..., :3] * (1 - a) + px[..., :3] * a
            Image.fromarray(show.astype(np.uint8), "RGBA").save(preview / f"{name}.png")
    blob = b"GUGA" + struct.pack("<H", len(images)) + b"".join(encode(*i) for i in images)
    Path(out).write_bytes(blob)
    print(f"{len(images)} images, {len(blob)} bytes -> {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
