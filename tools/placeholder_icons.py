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


def polygon(points):
    """A filled polygon (points in order around it)."""
    def shape(x, y):
        inside = False
        nearest = float("inf")
        for i, (ax, ay) in enumerate(points):
            bx, by = points[(i + 1) % len(points)]
            if (ay > y) != (by > y) and x < ax + (y - ay) * (bx - ax) / (by - ay):
                inside = not inside
            dx, dy = bx - ax, by - ay
            t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
            nearest = min(nearest, math.hypot(x - (ax + t * dx), y - (ay + t * dy)))
        return -nearest if inside else nearest
    return shape


def star(cx, cy, outer, inner, points=5, turn=0.0):
    corners = []
    for i in range(points * 2):
        angle = turn + i * math.pi / points - math.pi / 2
        r = outer if i % 2 == 0 else inner
        corners.append((cx + r * math.cos(angle), cy + r * math.sin(angle)))
    return polygon(corners)


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


WOOD = (150, 104, 62)
LIGHT_WOOD = (186, 138, 86)
STEEL = (190, 196, 204)
STRAP = (92, 58, 34)


def training_sword():
    return [
        (polygon([(9, 25), (22, 7), (25, 6), (24, 9), (11, 27)]), STEEL),  # the blade
        (capsule(6, 22, 13, 29, 1.6), (120, 84, 44)),  # the crossguard
        (capsule(6, 29, 9, 26, 1.8), STRAP),  # the grip
        (circle(5, 30, 2), (170, 140, 60)),  # the pommel
    ]


def wooden_staff():
    return [
        (capsule(8, 29, 23, 5, 1.8), WOOD),
        (circle(24, 5, 3.2), LIGHT_WOOD),  # the knob
        (detail(capsule(13, 21, 15, 18, 1.9)), STRAP),  # a leather wrap
    ]


def wooden_chestguard():
    return [
        (rounded_box(16, 17, 10, 11, 3), LIGHT_WOOD),
        (detail(capsule(9, 10, 9, 27, 0.8)), WOOD),  # plank edges
        (detail(capsule(16, 7, 16, 28, 0.8)), WOOD),
        (detail(capsule(23, 10, 23, 27, 0.8)), WOOD),
        (detail(capsule(6, 13, 26, 13, 1.3)), STRAP),  # straps
        (detail(capsule(6, 22, 26, 22, 1.3)), STRAP),
    ]


def wooden_buckler():
    iron = (120, 122, 132)
    return [
        (circle(16, 16, 12), iron),  # the rim
        (detail(circle(16, 16, 10)), LIGHT_WOOD),
        (detail(capsule(9, 12, 23, 12, 0.7)), WOOD),  # board edges
        (detail(capsule(7, 16, 25, 16, 0.7)), WOOD),
        (detail(capsule(9, 20, 23, 20, 0.7)), WOOD),
        (circle(16, 16, 3.5), iron),  # the boss
    ]


def wooden_legguards():
    return [
        (rounded_box(10.5, 17, 5, 12, 2.5), LIGHT_WOOD),
        (rounded_box(21.5, 17, 5, 12, 2.5), LIGHT_WOOD),
        (detail(capsule(6, 11, 15, 11, 1.2)), STRAP),
        (detail(capsule(17, 11, 26, 11, 1.2)), STRAP),
        (detail(capsule(6, 23, 15, 23, 1.2)), STRAP),
        (detail(capsule(17, 23, 26, 23, 1.2)), STRAP),
    ]


def plain_robes():
    blue = (72, 80, 140)
    return [
        (polygon([(12, 5), (20, 5), (27, 29), (5, 29)]), blue),
        (detail(capsule(11, 15, 21, 15, 1.2)), (200, 176, 104)),  # the sash
        (detail(polygon([(14, 5), (18, 5), (16, 10)])), (48, 52, 96)),  # the collar
    ]


# Ability, action and emote icons sit on a dark tile, so they read as buttons.

def tile(colour):
    return (rounded_box(16, 16, 14.5, 14.5, 4), colour)


