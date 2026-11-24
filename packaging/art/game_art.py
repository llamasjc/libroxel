#!/usr/bin/env python3
"""Draws the pictures the table games use: chess and draughts pieces,
Connect 4 discs and board tiles, and a deck of cards with its back and a
table cloth. Everything is drawn here from simple shapes, so it is
Libroxel's own work, under the same MIT or Apache 2.0 license as the rest.

    python3 packaging/art/game_art.py

writes into crates/libroxel-engine/assets/. Needs Pillow.
"""

import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "crates/libroxel-engine/assets"
FONT = ROOT / "crates/libroxel/assets/fonts/AtkinsonHyperlegible-Bold.ttf"
S = 4  # drawn this many times larger, then shrunk, for smooth edges


def canvas(w, h):
    return Image.new("RGBA", (w * S, h * S), (0, 0, 0, 0))


def finish(img, w, h):
    return img.resize((w, h), Image.LANCZOS)


def sc(points):
    return [(x * S, y * S) for x, y in points]


# --- Chess -----------------------------------------------------------------

LIGHT = ((250, 246, 236), (40, 34, 30))  # fill, outline
DARK = ((48, 44, 52), (238, 232, 220))


def chess_shapes(kind):
    """The piece as polygons and ellipses on a 128 x 128 grid."""
    base = [("rect", (26, 104, 102, 116), 6), ("rect", (34, 96, 94, 106), 4)]
    if kind == "p":
        return base + [
            ("poly", [(44, 96), (50, 70), (78, 70), (84, 96)]),
            ("rect", (42, 64, 86, 72), 4),
            ("ellipse", (46, 30, 82, 66)),
        ]
    if kind == "r":
        return base + [
            ("poly", [(40, 96), (44, 50), (84, 50), (88, 96)]),
            ("rect", (36, 40, 92, 52), 3),
            ("rect", (36, 22, 50, 42), 2),
            ("rect", (57, 22, 71, 42), 2),
            ("rect", (78, 22, 92, 42), 2),
        ]
    if kind == "n":
        return base + [
            ("poly", [
                (40, 96), (44, 74), (54, 62), (40, 64), (30, 58), (30, 48), (46, 34),
                (52, 22), (58, 30), (70, 24), (84, 34), (92, 54), (94, 78), (90, 96),
            ]),
            ("eye", (60, 38, 6)),
            ("mane", [(72, 30), (84, 44), (88, 62), (88, 84)]),
        ]
    if kind == "b":
        return base + [
            ("poly", [(44, 96), (52, 76), (76, 76), (84, 96)]),
            ("rect", (44, 70, 84, 78), 4),
            ("ellipse", (44, 28, 84, 74)),
            ("poly", [(54, 34), (64, 14), (74, 34)]),
            ("ellipse", (59, 8, 69, 18)),
            ("slit", [(68, 38), (58, 54)]),
        ]
    if kind == "q":
        return base + [
            ("poly", [(42, 96), (34, 40), (48, 66), (54, 32), (64, 62), (74, 32), (80, 66), (94, 40), (86, 96)]),
            ("ellipse", (28, 30, 40, 42)),
            ("ellipse", (48, 22, 60, 34)),
            ("ellipse", (68, 22, 80, 34)),
            ("ellipse", (88, 30, 100, 42)),
            ("rect", (40, 80, 88, 88), 3),
        ]
    if kind == "k":
        return base + [
            ("poly", [(42, 96), (36, 52), (92, 52), (86, 96)]),
            ("ellipse", (34, 38, 64, 62)),
            ("ellipse", (64, 38, 94, 62)),
            ("rect", (40, 80, 88, 88), 3),
            ("rect", (60, 10, 68, 40), 1),
            ("rect", (52, 18, 76, 26), 1),
        ]
    raise ValueError(kind)


def draw_shape(d, shape, fill, line, width):
    kind = shape[0]
    if kind == "rect":
        (x0, y0, x1, y1), r = shape[1], shape[2]
        d.rounded_rectangle((x0 * S, y0 * S, x1 * S, y1 * S), r * S, fill=fill, outline=line, width=width)
    elif kind == "ellipse":
        x0, y0, x1, y1 = shape[1]
        d.ellipse((x0 * S, y0 * S, x1 * S, y1 * S), fill=fill, outline=line, width=width)
    elif kind == "poly":
        d.polygon(sc(shape[1]), fill=fill, outline=line, width=width)


