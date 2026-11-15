#!/usr/bin/env python3
"""Generate assets/AppIcon.icns: macOS squircle, Norton/Midnight Commander blue,
two panels with double-line frames, cyan cursor bar and an F-key bar.
Requires Pillow. Usage: python3 scripts/make_icon.py"""
import os, subprocess, shutil, tempfile
from PIL import Image, ImageDraw, ImageFilter

S = 4096                      # work canvas; downsampled for quality
K = S / 1024
def px(v): return int(round(v * K))

BLUE_TOP, BLUE_BOT = (36, 52, 205), (0, 0, 128)
CYAN, LGRAY, YELLOW, WHITE, BLACK = (0, 214, 214), (205, 205, 215), (255, 235, 70), (255, 255, 255), (0, 0, 0)

def squircle_mask(size, margin, n=5.0):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    c, r = size / 2, size / 2 - margin
    pts = []
    import math
    for i in range(720):
        t = 2 * math.pi * i / 720
        x = math.copysign(abs(math.cos(t)) ** (2 / n), math.cos(t))
        y = math.copysign(abs(math.sin(t)) ** (2 / n), math.sin(t))
        pts.append((c + r * x, c + r * y))
    d.polygon(pts, fill=255)
    return m

mask = squircle_mask(S, px(100))

# background gradient
grad = Image.new("RGB", (S, S))
gd = ImageDraw.Draw(grad)
for y in range(S):
    t = y / S
    gd.line([(0, y), (S, y)], fill=tuple(int(BLUE_TOP[i] + (BLUE_BOT[i] - BLUE_TOP[i]) * t) for i in range(3)))

art = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(art)

def frame(x0, y0, x1, y1, col):
    for off, w in ((0, 10), (22, 10)):          # double line
        d.rectangle([px(x0 + off), px(y0 + off), px(x1 - off), px(y1 - off)], outline=col, width=px(w))

# panels
L = (170, 200, 502, 706)
R = (522, 200, 854, 706)
for p in (L, R):
    frame(*p, LGRAY)
# header bars on panels
for p in (L, R):
    d.rectangle([px(p[0] + 40), px(p[1] + 42), px(p[2] - 40), px(p[1] + 46)], fill=LGRAY)
# rows
def rows(p, cursor=None, marked=()):
    y = p[1] + 82
    for i in range(6):
        x0, x1 = p[0] + 48, p[2] - 48
        if i == cursor:
            d.rectangle([px(p[0] + 30), px(y - 10), px(p[2] - 30), px(y + 34)], fill=CYAN)
            col = BLACK
        else:
            col = YELLOW if i in marked else WHITE
        # folder marker + name bar + size bar
        d.rectangle([px(x0), px(y + 4), px(x0 + 22), px(y + 22)], fill=col)
        wname = (150, 110, 190, 90, 130, 170)[i]
        d.rectangle([px(x0 + 40), px(y + 8), px(x0 + 40 + wname), px(y + 18)], fill=col)
        d.rectangle([px(x1 - 62), px(y + 8), px(x1), px(y + 18)], fill=col)
        y += 72
rows(L, cursor=2, marked=(4, 5))
rows(R, cursor=None, marked=(1,))

# function-key bar
labels = 5
bx0, bx1, by0, by1 = 170, 854, 750, 806
w = (bx1 - bx0) / labels
for i in range(labels):
    x0 = bx0 + i * w
    d.rectangle([px(x0), px(by0), px(x0 + 34), px(by1)], fill=BLACK)
    d.rectangle([px(x0 + 34), px(by0), px(x0 + w - 8), px(by1)], fill=CYAN)
    d.rectangle([px(x0 + 12), px(by0 + 18), px(x0 + 22), px(by1 - 18)], fill=WHITE)

icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
base = grad.convert("RGBA")
# soft top highlight
hl = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(hl).ellipse([px(-200), px(-560), px(1224), px(380)], fill=(255, 255, 255, 40))
hl = hl.filter(ImageFilter.GaussianBlur(px(40)))
base = Image.alpha_composite(base, hl)
base = Image.alpha_composite(base, art)
icon.paste(base, (0, 0), mask)

# drop shadow
sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
shm = mask.filter(ImageFilter.GaussianBlur(px(14)))
sh.paste((0, 0, 0, 110), (0, px(12)), shm)
out = Image.alpha_composite(sh, icon)

# subtle edge
edge = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(edge).bitmap((0, 0), mask, fill=(255, 255, 255, 0))
master = out.resize((1024, 1024), Image.LANCZOS)

root = os.path.join(os.path.dirname(__file__), "..", "assets")
os.makedirs(root, exist_ok=True)
master.save(os.path.join(root, "AppIcon-1024.png"))
tmp = tempfile.mkdtemp(suffix=".iconset")
for sz in (16, 32, 128, 256, 512):
    master.resize((sz, sz), Image.LANCZOS).save(f"{tmp}/icon_{sz}x{sz}.png")
    master.resize((sz * 2, sz * 2), Image.LANCZOS).save(f"{tmp}/icon_{sz}x{sz}@2x.png")
subprocess.run(["iconutil", "-c", "icns", tmp, "-o", os.path.join(root, "AppIcon.icns")], check=True)
shutil.rmtree(tmp)
print("wrote assets/AppIcon.icns")
