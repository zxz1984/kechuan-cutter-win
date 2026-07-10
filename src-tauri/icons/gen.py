#!/usr/bin/env python3
"""生成可乐裁切应用图标：可乐蓝渐变背景 + "可 乐 / 裁 切" 2x2 排列"""
from PIL import Image, ImageDraw, ImageFont
import os

SIZE = 1024
img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))

# 渐变背景：可乐蓝（区别于混剪的红色）
for y in range(SIZE):
    t = y / SIZE
    r = int(20 + (60 - 20) * t)    # 20 -> 60
    g = int(80 + (140 - 80) * t)   # 80 -> 140
    b = int(150 + (220 - 150) * t) # 150 -> 220
    ImageDraw.Draw(img).line([(0, y), (SIZE, y)], fill=(r, g, b, 255))

# 圆角
mask = Image.new("L", (SIZE, SIZE), 0)
ImageDraw.Draw(mask).rounded_rectangle([0, 0, SIZE - 1, SIZE - 1], radius=180, fill=255)
bg = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
bg.paste(img, (0, 0), mask)
img = bg

# 字体
font_size = 380
font = None
candidates = [
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/Library/Fonts/Songti.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
]
for path in candidates:
    if os.path.exists(path):
        try:
            font = ImageFont.truetype(path, font_size)
            break
        except Exception:
            continue
if font is None:
    font = ImageFont.load_default()

draw = ImageDraw.Draw(img)

# 白色字 + 浅蓝阴影（深色字在蓝底上）
text_color = (255, 255, 255, 255)
shadow_color = (20, 40, 80, 200)

# 2x2：可 乐 / 裁 切（区别于混剪的可 乐 / 混 剪）
chars = ["可", "乐", "裁", "切"]
offset = 180
positions = [
    (SIZE // 2 - offset, SIZE // 2 - offset),  # 左上
    (SIZE // 2 + offset, SIZE // 2 - offset),  # 右上
    (SIZE // 2 - offset, SIZE // 2 + offset),  # 左下
    (SIZE // 2 + offset, SIZE // 2 + offset),  # 右下
]

for char, (cx, cy) in zip(chars, positions):
    bbox = draw.textbbox((0, 0), char, font=font)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    x = cx - tw // 2 - bbox[0]
    y = cy - th // 2 - bbox[1]
    draw.text((x + 8, y + 8), char, font=font, fill=shadow_color)
    draw.text((x, y), char, font=font, fill=text_color)

# 保存
out_dir = os.path.dirname(__file__)
img.save(os.path.join(out_dir, "icon.png"))
img.save(os.path.join(out_dir, "128x128.png"))
img.resize((256, 256), Image.LANCZOS).save(os.path.join(out_dir, "128x128@2x.png"))
img.resize((32, 32), Image.LANCZOS).save(os.path.join(out_dir, "32x32.png"))
print("可乐裁切图标已生成（蓝色背景）")
