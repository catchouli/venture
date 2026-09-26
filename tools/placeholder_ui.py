#!/usr/bin/env python3
"""Draws the placeholder interface theme in content/ui/.

These stand in until the real interface art is painted: a classic dark
MMORPG look, with bevelled gold rims (lit from the top left), dark textured
panels, red-brown buttons and inset slots and bars. Each piece is one PNG,
drawn as a nine-slice in the game (see content/ui/theme.ron, which says how
wide each piece's border is, and docs/content-authoring.md). Only the
standard library is used, so it runs anywhere:

    python3 tools/placeholder_ui.py

The toolbar's tab icons are drawn with the shapes from placeholder_icons.py.
Frames are (nearly) opaque on purpose: the game blends in linear colour, so
a dark frame that's even a little see-through lets a lot of the bright world
show through. Only the chat's backing is meant to be see-through.

To repaint a piece, overwrite its PNG (keeping its border in theme.ron in
step with the picture, or changing the number there).
"""

import math
import os
import struct
import zlib

import placeholder_icons as icons
from placeholder_icons import capsule, circle, detail, ellipse, polygon, rounded_box

FOLDER = os.path.join(os.path.dirname(__file__), "..", "content", "ui")
SUPERSAMPLE = 4

OUTLINE = (16, 11, 7)
GOLD_LIGHT = (240, 210, 140)
GOLD = (196, 154, 84)
GOLD_DARK = (112, 80, 40)
BRONZE = (140, 104, 60)


# Helpers --------------------------------------------------------------------

def mix(a, b, t):
    t = max(0.0, min(1.0, t))
    return tuple(x + (y - x) * t for x, y in zip(a, b))


def noise(x, y):
    """Repeatable grain, from -1 to 1, for texture."""
    n = int(x) * 374761393 + int(y) * 668265263
    n = (n ^ (n >> 13)) * 1274126177
    return ((n ^ (n >> 16)) & 0xFFFF) / 32767.5 - 1.0


def round_box(x, y, w, h, r_top, r_bottom=None):
    """Signed distance to a box from (0, 0) to (w, h) with rounded corners
    (negative inside)."""
    r_bottom = r_top if r_bottom is None else r_bottom
    r = r_top if y < h / 2 else r_bottom
    qx = abs(x - w / 2) - (w / 2 - r)
    qy = abs(y - h / 2) - (h / 2 - r)
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r


def nearest_edge(x, y, w, h):
    return min((x, "left"), (w - x, "right"), (y, "top"), (h - y, "bottom"))[1]


def paint(w, h, colour_at):
    """Renders a picture: colour_at(x, y) gives (r, g, b, a) at a point (in
    pixels, from the top left), averaged over several samples per pixel."""
    rows = []
    for py in range(h):
        row = []
        for px in range(w):
            r = g = b = a = 0.0
            for sy in range(SUPERSAMPLE):
                for sx in range(SUPERSAMPLE):
                    x = px + (sx + 0.5) / SUPERSAMPLE
                    y = py + (sy + 0.5) / SUPERSAMPLE
                    cr, cg, cb, ca = colour_at(x, y)
                    r, g, b, a = r + cr * ca, g + cg * ca, b + cb * ca, a + ca
            n = SUPERSAMPLE * SUPERSAMPLE
            if a > 0:
                row.append((int(r / a), int(g / a), int(b / a), int(a / n)))
            else:
                row.append((0, 0, 0, 0))
        rows.append(row)
    return rows