def rest():
    return [
        tile((44, 52, 70)),
        (polygon([(8, 24), (24, 24), (22, 27), (10, 27)]), WOOD),  # logs
        (detail(capsule(9, 25, 23, 25, 1.0)), (110, 72, 40)),
        (polygon([(16, 7), (22, 20), (16, 24), (10, 20)]), (240, 150, 50)),  # the flame
        (detail(polygon([(16, 13), (19, 21), (16, 23), (13, 21)])), (255, 220, 110)),
    ]


def whack():
    return [
        tile((80, 40, 36)),
        (star(18, 14, 10, 4.5, 7), (250, 214, 90)),  # the impact
        (capsule(7, 27, 15, 17, 2.4), WOOD),  # a club swinging in
    ]


def air_blast():
    return [
        tile((50, 70, 96)),
        (detail(ring(16, 16, 9, 2.2)), (236, 244, 255)),
        (detail(ring(19, 13, 4, 1.8)), (236, 244, 255)),
        (circle(13, 19, 2.2), (255, 255, 255)),
    ]


def cleave():
    return [
        tile((80, 40, 36)),
        (detail(ring(16, 30, 18, 2.4)), (250, 214, 90)),  # a wide arc
        (polygon([(8, 22), (22, 6), (25, 6), (24, 9), (10, 24)]), STEEL),
    ]


def second_wind():
    return [
        tile((40, 72, 52)),
        (polygon([(16, 5), (19, 13), (27, 13), (21, 18), (23, 26), (16, 21), (9, 26), (11, 18), (5, 13), (13, 13)]),
         (120, 220, 130)),
    ]


def frost_shard():
    return [
        tile((50, 70, 96)),
        (polygon([(16, 4), (21, 16), (16, 28), (11, 16)]), (170, 226, 255)),
        (detail(polygon([(16, 7), (18, 16), (16, 25), (14, 16)])), (230, 248, 255)),
    ]


def arcane_bolt():
    return [
        tile((60, 44, 90)),
        (polygon([(19, 4), (9, 18), (15, 18), (12, 28), (23, 13), (17, 13)]), (200, 150, 255)),
    ]


def action_attack():
    return [
        tile((70, 50, 44)),
        (polygon([(7, 24), (21, 7), (24, 7), (23, 10), (9, 26)]), STEEL),
        (polygon([(25, 24), (11, 7), (8, 7), (9, 10), (23, 26)]), STEEL),
        (detail(capsule(5, 23, 10, 28, 1.3)), STRAP),
        (detail(capsule(27, 23, 22, 28, 1.3)), STRAP),
    ]


def action_talk():
    return [
        tile((56, 60, 70)),
        (ellipse(16, 14, 11, 8), (240, 236, 224)),
        (polygon([(10, 19), (15, 21), (8, 27)]), (240, 236, 224)),
        (detail(circle(11, 14, 1.5)), (90, 90, 100)),
        (detail(circle(16, 14, 1.5)), (90, 90, 100)),
        (detail(circle(21, 14, 1.5)), (90, 90, 100)),
    ]


def action_follow():
    return [
        tile((56, 60, 70)),
        (ellipse(11, 22, 3.5, 5), (220, 214, 200)),  # footprints
        (ellipse(21, 11, 3.5, 5), (220, 214, 200)),
        (detail(circle(9, 15.5, 1.3)), (220, 214, 200)),
        (detail(circle(19, 4.5, 1.3)), (220, 214, 200)),
    ]


def action_target():
    red = (225, 70, 55)
    return [
        tile((60, 46, 44)),
        (ring(16, 16, 9, 2.2), red),  # a reticle
        (capsule(16, 3.5, 16, 9, 1.3), red),
        (capsule(16, 23, 16, 28.5, 1.3), red),
        (capsule(3.5, 16, 9, 16, 1.3), red),
        (capsule(23, 16, 28.5, 16, 1.3), red),
        (circle(16, 16, 2.2), (250, 220, 150)),
    ]