def chess(colour, kind):
    fill, line = LIGHT if colour == "w" else DARK
    img = canvas(128, 128)
    d = ImageDraw.Draw(img)
    shapes = chess_shapes(kind)
    # Outline first (every shape a little bigger), then the fill over it.
    for shape in shapes:
        if shape[0] in ("rect", "ellipse", "poly"):
            draw_shape(d, shape, line, line, 7 * S)
    for shape in shapes:
        if shape[0] in ("rect", "ellipse", "poly"):
            draw_shape(d, shape, fill, None, 0)
    for shape in shapes:
        if shape[0] == "eye":
            x, y, r = shape[1]
            d.ellipse(((x - r / 2) * S, (y - r / 2) * S, (x + r / 2) * S, (y + r / 2) * S), fill=line)
        elif shape[0] == "mane":
            d.line(sc(shape[1]), fill=line, width=3 * S, joint="curve")
        elif shape[0] == "slit":
            d.line(sc(shape[1]), fill=line, width=4 * S)
    return finish(img, 128, 128)


# --- Wooden sets ------------------------------------------------------------

MAPLE = ((232, 196, 140), (196, 150, 92), (92, 58, 28))  # light, dark grain, outline
WALNUT = ((112, 70, 42), (70, 40, 22), (34, 18, 8))


def wood_texture(w, h, wood, seed):
    """Wood grain: wavy rings across the piece, with a little noise."""
    light, dark, _ = wood
    rng = random.Random(seed)
    img = Image.new("RGBA", (w, h))
    px = img.load()
    wobble = [rng.uniform(0, math.tau) for _ in range(3)]
    for y in range(h):
        for x in range(w):
            u = x / w
            v = y / h
            ring = math.sin((u * 9 + 0.6 * math.sin(v * 5 + wobble[0]) + 0.25 * math.sin(v * 13 + wobble[1])) * math.pi)
            grain = 0.5 + 0.5 * ring
            grain = grain ** 3 + rng.uniform(-0.05, 0.05)
            k = max(0.0, min(1.0, grain))
            px[x, y] = tuple(int(light[i] * (1 - k) + dark[i] * k) for i in range(3)) + (255,)
    return img