def framed(w, h, radius, rim, fill, rim_light=GOLD_LIGHT, rim_dark=GOLD_DARK, inset=False, r_bottom=None,
           open_bottom=False, rivets=False):
    """A frame: a dark outline, a bevelled rim `rim` pixels wide, a thin dark
    line, then `fill(x, y)` inside. `inset` swaps the bevel (lit from below,
    as if pressed in). `open_bottom` leaves the bottom edge off (tabs)."""
    def colour_at(x, y):
        # An open bottom runs the box off the bottom of the picture.
        box_h = h + radius + 4 if open_bottom else h
        d = round_box(x, y, w, box_h, radius, r_bottom)
        if d > 0:
            return (0, 0, 0, 0)
        depth = -d
        if depth < 1:
            return OUTLINE + (255,)
        if depth < 1 + rim:
            edge = nearest_edge(x, y, w, box_h)
            lit = edge in ("top", "left")
            if inset:
                lit = not lit
            base = rim_light if lit else rim_dark
            # A soft ridge across the rim.
            across = (depth - 1) / rim
            ridge = 1.0 - abs(across - 0.4) * 0.6
            return tuple(min(255, c * ridge) for c in base) + (255,)
        if depth < 2 + rim:
            return OUTLINE + (230,)
        if rivets:
            for cx, cy in ((7.5, 7.5), (w - 7.5, 7.5), (7.5, h - 7.5), (w - 7.5, h - 7.5)):
                dist = math.hypot(x - cx, y - cy)
                if dist < 2.6:
                    light = 1.0 - (x - cx + y - cy) / 6
                    return tuple(min(255, c * light) for c in GOLD) + (255,)
                if dist < 3.4:
                    return OUTLINE + (255,)
        return fill(x, y)
    return colour_at


def gradient_fill(top, bottom, h, alpha, grain=5):
    def fill(x, y):
        colour = mix(top, bottom, y / h)
        g = noise(x, y) * grain
        return tuple(max(0, min(255, c + g)) for c in colour) + (alpha,)
    return fill


# Pieces ---------------------------------------------------------------------

WINDOW_TOP, WINDOW_BOTTOM = (40, 33, 28), (20, 16, 14)


def window():
    w = h = 64
    return w, h, framed(w, h, 5, 3, gradient_fill(WINDOW_TOP, WINDOW_BOTTOM, h, 252), rivets=True)


def header():
    w, h = 64, 24
    def colour_at(x, y):
        if y >= h - 2:
            return (GOLD if y < h - 1 else GOLD_DARK) + (255,)
        colour = mix((52, 42, 32), (30, 24, 19), y / h)
        return tuple(c + noise(x, y) * 4 for c in colour) + (200,)
    return w, h, colour_at


def panel():
    w = h = 32
    return w, h, framed(w, h, 4, 2, gradient_fill(WINDOW_TOP, WINDOW_BOTTOM, h, 250))


def chat():
    w = h = 32
    return w, h, framed(w, h, 4, 1, gradient_fill((8, 7, 6), (4, 3, 3), h, 105, 2), rim_light=(120, 96, 60),
                        rim_dark=(70, 52, 30))


def tooltip():
    w = h = 32
    return w, h, framed(w, h, 3, 1, gradient_fill((24, 20, 18), (12, 10, 9), h, 252, 3), rim_light=GOLD,
                        rim_dark=BRONZE)


def button(state):
    w, h = 48, 24
    top, bottom = {
        "normal": ((122, 52, 36), (72, 28, 20)),
        "hover": ((156, 72, 46), (96, 40, 27)),
        "pressed": ((64, 25, 18), (100, 42, 29)),
        "chosen": ((176, 118, 52), (112, 66, 28)),
    }[state]
    rim_light = GOLD_LIGHT if state in ("hover", "chosen") else GOLD
    return w, h, framed(w, h, 4, 2, gradient_fill(top, bottom, h, 255, 4), rim_light=rim_light,
                        inset=state == "pressed")


def slot(hover):
    w = h = 40

    def fill(x, y):
        # Darker towards the top left, as if sunk in.
        shade = min(x, y) / 8
        colour = mix((6, 5, 4), (26, 22, 19), shade)
        if hover:
            glow = max(0.0, 1.0 - round_box(x, y, w, h, 4) / -10)
            colour = mix(colour, (90, 70, 36), 0.35 * glow)
        return tuple(c + noise(x, y) * 3 for c in colour) + (250,)

    rim = GOLD_LIGHT if hover else BRONZE
    return w, h, framed(w, h, 4, 1, fill, rim_light=rim, rim_dark=GOLD_DARK if not hover else GOLD, inset=True)