def emote_wave():
    skin = (232, 190, 150)
    return [
        tile((70, 64, 50)),
        (rounded_box(16, 19, 6, 7, 3), skin),  # the palm
        (capsule(11.5, 14, 11.5, 7, 1.7), skin),  # fingers
        (capsule(15, 13, 15, 5, 1.7), skin),
        (capsule(18.5, 13, 18.5, 6, 1.7), skin),
        (capsule(22, 18, 25, 12, 1.7), skin),  # the thumb
        (detail(capsule(6, 8, 4, 12, 0.8)), (250, 240, 200)),  # motion lines
        (detail(capsule(27, 5, 29, 9, 0.8)), (250, 240, 200)),
    ]


def action_town():
    wall, roof = (226, 214, 190), (170, 70, 50)
    return [
        tile((56, 70, 60)),
        (rounded_box(16, 21, 9, 6, 1), wall),  # a house
        (polygon([(5, 16), (16, 6), (27, 16)]), roof),
        (detail(rounded_box(16, 23.5, 2.2, 3.5, 0.8)), (110, 70, 40)),  # its door
        (detail(rounded_box(10.5, 20, 1.8, 1.8, 0.4)), (120, 160, 200)),  # a window
        (detail(rounded_box(21.5, 20, 1.8, 1.8, 0.4)), (120, 160, 200)),
    ]


def skill_fishing():
    scales = (110, 160, 190)
    return [
        tile((40, 62, 80)),
        (ellipse(15, 17, 10, 5.5), scales),  # the fish
        (polygon([(23, 17), (29, 11), (29, 23)]), scales),  # its tail
        (detail(circle(10, 16, 1.4)), (30, 30, 40)),  # its eye
        (detail(capsule(14, 14, 18, 20, 0.7)), (80, 120, 150)),  # a gill line
    ]


def skill_woodworking():
    return [
        tile((56, 70, 44)),
        (capsule(9, 26, 22, 7, 1.8), WOOD),  # the handle
        (polygon([(18, 4), (27, 8), (25, 15), (19, 11)]), STEEL),  # the head
    ]


def skill_mining():
    # (No longer a skill of its own, part of Blacksmithing; the interface's placeholders still draw it.)
    return [
        tile((66, 60, 56)),
        (capsule(8, 26, 21, 9, 1.8), WOOD),  # the handle
        (polygon([(9, 9), (16, 5), (26, 6), (31, 12), (25, 9), (17, 9), (11, 12)]), STEEL),  # the pick's head
    ]


def skill_blacksmithing():
    iron = (110, 112, 120)
    return [
        tile((70, 50, 40)),
        (polygon([(5, 13), (27, 13), (24, 18), (20, 19), (21, 25), (11, 25), (12, 19), (8, 18)]), iron),  # the anvil
        (capsule(18, 9, 26, 3, 1.3), WOOD),  # the hammer's handle
        (rounded_box(16, 8, 4, 2.4, 0.8), STEEL),  # the hammer's head
        (detail(circle(24, 11, 1.1)), (255, 190, 90)),  # a spark
    ]


ICONS = {
    "miasma.png": miasma,
    "bread.png": bread,
    "meat_pie.png": meat_pie,
    "sweet_roll.png": sweet_roll,
    "old_pipe.png": old_pipe,
    "training_sword.png": training_sword,
    "wooden_staff.png": wooden_staff,
    "wooden_chestguard.png": wooden_chestguard,
    "wooden_legguards.png": wooden_legguards,
    "plain_robes.png": plain_robes,
    "wooden_buckler.png": wooden_buckler,
    "rest.png": rest,
    "whack.png": whack,
    "air_blast.png": air_blast,
    "cleave.png": cleave,
    "second_wind.png": second_wind,
    "frost_shard.png": frost_shard,
    "arcane_bolt.png": arcane_bolt,
    "action_attack.png": action_attack,
    "action_talk.png": action_talk,
    "action_follow.png": action_follow,
    "action_target.png": action_target,
    "emote_wave.png": emote_wave,
    "action_town.png": action_town,
    "skill_fishing.png": skill_fishing,
    "skill_woodworking.png": skill_woodworking,
    "skill_blacksmithing.png": skill_blacksmithing,
}


# Fishing: fish drawn from a few numbers, so each species gets its own icon
# (until they're painted), and the gear.