def carved(mask, wood, seed):
    """Wood seen through `mask`, with a dark outline and a soft highlight."""
    w, h = mask.size
    _, _, line = wood
    outline = mask.filter(ImageFilter.MaxFilter(5 * S + 1))
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    img.paste(Image.new("RGBA", (w, h), line + (255,)), (0, 0), outline)
    texture = wood_texture(w // S, h // S, wood, seed).resize((w, h))
    img.paste(texture, (0, 0), mask)
    shine = Image.new("L", (w, h), 0)
    ImageDraw.Draw(shine).ellipse((w * 0.2, h * 0.05, w * 0.55, h * 0.5), fill=70)
    shine = Image.composite(shine.filter(ImageFilter.GaussianBlur(10 * S)), Image.new("L", (w, h), 0), mask)
    img.paste(Image.new("RGBA", (w, h), (255, 255, 255, 255)), (0, 0), shine)
    return img


def chess_wood(colour, kind):
    mask = Image.new("L", (128 * S, 128 * S), 0)
    d = ImageDraw.Draw(mask)
    shapes = chess_shapes(kind)
    for shape in shapes:
        if shape[0] in ("rect", "ellipse", "poly"):
            draw_shape(d, shape, 255, None, 0)
    wood = MAPLE if colour == "w" else WALNUT
    img = carved(mask, wood, seed="wb".index(colour) * 10 + "kqrbnp".index(kind))
    line = ImageDraw.Draw(img)
    _, _, ink = wood
    for shape in shapes:
        if shape[0] == "eye":
            x, y, r = shape[1]
            line.ellipse(((x - r / 2) * S, (y - r / 2) * S, (x + r / 2) * S, (y + r / 2) * S), fill=ink)
        elif shape[0] == "mane":
            line.line(sc(shape[1]), fill=ink, width=3 * S, joint="curve")
        elif shape[0] == "slit":
            line.line(sc(shape[1]), fill=ink, width=4 * S)
    # A carved line round the base.
    line.line(sc([(36, 104), (92, 104)]), fill=ink, width=2 * S)
    return finish(img, 128, 128)


def draughts_wood(colour, king):
    wood = MAPLE if colour == "white" else WALNUT
    light, dark, line = wood
    piece = disc(light + (255,), dark + (255,), line + (255,), stack=2 if king else 1)
    # Grain on the top face.
    top = 28 if king else 40
    mask = Image.new("L", (128, 128), 0)
    ImageDraw.Draw(mask).ellipse((14, top + 2, 114, top + 42), fill=255)
    texture = wood_texture(128, 128, wood, seed=7 if king else 3)
    piece.paste(texture, (0, 0), mask)
    d = ImageDraw.Draw(piece)
    d.ellipse((26, top + 7, 102, top + 37), outline=line, width=2)
    if king:
        cx, cy = 64, top + 22
        pts = [(cx - 22, cy + 10), (cx - 24, cy - 10), (cx - 12, cy), (cx, cy - 14), (cx + 12, cy), (cx + 24, cy - 10), (cx + 22, cy + 10)]
        d.polygon(pts, fill=(220, 170, 40), outline=line)
    return piece


# --- Draughts and Connect 4 ------------------------------------------------


def disc(face, rim, edge, stack=1, mark=None, mark_colour=None):
    """A round piece seen a little from above: a top and a side for each layer."""
    img = canvas(128, 128)
    d = ImageDraw.Draw(img)
    h = 16 if stack == 1 else 14
    top = 40 if stack == 1 else 28
    for layer in range(stack - 1, -1, -1):
        y = top + layer * h
        d.rounded_rectangle((12 * S, (y + 22) * S, 116 * S, (y + 22 + h) * S), 6 * S, fill=rim, outline=edge, width=2 * S)
        d.ellipse((12 * S, (y + h + 2) * S, 116 * S, (y + 44 + h) * S), fill=rim, outline=edge, width=2 * S)
        d.ellipse((12 * S, y * S, 116 * S, (y + 44) * S), fill=face, outline=edge, width=2 * S)
        d.ellipse((26 * S, (y + 7) * S, 102 * S, (y + 37) * S), outline=edge, width=2 * S)
    if mark == "crown":
        cx, cy = 64, top + 22
        pts = [(cx - 22, cy + 10), (cx - 24, cy - 10), (cx - 12, cy), (cx, cy - 14), (cx + 12, cy), (cx + 24, cy - 10), (cx + 22, cy + 10)]
        d.polygon(sc(pts), fill=mark_colour, outline=edge, width=2 * S)
    return finish(img, 128, 128)


def counter(colour, shine, ring):
    """A Connect 4 counter, face on: a domed disc with a ring and a dot."""
    img = canvas(128, 128)
    d = ImageDraw.Draw(img)
    d.ellipse((4 * S, 4 * S, 124 * S, 124 * S), fill=ring)
    d.ellipse((10 * S, 10 * S, 118 * S, 118 * S), fill=colour)
    d.ellipse((30 * S, 30 * S, 98 * S, 98 * S), outline=ring, width=5 * S)
    d.ellipse((56 * S, 56 * S, 72 * S, 72 * S), fill=ring)
    glow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse((26 * S, 18 * S, 70 * S, 46 * S), fill=shine)
    img = Image.alpha_composite(img, glow.filter(ImageFilter.GaussianBlur(6 * S)))
    return finish(img, 128, 128)


def board_tile(top):
    """A square of the blue Connect 4 board; the top row has a lighter edge."""
    img = Image.new("RGBA", (128, 128))
    for y in range(128):
        for x in range(128):
            r = math.hypot(x - 64, y - 64) / 90
            shade = int(60 - 22 * r)
            img.putpixel((x, y), (shade // 3, shade // 2 + 10, shade + 40, 255))
    if top:
        ImageDraw.Draw(img).rectangle((0, 0, 127, 10), fill=(70, 110, 190, 255))
    return img


# --- Cards -----------------------------------------------------------------

CW, CH = 162, 252
RED, BLACK = (200, 30, 40), (24, 24, 28)


def suit_shape(d, suit, cx, cy, size, colour):
    """A suit sign of height `size` centred on (cx, cy), in canvas pixels."""
    s = size / 2
    if suit == "d":
        d.polygon([(cx, cy - s), (cx + s * 0.72, cy), (cx, cy + s), (cx - s * 0.72, cy)], fill=colour)
    elif suit == "h":
        r = s * 0.52
        d.ellipse((cx - 2 * r, cy - s, cx, cy - s + 2 * r), fill=colour)
        d.ellipse((cx, cy - s, cx + 2 * r, cy - s + 2 * r), fill=colour)
        d.polygon([(cx - 2 * r + r * 0.12, cy - s + r * 1.35), (cx + 2 * r - r * 0.12, cy - s + r * 1.35), (cx, cy + s)], fill=colour)
    elif suit == "s":
        r = s * 0.5
        d.polygon([(cx, cy - s), (cx + 2 * r - r * 0.1, cy + s * 0.1), (cx - 2 * r + r * 0.1, cy + s * 0.1)], fill=colour)
        d.ellipse((cx - 2 * r, cy - s * 0.25, cx, cy - s * 0.25 + 2 * r), fill=colour)
        d.ellipse((cx, cy - s * 0.25, cx + 2 * r, cy - s * 0.25 + 2 * r), fill=colour)
        d.polygon([(cx, cy + s * 0.2), (cx + s * 0.35, cy + s), (cx - s * 0.35, cy + s)], fill=colour)
    elif suit == "c":
        r = s * 0.42
        for ox, oy in ((0, -s + r), (-r * 1.05, -s + r * 2.6), (r * 1.05, -s + r * 2.6)):
            d.ellipse((cx + ox - r, cy + oy - r, cx + ox + r, cy + oy + r), fill=colour)
        d.polygon([(cx, cy - s * 0.1), (cx + s * 0.32, cy + s), (cx - s * 0.32, cy + s)], fill=colour)


# Where the pips go, as (column, row) on a 3 x 7 grid, for 2 to 10.
PIPS = {
    2: [(1, 0), (1, 6)],
    3: [(1, 0), (1, 3), (1, 6)],
    4: [(0, 0), (2, 0), (0, 6), (2, 6)],
    5: [(0, 0), (2, 0), (1, 3), (0, 6), (2, 6)],
    6: [(0, 0), (2, 0), (0, 3), (2, 3), (0, 6), (2, 6)],
    7: [(0, 0), (2, 0), (1, 1.5), (0, 3), (2, 3), (0, 6), (2, 6)],
    8: [(0, 0), (2, 0), (1, 1.5), (0, 3), (2, 3), (1, 4.5), (0, 6), (2, 6)],
    9: [(0, 0), (2, 0), (0, 2), (2, 2), (1, 3), (0, 4), (2, 4), (0, 6), (2, 6)],
    10: [(0, 0), (2, 0), (1, 1), (0, 2), (2, 2), (0, 4), (2, 4), (1, 5), (0, 6), (2, 6)],
}


def card(rank, suit):
    img = canvas(CW, CH)
    d = ImageDraw.Draw(img)
    colour = RED if suit in "hd" else BLACK
    d.rounded_rectangle((1 * S, 1 * S, (CW - 2) * S, (CH - 2) * S), 10 * S, fill=(255, 255, 255), outline=(150, 150, 150), width=2 * S)
    label = {"a": "A", "j": "J", "q": "Q", "k": "K"}.get(rank, rank)
    font = ImageFont.truetype(str(FONT), 30 * S)
    # The corner: rank over a small suit, then the same turned round.
    corner = Image.new("RGBA", (34 * S, 66 * S), (0, 0, 0, 0))
    cd = ImageDraw.Draw(corner)
    cd.text((17 * S, 2 * S), label, font=font, fill=colour, anchor="mt")
    suit_shape(cd, suit, 17 * S, 48 * S, 20 * S, colour)
    img.alpha_composite(corner, (6 * S, 6 * S))
    img.alpha_composite(corner.rotate(180), ((CW - 40) * S, (CH - 72) * S))
    if rank in PIPS or rank.isdigit():
        n = int(rank)
        for col, row in PIPS[n]:
            x = (57 + col * 24) * S
            y = (50 + row * 25.3) * S
            size = 28 * S
            if row > 3:
                pip = Image.new("RGBA", (size, size), (0, 0, 0, 0))
                suit_shape(ImageDraw.Draw(pip), suit, size / 2, size / 2, size * 0.95, colour)
                img.alpha_composite(pip.rotate(180), (int(x - size / 2), int(y - size / 2)))
            else:
                suit_shape(d, suit, x, y, size * 0.95, colour)
    elif rank == "a":
        suit_shape(d, suit, CW / 2 * S, CH / 2 * S, 80 * S, colour)
    else:
        # Court cards: a framed panel, a big letter, the suit, and a band of
        # simple shapes, mirrored top to bottom like a real court card.
        d.rounded_rectangle((34 * S, 34 * S, (CW - 34) * S, (CH - 34) * S), 6 * S, outline=colour, width=3 * S)
        half = Image.new("RGBA", ((CW - 72) * S, ((CH - 72) // 2) * S), (0, 0, 0, 0))
        hd = ImageDraw.Draw(half)
        w, h = half.size
        tint = (255, 228, 160) if rank == "k" else (200, 225, 255) if rank == "q" else (215, 245, 210)
        hd.rectangle((0, 0, w, h), fill=tint)
        big = ImageFont.truetype(str(FONT), 52 * S)
        hd.text((w / 2, h * 0.42), label, font=big, fill=colour, anchor="mm")
        suit_shape(hd, suit, w * 0.2, h * 0.2, 22 * S, colour)
        band = h * 0.82
        for i in range(6):
            x0 = i * w / 6
            hd.polygon([(x0, band), (x0 + w / 12, band - 12 * S), (x0 + w / 6, band)], fill=colour)
        hd.line((0, band, w, band), fill=colour, width=2 * S)
        img.alpha_composite(half, (36 * S, 36 * S))
        img.alpha_composite(half.rotate(180), (36 * S, (CH - 36) * S - h))
    return finish(img, CW, CH)


def card_back():
    img = canvas(CW, CH)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((1 * S, 1 * S, (CW - 2) * S, (CH - 2) * S), 10 * S, fill=(255, 255, 255), outline=(150, 150, 150), width=2 * S)
    d.rounded_rectangle((9 * S, 9 * S, (CW - 10) * S, (CH - 10) * S), 6 * S, fill=(30, 96, 170))
    # Rows of little cubes, Libroxel's building block.
    for row in range(14):
        for col in range(8):
            x = (20 + col * 17 + (8 if row % 2 else 0)) * S
            y = (20 + row * 16) * S
            if x > (CW - 26) * S or y > (CH - 24) * S:
                continue
            u = 6 * S
            d.polygon([(x, y - u), (x + u, y - u / 2), (x, y), (x - u, y - u / 2)], fill=(120, 180, 240))
            d.polygon([(x - u, y - u / 2), (x, y), (x, y + u), (x - u, y + u / 2)], fill=(60, 130, 210))
            d.polygon([(x + u, y - u / 2), (x, y), (x, y + u), (x + u, y + u / 2)], fill=(20, 70, 140))
    return finish(img, CW, CH)


def baize():
    rng = random.Random(7)
    img = Image.new("RGB", (512, 512))
    for y in range(512):
        for x in range(512):
            n = rng.randint(-10, 10)
            img.putpixel((x, y), (28 + n // 2, 100 + n, 52 + n // 2))
    return img.filter(ImageFilter.GaussianBlur(0.6))


def main():
    for colour in "wb":
        for kind in "kqrbnp":
            chess(colour, kind).save(ASSETS / f"chess-{colour}{kind}.png")
    for colour in "wb":
        for kind in "kqrbnp":
            chess_wood(colour, kind).save(ASSETS / f"chess-wood-{colour}{kind}.png")
    dr = ASSETS / "draughts"
    for colour in ("white", "black"):
        draughts_wood(colour, False).save(dr / f"draughts-wood-{colour}.png")
        draughts_wood(colour, True).save(dr / f"draughts-wood-{colour}-king.png")
    white = ((250, 246, 236), (214, 206, 190), (60, 52, 44))
    black = ((52, 48, 56), (30, 28, 34), (210, 204, 196))
    disc(*white).save(dr / "draughts-white.png")
    disc(*black).save(dr / "draughts-black.png")
    disc(*white, stack=2, mark="crown", mark_colour=(220, 170, 40)).save(dr / "draughts-white-king.png")
    disc(*black, stack=2, mark="crown", mark_colour=(220, 170, 40)).save(dr / "draughts-black-king.png")
    c4 = ASSETS / "connect4"
    counter((220, 40, 44), (255, 200, 200, 150), (150, 20, 24)).save(c4 / "c4-red.png")
    counter((40, 170, 80), (210, 255, 220, 150), (20, 110, 50)).save(c4 / "c4-green.png")
    board_tile(False).save(c4 / "c4-hole.png")
    board_tile(True).save(c4 / "c4-top.png")
    cards = ASSETS / "cards"
    for suit in "cdhs":
        for rank in ["a", "2", "3", "4", "5", "6", "7", "8", "9", "10", "j", "q", "k"]:
            card(rank, suit).save(cards / f"card-{rank}{suit}.png", optimize=True)
    card_back().save(cards / "card-back.png", optimize=True)
    baize().save(cards / "baize.jpg", quality=88)


if __name__ == "__main__":
    main()
