#!/usr/bin/env python3
"""Draws the Homebrew Menu icon of horizon.nro: the Minecraft logo, a grass block and "Rust
Edition", from the player's own prepared resources (horizon-prepare), on a sky. The textures are Mojang's, so the icon is made on the
player's computer and kept out of the repository: it lands in .local/switch/icon.jpg, which
switch/inner-build.sh uses when it is there (switch/nro/icon.jpg otherwise).

Usage: switch/tools/make-icon.py [RESOURCES]   (default .local/horizon-client/26.3)
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageEnhance, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
SIZE = 256
GRASS = (145, 189, 89)  # the plains biome's grass colour


def texture(resources: Path, name: str) -> Image.Image:
    path = resources / "assets/minecraft/textures/block" / f"{name}.png"
    if not path.is_file():
        sys.exit(f"{path} is missing: prepare the resources first (docs/switch.md)")
    img = Image.open(path).convert("RGBA")
    return img.crop((0, 0, img.width, img.width))  # first frame of an animated strip


def tint(img: Image.Image, colour: tuple[int, int, int]) -> Image.Image:
    r, g, b, a = img.split()
    grey = Image.merge("RGB", (r, g, b))
    tinted = Image.new("RGB", img.size)
    px, out = grey.load(), tinted.load()
    for y in range(img.height):
        for x in range(img.width):
            v = px[x, y]
            out[x, y] = tuple(v[i] * colour[i] // 255 for i in range(3))
    return Image.merge("RGBA", (*tinted.split(), a))


def shade(img: Image.Image, k: float) -> Image.Image:
    rgb = ImageEnhance.Brightness(img.convert("RGB")).enhance(k)
    return Image.merge("RGBA", (*rgb.split(), img.getchannel("A")))


def face(canvas: Image.Image, tex: Image.Image, o, u, v) -> None:
    """Paints `tex` on the parallelogram o + s*u + t*v (s, t in 0..1), pixels kept square."""
    w, h = tex.size
    det = u[0] * v[1] - u[1] * v[0]
    # Output (x, y) -> (s, t) = M^-1 (p - o), then texture pixels (w*s, h*t).
    a, b = v[1] / det, -v[0] / det
    c, d = -u[1] / det, u[0] / det
    coeffs = (
        w * a, w * b, -w * (a * o[0] + b * o[1]),
        h * c, h * d, -h * (c * o[0] + d * o[1]),
    )
    layer = tex.transform(canvas.size, Image.AFFINE, coeffs, resample=Image.NEAREST)
    # Pixels outside the face sample the texture's edge: cut them with the face's own outline.
    mask = Image.new("L", canvas.size, 0)
    corners = [o, (o[0] + u[0], o[1] + u[1]), (o[0] + u[0] + v[0], o[1] + u[1] + v[1]), (o[0] + v[0], o[1] + v[1])]
    ImageDraw.Draw(mask).polygon(corners, fill=255, outline=255)  # outline closes the seams
    alpha = Image.composite(layer.getchannel("A"), Image.new("L", canvas.size, 0), mask)
    canvas.paste(layer, (0, 0), alpha)


def sky() -> Image.Image:
    img = Image.new("RGB", (SIZE, SIZE))
    top, bottom = (96, 150, 245), (190, 222, 255)
    draw = ImageDraw.Draw(img)
    for y in range(SIZE):
        k = y / (SIZE - 1)
        draw.line([(0, y), (SIZE, y)], fill=tuple(round(top[i] + (bottom[i] - top[i]) * k) for i in range(3)))
    # Blocky clouds, in 8-pixel steps like the game's.
    for cx, cy, cells in ((20, 34, [(0, 0), (1, 0), (2, 0), (1, -1), (3, 0)]),
                          (176, 22, [(0, 0), (1, 0), (2, 0), (2, -1), (3, -1), (4, 0)])):
        for gx, gy in cells:
            draw.rectangle([cx + gx * 12, cy + gy * 8, cx + gx * 12 + 11, cy + gy * 8 + 7], fill=(250, 252, 255))
    return img


def logo(resources: Path, width: int) -> Image.Image:
    """The title screen's logo (gui/title/minecraft.png), cropped to its letters."""
    path = resources / "assets/minecraft/textures/gui/title/minecraft.png"
    img = Image.open(path).convert("RGBA")
    img = img.crop(img.getchannel("A").getbbox())
    return img.resize((width, round(img.height * width / img.width)), Image.LANCZOS)