def fish(body, back, deep=1.0, stripes=None, spots=None, red_fins=False):
    """A fish facing left: `deep` makes it taller (bream) or slimmer (dace)."""
    fins = (200, 80, 60) if red_fins else back
    height = 5.5 * deep
    layers = [
        (polygon([(23, 16), (29, 16 - 5.5 * deep), (28, 16), (29, 16 + 5.5 * deep)]), fins),  # the tail
        (polygon([(12, 16 - height + 1), (17, 16 - height - 2.5), (20, 16 - height + 2)]), fins),  # the back fin
        (ellipse(15, 16, 10, height), body),
        (detail(ellipse(15, 16 - height * 0.45, 8.5, height * 0.45)), back),  # its darker back
    ]
    for x in stripes or []:
        layers.append((detail(capsule(x, 16 - height * 0.8, x - 1, 16 + height * 0.5, 0.8)), back))
    for (x, y) in spots or []:
        layers.append((detail(circle(x, y, 0.9)), back))
    layers += [
        (detail(circle(9, 15, 1.3)), (30, 30, 40)),  # the eye
        (detail(capsule(12, 14, 12.5, 18.5, 0.5)), back),  # the gill line
    ]
    return lambda: layers


def crayfish(shell):
    dark = tuple(c * 3 // 4 for c in shell)
    return lambda: [
        (capsule(8, 9, 5, 4, 1.4), shell),  # the claws
        (capsule(20, 9, 23, 4, 1.4), shell),
        (ellipse(5, 4, 2.4, 1.8), dark),
        (ellipse(23, 4, 2.4, 1.8), dark),
        (ellipse(14, 13, 4.5, 5), shell),  # the head
        (rounded_box(14, 22, 3.6, 5, 2), shell),  # the tail
        (polygon([(10, 28), (18, 28), (14, 31)]), dark),  # its fan
        (detail(capsule(11, 20, 17, 20, 0.5)), dark),
        (detail(capsule(11, 23, 17, 23, 0.5)), dark),
    ]


def crab(shell):
    dark = tuple(c * 3 // 4 for c in shell)
    legs = [(capsule(x0, 18, x1, 25, 1.0), dark) for x0, x1 in [(9, 4), (11, 7), (21, 25), (23, 28)]]
    return lambda: legs + [
        (capsule(9, 13, 5, 7, 1.5), shell),  # the claws
        (capsule(23, 13, 27, 7, 1.5), shell),
        (ellipse(16, 17, 10, 7), shell),
        (detail(circle(13, 12, 1.1)), (30, 30, 30)),
        (detail(circle(19, 12, 1.1)), (30, 30, 30)),
    ]


def shell(colour, spiral):
    dark = tuple(c * 2 // 3 for c in colour)
    return lambda: [
        (polygon([(6, 24), (14, 6), (26, 22), (20, 27)]) if not spiral else circle(16, 17, 10), colour),
        (detail(ring(16, 17, 5.5, 1.3) if spiral else capsule(10, 22, 16, 9, 0.8)), dark),
        (detail(ring(16, 17, 2.2, 1.2) if spiral else capsule(15, 25, 20, 12, 0.8)), dark),
    ]


def mussel():
    return [
        (polygon([(5, 22), (9, 10), (18, 5), (27, 9), (24, 20), (12, 27)]), (46, 52, 70)),
        (detail(capsule(9, 20, 22, 10, 0.7)), (90, 100, 130)),
    ]


def shrimp(colour):
    dark = tuple(c * 3 // 4 for c in colour)
    return lambda: [
        (capsule(8, 12, 3, 5, 0.6), dark),  # the feelers
        (ring(17, 14, 8, 5), colour),
        (polygon([(6, 17), (12, 14), (12, 20)]), colour),  # the head
        (polygon([(23, 22), (29, 28), (22, 29)]), dark),  # the tail
        (detail(circle(9, 15, 1.0)), (30, 30, 30)),
    ]


def rod(handle):
    return lambda: [
        (capsule(5, 28, 28, 4, 0.9), (120, 90, 60)),  # the rod
        (capsule(5, 28, 11, 22, 2.0), handle),  # the handle
        (detail(capsule(28, 4, 29, 18, 0.35)), (220, 220, 220)),  # the line
        (circle(12, 21, 2.4), (120, 124, 130)),  # the reel
    ]


def reel(colour):
    dark = tuple(c * 2 // 3 for c in colour)
    return lambda: [
        (capsule(16, 5, 16, 9, 1.5), dark),  # the foot
        (circle(16, 18, 10), colour),
        (detail(ring(16, 18, 6, 2.5)), (220, 216, 200)),  # the line on the spool
        (detail(circle(16, 18, 2)), dark),
        (capsule(24, 22, 28, 27, 1.3), dark),  # the handle
    ]


def worm():
    pink = (206, 120, 120)
    return [
        (capsule(6, 22, 12, 14, 2.2), pink),
        (capsule(12, 14, 19, 20, 2.2), pink),
        (capsule(19, 20, 26, 12, 2.2), pink),
        (detail(capsule(11, 16, 13, 17, 0.6)), (160, 80, 84)),
    ]


def sweetcorn():
    return [(circle(x, y, 2.6), (240, 200, 70)) for x, y in [(10, 12), (16, 10), (22, 12), (8, 18), (14, 17), (20, 17), (25, 19), (12, 23), (18, 24)]]


def paste_ball():
    return [
        (circle(16, 17, 10), (206, 184, 140)),
        (detail(circle(12, 13, 1.2)), (150, 128, 90)),
        (detail(circle(19, 20, 1.2)), (150, 128, 90)),
        (detail(circle(18, 12, 1.0)), (150, 128, 90)),
    ]


def shrimp_bits():
    pink = (230, 150, 130)
    return [
        (rounded_box(10, 12, 4, 3, 1.5), pink),
        (rounded_box(21, 14, 4, 3, 1.5), pink),
        (rounded_box(14, 22, 4, 3, 1.5), pink),
        (rounded_box(24, 24, 3, 2.5, 1.2), pink),
    ]


def spinner():
    return [
        (capsule(16, 4, 16, 27, 0.6), (140, 140, 150)),  # the wire
        (ellipse(16, 13, 5, 7.5), (230, 206, 110)),  # the blade
        (detail(ellipse(15, 11, 2, 3.5)), (255, 244, 190)),
        (capsule(16, 21, 16, 25, 2.0), (200, 60, 50)),  # the body
        (detail(capsule(13, 29, 19, 29, 0.6)), (140, 140, 150)),  # the hook
    ]


def plug():
    return [
        (ellipse(15, 16, 11, 5), (110, 130, 90)),
        (detail(ellipse(15, 14, 9, 2.4)), (70, 90, 60)),
        (polygon([(3, 17), (7, 14), (7, 20)]), (200, 200, 190)),  # the lip
        (detail(circle(8, 15, 1.2)), (240, 200, 60)),
        (detail(capsule(13, 22, 13, 26, 0.6)), (140, 140, 150)),  # the hooks
        (detail(capsule(21, 21, 21, 25, 0.6)), (140, 140, 150)),
    ]


def cage_trap():
    wire = (150, 150, 140)
    bars = [(detail(capsule(x, 9, x, 25, 0.6)), wire) for x in (10, 14, 18, 22)]
    return [(rounded_box(16, 17, 11, 9, 2), (60, 54, 44))] + bars + [
        (detail(capsule(6, 17, 26, 17, 0.6)), wire),
        (detail(ring(16, 17, 3, 1.2)), (180, 170, 150)),  # the funnel
    ]


FISHING_ICONS = {
    "dace.png": fish((200, 206, 210), (100, 116, 128), deep=0.8),
    "gudgeon.png": fish((180, 170, 140), (110, 100, 80), deep=0.75, spots=[(13, 17), (17, 17), (21, 16)]),
    "roach.png": fish((200, 204, 210), (80, 100, 110), deep=1.1, red_fins=True),
    "chub.png": fish((190, 186, 170), (80, 86, 80), deep=0.95),
    "perch.png": fish((170, 180, 90), (70, 90, 50), deep=1.15, stripes=[12, 16, 20], red_fins=True),
    "bream.png": fish((170, 150, 100), (100, 86, 60), deep=1.5),
    "crucian_carp.png": fish((200, 160, 70), (130, 100, 40), deep=1.3),
    "rainbow_trout.png": fish((190, 190, 200), (80, 110, 90), deep=0.95, spots=[(12, 13), (16, 12), (20, 13), (18, 15)]),
    "grey_mullet.png": fish((170, 176, 184), (90, 100, 110), deep=0.9, stripes=[13, 17, 21]),
    "white_clawed_crayfish.png": crayfish((140, 120, 90)),
    "signal_crayfish.png": crayfish((130, 90, 60)),
    "red_swamp_crayfish.png": crayfish((170, 50, 40)),
    "noble_crayfish.png": crayfish((100, 70, 60)),
    "shore_crab.png": crab((90, 110, 60)),
    "brown_shrimp.png": shrimp((170, 150, 120)),
    "river_snail.png": shell((120, 100, 70), spiral=True),
    "common_whelk.png": shell((210, 190, 160), spiral=False),
    "freshwater_mussel.png": mussel,
    "ledger_rod.png": rod((120, 80, 50)),
    "ash_ledger_rod.png": rod((190, 150, 90)),
    "float_rod.png": rod((200, 170, 120)),
    "lure_rod.png": rod((60, 60, 70)),
    "wooden_reel.png": reel((150, 104, 62)),
    "brass_reel.png": reel((200, 160, 70)),
    "worm.png": worm,
    "sweetcorn.png": sweetcorn,
    "paste_ball.png": paste_ball,
    "shrimp_bits.png": shrimp_bits,
    "live_shrimp.png": shrimp((220, 140, 120)),
    "spinner.png": spinner,
    "plug.png": plug,
    "cage_trap.png": cage_trap,
}
ICONS.update(FISHING_ICONS)


def logs(bark, heart):
    """A stack of three logs, their cut ends showing rings."""

    def draw():
        layers = []
        for cx, cy in [(10, 21), (22, 21), (16, 11)]:
            layers.append((circle(cx, cy, 6.2), bark))
            layers.append((detail(circle(cx, cy, 4.6)), heart))
            layers.append((detail(ring(cx, cy, 2.6, 0.7)), tuple(max(0, c - 40) for c in heart)))
            layers.append((detail(circle(cx, cy, 0.9)), tuple(max(0, c - 60) for c in heart)))
        return layers

    return draw


def hazel_rods():
    hazel = (150, 104, 70)
    return [(capsule(6 + i * 3, 27, 12 + i * 3, 4, 1.4), hazel) for i in range(5)] + [
        (detail(capsule(5, 16, 27, 16, 0.8)), (120, 150, 80)),  # the twine binding them
    ]


def stone_axe():
    flint = (120, 118, 124)
    return [
        (capsule(10, 28, 20, 6, 1.7), WOOD),  # the haft
        (polygon([(15, 5), (27, 4), (29, 12), (24, 15), (16, 11)]), flint),  # the knapped head
        (detail(capsule(15, 9, 20, 11, 0.7)), (150, 120, 80)),  # the lashing
        (detail(polygon([(20, 6), (25, 6), (26, 9)])), (160, 158, 166)),  # a knapped facet
    ]


WOOD_ICONS = {
    "hazel_rods.png": hazel_rods,
    "birch_logs.png": logs((225, 222, 210), (236, 222, 190)),
    "willow_logs.png": logs((120, 110, 90), (226, 206, 170)),
    "pine_logs.png": logs((170, 96, 60), (232, 196, 140)),
    "hawthorn_logs.png": logs((110, 100, 92), (214, 180, 140)),
    "sycamore_logs.png": logs((150, 140, 128), (238, 226, 200)),
    "ash_logs.png": logs((150, 150, 142), (230, 212, 176)),
    "beech_logs.png": logs((160, 160, 156), (222, 176, 130)),
    "oak_logs.png": logs((96, 84, 72), (196, 150, 96)),
    "yew_logs.png": logs((140, 76, 56), (220, 150, 70)),
    "stone_axe.png": stone_axe,
}
ICONS.update(WOOD_ICONS)


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