def tab(state):
    w, h = 40, 24
    top, bottom, rim = {
        "normal": ((34, 28, 24), (22, 18, 15), BRONZE),
        "hover": ((50, 41, 33), (30, 25, 20), GOLD),
        "open": ((58, 48, 38), WINDOW_TOP, GOLD_LIGHT),
    }[state]
    return w, h, framed(w, h, 5, 2, gradient_fill(top, bottom, h, 252, 4), rim_light=rim, rim_dark=GOLD_DARK,
                        open_bottom=True)


def bar_back():
    w, h = 24, 12

    def fill(x, y):
        colour = mix((4, 3, 3), (20, 17, 15), y / h)
        return colour + (235,)

    return w, h, framed(w, h, 3, 1, fill, rim_light=GOLD_DARK, rim_dark=BRONZE, inset=True)


def bar_fill():
    w, h = 24, 12

    def colour_at(x, y):
        d = round_box(x, y, w, h, 2.5)
        if d > 0:
            return (0, 0, 0, 0)
        t = y / h
        level = 255 - 110 * t
        if 1.5 < y < 3.5:
            level = 255  # the shine
        return (level, level, level, 255)

    return w, h, colour_at


def close(hover):
    w = h = 18
    red_top, red_bottom = ((196, 66, 46), (110, 30, 22)) if hover else ((150, 48, 34), (84, 22, 16))

    def colour_at(x, y):
        d = math.hypot(x - 9, y - 9) - 8.5
        if d > 0:
            return (0, 0, 0, 0)
        if d > -1:
            return OUTLINE + (255,)
        if d > -2.5:
            return (GOLD_LIGHT if y < 9 else GOLD_DARK) + (255,)
        # The cross.
        u, v = (x - 9 + y - 9) / math.sqrt(2), (x - 9 - (y - 9)) / math.sqrt(2)
        if (abs(u) < 1.1 and abs(v) < 4) or (abs(v) < 1.1 and abs(u) < 4):
            return (245, 230, 200, 255)
        return mix(red_top, red_bottom, y / h) + (255,)

    return w, h, colour_at


def divider():
    w, h = 64, 6

    def colour_at(x, y):
        fade = min(1.0, min(x, w - x) / 20)
        if 2 <= y < 3:
            return GOLD + (int(255 * fade),)
        if 3 <= y < 4:
            return GOLD_DARK + (int(200 * fade),)
        return (0, 0, 0, 0)

    return w, h, colour_at


def text_field():
    w, h = 32, 24

    def fill(x, y):
        return mix((2, 2, 2), (14, 12, 11), min(1.0, y / 8)) + (200,)

    return w, h, framed(w, h, 3, 1, fill, rim_light=GOLD_DARK, rim_dark=BRONZE, inset=True)


def toolbar_button(hover):
    w = h = 40

    def fill(x, y):
        centre = math.hypot(x - w / 2, y - h / 2) / (w / 2)
        colour = mix((58, 46, 34) if hover else (44, 36, 28), (16, 13, 11), centre)
        return tuple(c + noise(x, y) * 3 for c in colour) + (252,)

    return w, h, framed(w, h, 6, 3, fill, rim_light=GOLD_LIGHT if hover else GOLD)


PIECES = {
    "window.png": window,
    "header.png": header,
    "panel.png": panel,
    "chat.png": chat,
    "tooltip.png": tooltip,
    "button.png": lambda: button("normal"),
    "button_hover.png": lambda: button("hover"),
    "button_pressed.png": lambda: button("pressed"),
    "button_chosen.png": lambda: button("chosen"),
    "slot.png": lambda: slot(False),
    "slot_hover.png": lambda: slot(True),
    "tab.png": lambda: tab("normal"),
    "tab_hover.png": lambda: tab("hover"),
    "tab_open.png": lambda: tab("open"),
    "bar_back.png": bar_back,
    "bar_fill.png": bar_fill,
    "close.png": lambda: close(False),
    "close_hover.png": lambda: close(True),
    "divider.png": divider,
    "text_field.png": text_field,
    "toolbar_button.png": lambda: toolbar_button(False),
    "toolbar_button_hover.png": lambda: toolbar_button(True),
}


