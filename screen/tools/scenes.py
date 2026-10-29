#!/usr/bin/env python3
"""Build screen/assets/scenes.bin, the backgrounds of 咕嘎's scenes on HOME.

Inputs are not vendored: one directory per scene, each holding morning.png,
noon.png and evening.png (soft anime backgrounds made with Codex's image tool;
noon from a text prompt, morning and evening as edits of noon), plus
`trainview/`, the landscape whose band of near trees scrolls past the train
windows.

    python3 screen/tools/scenes.py SCENES_DIR screen/assets/scenes.bin [PREVIEW_DIR]

Each background is cropped to the stage's aspect, scaled to STAGE and
ordered-dithered to RGB565. The train view becomes a strip VIEW_H px high,
followed by its own mirror image so it loops without a seam.

Layout, little endian: b"SCNS", u16 count, then per image: u8 name length,
name ("cafe.noon", "view.noon"), u16 w, u16 h, and w*h u16 RGB565 pixels.
Needs Pillow and NumPy.
"""
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

# Inside the stage frame on HOME.
STAGE = (310, 233)
SCENES = ["bedroom", "cafe", "library", "server", "train", "rooftop"]
TIMES = ["morning", "noon", "evening"]
# Where the 4:3 crop sits in each 3:2 picture: 0 = left edge, 1 = right edge.
CROP = {"bedroom": 0.5, "cafe": 0.5, "library": 0.5, "server": 0.5, "train": 0.2, "rooftop": 0.5}
# The train view: which band of the landscape (fractions of its height: the
# trees nearest the track) and the strip's height on screen. The painted
# windows keep their own sky, hills and town; only this band moves.
VIEW_BAND = (0.70, 0.98)
VIEW_H = 34

BAYER = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], np.float32) / 16 - 0.5


def rgb565(a):
    h, w = a.shape[:2]
    t = np.tile(BAYER, (h // 4 + 1, w // 4 + 1))[:h, :w]
    r = np.clip(a[..., 0] + t * 8, 0, 255).astype(np.uint16) >> 3
    g = np.clip(a[..., 1] + t * 4, 0, 255).astype(np.uint16) >> 2
    b = np.clip(a[..., 2] + t * 8, 0, 255).astype(np.uint16) >> 3
    return (r << 11) | (g << 5) | b


def background(path, crop):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    cw = round(h * STAGE[0] / STAGE[1])
    if cw <= w:
        x0 = round((w - cw) * crop)
        im = im.crop((x0, 0, x0 + cw, h))
    else:
        ch = round(w * STAGE[1] / STAGE[0])
        y0 = (h - ch) // 2
        im = im.crop((0, y0, w, y0 + ch))
    return np.asarray(im.resize(STAGE, Image.LANCZOS)).astype(np.float32)


def view(path):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    band = im.crop((0, round(h * VIEW_BAND[0]), w, round(h * VIEW_BAND[1])))
    sw = round(w * VIEW_H / band.size[1])
    a = np.asarray(band.resize((sw, VIEW_H), Image.LANCZOS)).astype(np.float32)
    return np.concatenate([a, a[:, ::-1]], axis=1)


def encode(name, a):
    h, w = a.shape[:2]
    n = name.encode()
    return bytes([len(n)]) + n + struct.pack("<HH", w, h) + rgb565(a).astype("<u2").tobytes()


def main():
    src, out = Path(sys.argv[1]), sys.argv[2]
    preview = Path(sys.argv[3]) if len(sys.argv) > 3 else None
    images = []
    for scene in SCENES:
        for t in TIMES:
            path = src / scene / f"{t}.png"
            if not path.exists():
                sys.exit(f"{path}: missing")
            images.append((f"{scene}.{t}", background(path, CROP[scene])))
    for t in TIMES:
        path = src / "trainview" / f"{t}.png"
        if not path.exists():
            sys.exit(f"{path}: missing")
        images.append((f"view.{t}", view(path)))
    if preview:
        preview.mkdir(parents=True, exist_ok=True)
        for name, a in images:
            Image.fromarray(a.astype(np.uint8)).save(preview / f"{name}.png")
    blob = b"SCNS" + struct.pack("<H", len(images)) + b"".join(encode(*i) for i in images)
    Path(out).write_bytes(blob)
    print(f"{len(images)} images, {len(blob)} bytes -> {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