def font_text(resources: Path, text: str, scale: int) -> Image.Image:
    """`text` in the game's own pixel font (font/ascii.png, 8x8 cells), as a mask."""
    sheet = Image.open(resources / "assets/minecraft/textures/font/ascii.png").convert("RGBA").getchannel("A")
    glyphs = []
    for ch in text:
        code = ord(ch)
        cell = sheet.crop(((code % 16) * 8, (code // 16) * 8, (code % 16) * 8 + 8, (code // 16) * 8 + 8))
        box = cell.getbbox()
        glyphs.append(cell.crop((0, 0, box[2], 8)) if box else Image.new("L", (3, 8), 0))
    width = sum(g.width + 1 for g in glyphs) - 1
    mask = Image.new("L", (width, 8), 0)
    x = 0
    for g in glyphs:
        mask.paste(g, (x, 0))
        x += g.width + 1
    return mask.resize((width * scale, 8 * scale), Image.NEAREST)


def subtitle(resources: Path, text: str, scale: int) -> Image.Image:
    """`text` like the logo's "Java Edition": light faces over a dark extrusion, outlined in black."""
    mask = font_text(resources, text, scale)
    depth, pad = scale + 1, 2
    w, h = mask.width + 2 * pad, mask.height + depth + 2 * pad
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    solid = Image.new("L", (w, h), 0)
    for k in range(depth + 1):
        solid.paste(255, (pad, pad + k), mask)
    # The outline: the letters and their extrusion, one step bigger each way.
    outline = solid.filter(ImageFilter.MaxFilter(2 * pad + 1))
    out.paste((20, 12, 8, 255), (0, 0), outline)
    out.paste((122, 46, 18, 255), (0, 0), solid)  # the extrusion: dark rust
    # The faces: rust orange, lighter at the top.
    face_img = Image.new("RGBA", (w, h))
    draw = ImageDraw.Draw(face_img)
    for y in range(h):
        k = max(0.0, min(1.0, (y - pad) / max(1, mask.height)))
        light, deep = (246, 160, 92), (212, 92, 36)
        draw.line([(0, y), (w, y)], fill=tuple(round(light[i] + (deep[i] - light[i]) * k) for i in range(3)) + (255,))
    face_mask = Image.new("L", (w, h), 0)
    face_mask.paste(mask, (pad, pad))
    out.paste(face_img, (0, 0), face_mask)
    return out


def main() -> None:
    resources = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / ".local/horizon-client/26.3"
    top = tint(texture(resources, "grass_block_top"), GRASS)
    side = texture(resources, "grass_block_side")
    side.alpha_composite(tint(texture(resources, "grass_block_side_overlay"), GRASS))

    canvas = sky().convert("RGBA")
    # The grass block in the middle: an isometric cube 2:1, edge e on screen.
    e = 60
    cx, cy = SIZE // 2, 64
    hw, hh = e, e // 2  # half width / half height of the top rhombus
    down = (0, e)
    # left face (front-left), right face (front-right), then the top over their upper edges
    face(canvas, shade(side, 0.82), (cx - hw, cy + hh), (hw, hh), down)
    face(canvas, shade(side, 0.62), (cx, cy + 2 * hh), (hw, -hh), down)
    # top: from the back corner, texture x along the right edge, y along the left edge
    face(canvas, top, (cx, cy), (hw, hh), (-hw, hh))

    # The logo above, "Rust Edition" below, as the title screen puts "Java Edition".
    title = logo(resources, SIZE - 16)
    canvas.alpha_composite(title, ((SIZE - title.width) // 2, 10))
    sub = subtitle(resources, "RUST EDITION", 3)
    canvas.alpha_composite(sub, ((SIZE - sub.width) // 2, SIZE - sub.height - 8))

    out = ROOT / ".local/switch/icon.jpg"
    out.parent.mkdir(parents=True, exist_ok=True)
    canvas.convert("RGB").save(out, quality=95)
    print(out)


if __name__ == "__main__":
    main()
