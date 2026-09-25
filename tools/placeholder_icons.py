#!/usr/bin/env python3
"""Draws the placeholder item icons in content/icons/.

These stand in until real icons are painted: each is a few simple shapes
(signed distance functions), filled with shading and outlined, rendered at 4x
and scaled down for smooth edges. Only the standard library is used, so it
runs anywhere:

    python3 tools/placeholder_icons.py

To add an icon, add a function returning its layers to ICONS below. Each
layer is (shape, colour), drawn back to front; shapes are functions from a
point (in icon pixels, 0 to 32) to a distance (negative inside).
"""

import math
import os
import struct
import zlib

SIZE = 32
SUPERSAMPLE = 4
OUTLINE = 1.1  # pixels
OUTLINE_COLOUR = (28, 20, 14)
FOLDER = os.path.join(os.path.dirname(__file__), "..", "content", "icons")


# Shapes ---------------------------------------------------------------------

def circle(cx, cy, r):
    return lambda x, y: math.hypot(x - cx, y - cy) - r


def ellipse(cx, cy, rx, ry):
    # Not an exact distance, but close enough for small icons.
    def shape(x, y):
        k = math.hypot((x - cx) / rx, (y - cy) / ry)
        return (k - 1.0) * min(rx, ry)
    return shape


def capsule(ax, ay, bx, by, r):
    def shape(x, y):
        dx, dy = bx - ax, by - ay
        t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
        return math.hypot(x - (ax + t * dx), y - (ay + t * dy)) - r
    return shape


def rounded_box(cx, cy, hw, hh, r):
    def shape(x, y):
        qx, qy = abs(x - cx) - hw + r, abs(y - cy) - hh + r
        return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - r
    return shape


def ring(cx, cy, r, width):
    return lambda x, y: abs(math.hypot(x - cx, y - cy) - r) - width / 2


def detail(shape):
    """A layer drawn without an outline (markings on top of a shape)."""
    shape.detail = True
    return shape


# Icons ----------------------------------------------------------------------

CRUST = (196, 128, 58)
DARK_CRUST = (150, 88, 38)


def miasma():
    return [
        (circle(16, 17, 10), (132, 72, 176)),
        (detail(circle(13, 14, 5)), (186, 134, 226)),
        (detail(capsule(16, 7, 13, 3, 1.6)), (160, 104, 204)),  # a wisp rising off it
        (detail(circle(19, 21, 2)), (98, 50, 140)),
    ]


def bread():
    return [
        (ellipse(16, 18, 13, 8.5), CRUST),
        (detail(capsule(9, 17, 12, 13, 0.9)), (236, 196, 130)),
        (detail(capsule(14, 18, 17, 13, 0.9)), (236, 196, 130)),
        (detail(capsule(19, 19, 22, 14, 0.9)), (236, 196, 130)),
    ]


def meat_pie():
    return [
        (ellipse(16, 20, 13, 7), (120, 110, 104)),  # the dish
        (ellipse(16, 17, 11.5, 7.5), CRUST),
        (detail(ring(16, 17, 8.5, 1.4)), DARK_CRUST),
        (detail(capsule(14, 16, 18, 18, 0.9)), (92, 44, 26)),  # the vent
    ]


def sweet_roll():
    return [
        (circle(16, 17, 11), CRUST),
        (detail(ring(16, 17, 7, 1.6)), DARK_CRUST),
        (detail(ring(16, 17, 3.5, 1.6)), DARK_CRUST),
        (detail(ellipse(14, 13, 6, 3.5)), (248, 238, 218)),  # a blob of icing
    ]


def old_pipe():
    wood = (122, 74, 42)
    return [
        (capsule(7, 23, 20, 14, 2.0), (92, 60, 36)),  # the stem
        (rounded_box(22, 15, 5, 7, 3), wood),  # the bowl
        (detail(ellipse(22, 9.5, 3.4, 1.4)), (40, 28, 20)),  # its opening
        (detail(capsule(6, 24, 8.5, 22.5, 1.6)), (60, 44, 34)),  # the mouthpiece
    ]


ICONS = {
    "miasma.png": miasma,
    "bread.png": bread,
    "meat_pie.png": meat_pie,
    "sweet_roll.png": sweet_roll,
    "old_pipe.png": old_pipe,
}


# Drawing --------------------------------------------------------------------

def render(layers):
    big = SIZE * SUPERSAMPLE
    pixels = [[(0, 0, 0, 0)] * big for _ in range(big)]
    for shape, colour in layers:
        outlined = not getattr(shape, "detail", False)
        for py in range(big):
            y = (py + 0.5) / SUPERSAMPLE
            row = pixels[py]
            for px in range(big):
                x = (px + 0.5) / SUPERSAMPLE
                d = shape(x, y)
                if d < 0:
                    # Lighter towards the top left, as if lit from there.
                    light = 1.0 + 0.28 * ((SIZE - x - y) / SIZE)
                    row[px] = tuple(min(255, int(c * light)) for c in colour) + (255,)
                elif outlined and d < OUTLINE:
                    row[px] = OUTLINE_COLOUR + (255,)
    # Average each 4x4 block (with premultiplied alpha).
    small = []
    for sy in range(SIZE):
        row = []
        for sx in range(SIZE):
            r = g = b = a = 0
            for py in range(sy * SUPERSAMPLE, (sy + 1) * SUPERSAMPLE):
                for px in range(sx * SUPERSAMPLE, (sx + 1) * SUPERSAMPLE):
                    pr, pg, pb, pa = pixels[py][px]
                    r, g, b, a = r + pr * pa, g + pg * pa, b + pb * pa, a + pa
            if a:
                row.append((r // a, g // a, b // a, a // SUPERSAMPLE ** 2))
            else:
                row.append((0, 0, 0, 0))
        small.append(row)
    return small


def write_png(path, pixels):
    raw = b"".join(b"\x00" + bytes(channel for pixel in row for channel in pixel) for row in pixels)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)  # 8-bit RGBA
    with open(path, "wb") as file:
        file.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def main():
    os.makedirs(FOLDER, exist_ok=True)
    for name, layers in ICONS.items():
        write_png(os.path.join(FOLDER, name), render(layers()))
        print("wrote", name)


if __name__ == "__main__":
    main()
