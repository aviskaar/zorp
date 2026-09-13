#!/usr/bin/env python3
"""Generates macOS and desktop application icons for Zorp."""

import os
import shutil
import subprocess
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

ROOT_DIR = Path(__file__).resolve().parent.parent
ICONS_DIR = ROOT_DIR / "zorp-desktop" / "icons"
ICONS_DIR.mkdir(parents=True, exist_ok=True)

# Generate master 2048x2048 canvas for supersampled crispness
size = 2048
margin = 200
r = 370
rect = [margin, margin, size - margin, size - margin]

# Background: dark sleek slate/zinc gradient matching Zorp chrome
bg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
bg_draw = ImageDraw.Draw(bg)
for y in range(margin, size - margin):
    progress = (y - margin) / (size - 2 * margin)
    r_val = int(28 * (1 - progress) + 14 * progress)
    g_val = int(30 * (1 - progress) + 15 * progress)
    b_val = int(36 * (1 - progress) + 18 * progress)
    bg_draw.line([(margin, y), (size - margin, y)], fill=(r_val, g_val, b_val, 255))

mask = Image.new("L", (size, size), 0)
mask_draw = ImageDraw.Draw(mask)
mask_draw.rounded_rectangle(rect, radius=r, fill=255)

squircle = Image.new("RGBA", (size, size), (0, 0, 0, 0))
squircle.paste(bg, (0, 0), mask)

# Inner border highlight
border_mask = Image.new("L", (size, size), 0)
b_draw = ImageDraw.Draw(border_mask)
b_draw.rounded_rectangle(rect, radius=r, outline=255, width=4)

border_layer = Image.new("RGBA", (size, size), (255, 255, 255, 30))
squircle.paste(border_layer, (0, 0), border_mask)

# Drop shadow under squircle
shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
s_draw = ImageDraw.Draw(shadow)
shadow_rect = [margin, margin + 20, size - margin, size - margin + 20]
s_draw.rounded_rectangle(shadow_rect, radius=r, fill=(0, 0, 0, 100))
shadow = shadow.filter(ImageFilter.GaussianBlur(radius=30))

final_img = Image.alpha_composite(shadow, squircle)

# Draw Zorp node graph mark (viewBox: -30 -30 400 424)
mark_layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
m_draw = ImageDraw.Draw(mark_layer)

vb_x0, vb_y0 = -30.0, -30.0
vb_w, vb_h = 400.0, 424.0

scale = 2.65
offset_x = (size - vb_w * scale) / 2.0 - vb_x0 * scale
offset_y = (size - vb_h * scale) / 2.0 - vb_y0 * scale

def tr(x, y):
    return (x * scale + offset_x, y * scale + offset_y)

lines = [
    ((15, 17), (325, 17)),
    ((325, 17), (15, 335)),
    ((15, 335), (325, 335)),
]

line_width = int(14 * scale)
for p1, p2 in lines:
    m_draw.line([tr(*p1), tr(*p2)], fill=(255, 255, 255, 255), width=line_width)

nodes = [
    (15, 17, 23, False),
    (167, 17, 20, True),
    (325, 17, 23, False),
    (234, 123, 22, False),
    (122, 225, 20, True),
    (15, 335, 23, False),
    (167, 335, 20, False),
    (325, 335, 23, False),
]

for x, y, rad, is_accent in nodes:
    cx, cy = tr(x, y)
    r_scaled = rad * scale
    color = (94, 234, 212, 255) if is_accent else (255, 255, 255, 255)
    m_draw.ellipse([cx - r_scaled, cy - r_scaled, cx + r_scaled, cy + r_scaled], fill=color)

final_img = Image.alpha_composite(final_img, mark_layer)

# Generate individual icons
icon_1024 = final_img.resize((1024, 1024), Image.Resampling.LANCZOS)
icon_1024.save(ICONS_DIR / "icon-1024.png")

icon_512 = final_img.resize((512, 512), Image.Resampling.LANCZOS)
icon_512.save(ICONS_DIR / "icon.png")

icon_256 = final_img.resize((256, 256), Image.Resampling.LANCZOS)
icon_256.save(ICONS_DIR / "128x128@2x.png")

icon_128 = final_img.resize((128, 128), Image.Resampling.LANCZOS)
icon_128.save(ICONS_DIR / "128x128.png")

icon_32 = final_img.resize((32, 32), Image.Resampling.LANCZOS)
icon_32.save(ICONS_DIR / "32x32.png")
icon_32.save(ICONS_DIR / "icon.ico")

# Generate .icns using iconutil if available
iconset_dir = ICONS_DIR / "zorp.iconset"
if iconset_dir.exists():
    shutil.rmtree(iconset_dir)
iconset_dir.mkdir(parents=True)

resolutions = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]

for name, res in resolutions:
    img = final_img.resize((res, res), Image.Resampling.LANCZOS)
    img.save(iconset_dir / name)

if shutil.which("iconutil"):
    icns_path = ICONS_DIR / "icon.icns"
    subprocess.run(["iconutil", "-c", "icns", str(iconset_dir), "-o", str(icns_path)], check=True)
    shutil.rmtree(iconset_dir)
    print("Generated icon.icns successfully!")