# Tab icons ------------------------------------------------------------------

def tab_stats():
    skin, tunic = (226, 186, 146), (150, 60, 50)
    return [
        (ellipse(16, 27, 10, 7), tunic),  # shoulders
        (circle(16, 13, 6.5), skin),  # the head
        (detail(capsule(12, 24, 20, 24, 1.0)), (200, 160, 80)),  # a collar
    ]


def tab_skills():
    return icons.skill_mining()[1:] + [(detail(circle(24, 24, 1.2)), (255, 200, 100))]


def tab_abilities():
    flame = (250, 170, 60)
    return [
        (polygon([(16, 4), (24, 16), (22, 25), (16, 28), (10, 25), (8, 16)]), flame),
        (detail(polygon([(16, 12), (20, 20), (16, 25), (12, 20)])), (255, 236, 160)),
    ]


def tab_bag():
    sack = (170, 124, 76)
    return [
        (ellipse(16, 20, 10, 9), sack),
        (polygon([(11, 12), (21, 12), (19, 7), (13, 7)]), sack),
        (detail(capsule(11, 12, 21, 12, 1.2)), (92, 58, 34)),  # the tie
    ]


def tab_gear():
    return [
        (polygon([(8, 20), (8, 12), (12, 6), (20, 6), (24, 12), (24, 20), (20, 26), (12, 26)]), icons.STEEL),
        (detail(capsule(11, 15, 21, 15, 1.2)), (40, 40, 48)),  # the visor
        (detail(capsule(16, 16, 16, 24, 1.0)), (40, 40, 48)),
    ]


def tab_quests():
    paper = (234, 218, 178)
    return [
        (rounded_box(16, 16, 9, 11, 1.5), paper),
        (capsule(7, 5.5, 25, 5.5, 2.2), (200, 180, 140)),  # rolled ends
        (capsule(7, 26.5, 25, 26.5, 2.2), (200, 180, 140)),
        (detail(capsule(11, 12, 21, 12, 0.6)), (120, 100, 80)),  # writing
        (detail(capsule(11, 16, 21, 16, 0.6)), (120, 100, 80)),
        (detail(capsule(11, 20, 18, 20, 0.6)), (120, 100, 80)),
    ]


def tab_emotes():
    return icons.emote_wave()[1:]


TAB_ICONS = {
    "tab_stats.png": tab_stats,
    "tab_skills.png": tab_skills,
    "tab_abilities.png": tab_abilities,
    "tab_bag.png": tab_bag,
    "tab_gear.png": tab_gear,
    "tab_quests.png": tab_quests,
    "tab_emotes.png": tab_emotes,
}


# Files ----------------------------------------------------------------------

def write_png(path, rows):
    h, w = len(rows), len(rows[0])
    raw = b"".join(b"\x00" + bytes(int(max(0, min(255, c))) for pixel in row for c in pixel) for row in rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header_bytes = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)  # 8-bit RGBA
    with open(path, "wb") as file:
        file.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header_bytes) + chunk(b"IDAT", zlib.compress(raw, 9))
                   + chunk(b"IEND", b""))


def main():
    os.makedirs(FOLDER, exist_ok=True)
    for name, make in PIECES.items():
        w, h, colour_at = make()
        write_png(os.path.join(FOLDER, name), paint(w, h, colour_at))
        print("wrote", name)
    for name, layers in TAB_ICONS.items():
        write_png(os.path.join(FOLDER, name), icons.render(layers()))
        print("wrote", name)


if __name__ == "__main__":
    main()
