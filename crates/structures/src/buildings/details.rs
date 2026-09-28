//! A building's details: its openings (the sides through the wall, lintels
//! and sills or wooden frames, windows' glass and frames, shutters, and
//! doors, standing open), its floors, and its inner walls.

use glam::{Vec2, Vec3};

use super::faces::{Hole, LINTEL, face, facing};
use super::plan::{Opening, OpeningKind, wings};
use super::{Builder, Detail, Surface, WallConstruction, quad_facing};

/// An inner wall: from one point to another (along its middle), in metres.
pub type InnerWall = ((f32, f32), (f32, f32));

/// How thick a door, a window's frame, and a shutter are, in metres.
const LEAF: f32 = 0.05;
/// How far open outer doors stand, in radians.
const DOOR_OPEN: f32 = 1.3;
/// How thick a floor between storeys is, in metres.
pub const FLOOR_THICKNESS: f32 = 0.22;
/// Glass (linear).
const GLASS: [f32; 4] = [0.035, 0.045, 0.05, 1.0];

/// Every outer wall's openings: the sides through the wall, lintels and
/// sills (stone) or frames (timber), glass and frames, shutters and doors.
pub fn openings(builder: &mut Builder, openings: &[Vec<Opening>]) {
    let thickness = builder.style.wall_thickness;
    let storey = builder.style.storey;
    let stone = builder.style.walls == WallConstruction::Stone;
    for (i, wall) in openings.iter().enumerate() {
        let (a, b) = builder.footprint.edge(i);
        let way = (b - a).normalize_or(Vec2::X);
        let (along, out) = (builder.direction(way), builder.direction(-way.perp()));
        let start = builder.point(a, 0.0);
        for opening in wall {
            let upper = opening.bottom >= storey - 0.01;
            let at = |x: f32, y: f32, depth: f32| start + along * x + Vec3::Y * y + out * depth;
            let (from, to, bottom, top) = (opening.from, opening.to, opening.bottom, opening.top);
            let width = to - from;

            // Through the wall: its sides, its head, and (a window's) its sill.
            let lining = if stone { Surface::Stone } else { Surface::Wood };
            let colour = if stone { builder.colours.walls.map(|c| c * 0.85) } else { builder.colours.trim };
            let mesh = builder.mesh(lining, Detail::Full, upper);
            let depth = -thickness;
            quad_facing(
                mesh,
                [at(from, bottom, 0.0), at(from, bottom, depth), at(from, top, depth), at(from, top, 0.0)],
                [[0.0, 0.0], [thickness, 0.0], [thickness, top - bottom], [0.0, top - bottom]],
                colour,
                along,
            );
            quad_facing(
                mesh,
                [at(to, bottom, 0.0), at(to, bottom, depth), at(to, top, depth), at(to, top, 0.0)],
                [[0.0, 0.0], [thickness, 0.0], [thickness, top - bottom], [0.0, top - bottom]],
                colour,
                -along,
            );
            quad_facing(
                mesh,
                [at(from, top, 0.0), at(to, top, 0.0), at(to, top, depth), at(from, top, depth)],
                [[0.0, 0.0], [width, 0.0], [width, thickness], [0.0, thickness]],
                colour,
                -Vec3::Y,
            );
            if opening.kind == OpeningKind::Window {
                quad_facing(
                    mesh,
                    [at(from, bottom, 0.0), at(to, bottom, 0.0), at(to, bottom, depth), at(from, bottom, depth)],
                    [[0.0, 0.0], [width, 0.0], [width, thickness], [0.0, thickness]],
                    colour,
                    Vec3::Y,
                );
            }

            if stone {
                // A stone lintel over it, and (a window) a sill under it, standing a little proud.
                let lintel = builder.colours.walls.map(|c| c * builder.random.range(0.95, 1.1));
                let lumps = std::array::from_fn(|_| builder.random.signed() * 0.03);
                let centre = at((from + to) / 2.0, top + LINTEL.1 / 2.0, 0.02 - 0.15);
                let size = Vec3::new(0.3, LINTEL.1, width + LINTEL.0 * 2.0);
                builder.mesh(Surface::Stone, Detail::Full, upper).block(
                    centre,
                    facing(along),
                    size,
                    lumps,
                    [0.3, 0.1],
                    lintel,
                );
                if opening.kind == OpeningKind::Window {
                    let centre = at((from + to) / 2.0, bottom - 0.05, 0.06 - 0.15);
                    let size = Vec3::new(0.3, 0.1, width + 0.16);
                    builder.mesh(Surface::Stone, Detail::Full, upper).block(
                        centre,
                        facing(along),
                        size,
                        lumps,
                        [0.7, 0.2],
                        lintel,
                    );
                }
            } else {
                // A wooden frame round it, on the boards.
                let trim = builder.colours.trim;
                let mesh = builder.mesh(Surface::Wood, Detail::Full, upper);
                let frame = 0.09;
                let bars = [
                    (
                        at(from - frame / 2.0, (bottom + top) / 2.0, 0.03),
                        Vec3::new(0.06, top - bottom + frame * 2.0, frame),
                    ),
                    (
                        at(to + frame / 2.0, (bottom + top) / 2.0, 0.03),
                        Vec3::new(0.06, top - bottom + frame * 2.0, frame),
                    ),
                    (at((from + to) / 2.0, top + frame / 2.0, 0.03), Vec3::new(0.06, frame, width + frame * 2.0)),
                ];
                for (centre, size) in bars {
                    mesh.block(centre, facing(along), size, [0.0; 8], [0.0, 0.0], trim);
                }
                if opening.kind == OpeningKind::Window {
                    mesh.block(
                        at((from + to) / 2.0, bottom - frame / 2.0, 0.05),
                        facing(along),
                        Vec3::new(0.1, frame, width + frame * 2.2),
                        [0.0; 8],
                        [0.0, 0.0],
                        trim,
                    );
                }
            }

            match opening.kind {
                OpeningKind::Window => window(builder, (start, along, out), opening, upper),
                OpeningKind::Door => door(builder, (start, along, out), opening, stone),
            }
        }
    }
}

/// A window's glass (seen from either side), its frame and bars, and shutters.
fn window(builder: &mut Builder, (start, along, out): (Vec3, Vec3, Vec3), opening: &Opening, upper: bool) {
    let thickness = builder.style.wall_thickness;
    let (from, to, bottom, top) = (opening.from, opening.to, opening.bottom, opening.top);
    let depth = -thickness * 0.4;
    let at = |x: f32, y: f32, d: f32| start + along * x + Vec3::Y * y + out * d;
    let glass = builder.mesh(Surface::Dark, Detail::Full, upper);
    let pane = [at(from, bottom, depth), at(to, bottom, depth), at(to, top, depth), at(from, top, depth)];
    quad_facing(glass, pane, [[0.0; 2]; 4], GLASS, out);
    quad_facing(glass, pane, [[0.0; 2]; 4], GLASS, -out);
    // The frame round the glass, and a cross of bars.
    let trim = builder.colours.trim;
    let (width, height) = (to - from, top - bottom);
    let (middle_x, middle_y) = ((from + to) / 2.0, (bottom + top) / 2.0);
    let bar = 0.05;
    let frame = builder.mesh(Surface::Wood, Detail::Full, upper);
    for (centre, size) in [
        (at(from + bar / 2.0, middle_y, depth), Vec3::new(LEAF, height, bar)),
        (at(to - bar / 2.0, middle_y, depth), Vec3::new(LEAF, height, bar)),
        (at(middle_x, top - bar / 2.0, depth), Vec3::new(LEAF, bar, width)),
        (at(middle_x, bottom + bar / 2.0, depth), Vec3::new(LEAF, bar, width)),
        (at(middle_x, middle_y, depth), Vec3::new(LEAF * 0.8, height, bar * 0.6)),
        (at(middle_x, middle_y + height * 0.1, depth), Vec3::new(LEAF * 0.8, bar * 0.6, width)),
    ] {
        frame.block(centre, facing(along), size, [0.0; 8], [0.0, 0.0], trim);
    }
    if builder.style.window.shutters {
        // Open, flat against the wall either side.
        let door = builder.colours.door;
        let half = width / 2.0;
        let mesh = builder.mesh(Surface::Wood, Detail::Full, upper);
        for x in [from - half / 2.0 - 0.12, to + half / 2.0 + 0.12] {
            mesh.block(
                at(x, middle_y, 0.06),
                facing(along),
                Vec3::new(0.035, height, half),
                [0.0; 8],
                [x, bottom],
                door,
            );
        }
    }
}

/// A door, standing open (into the building), and a step before it.
fn door(builder: &mut Builder, (start, along, out): (Vec3, Vec3, Vec3), opening: &Opening, stone: bool) {
    let thickness = builder.style.wall_thickness;
    let (from, to, top) = (opening.from, opening.to, opening.top);
    let width = to - from;
    // Hinged on one side (the seed says which), just inside.
    let left = builder.random.unit() < 0.5;
    let hinge_x = if left { from + 0.04 } else { to - 0.04 };
    let hinge = start + along * hinge_x - out * (thickness - 0.06);
    let closed = if left { along } else { -along };
    let open = (closed * DOOR_OPEN.cos() - out * DOOR_OPEN.sin()).normalize();
    let leaf_width = width - 0.06;
    let centre = hinge + open * (leaf_width / 2.0) + Vec3::Y * (top - 0.02) / 2.0;
    let door = builder.colours.door;
    let mesh = builder.mesh(Surface::Wood, Detail::Full, false);
    mesh.block(centre, facing(open), Vec3::new(LEAF, top - 0.04, leaf_width), [0.0; 8], [0.0, 0.0], door);
    // Ledges across its back.
    let across = open.cross(Vec3::Y).normalize_or(Vec3::X);
    for height in [0.35, top * 0.5, top - 0.35] {
        let at = hinge + open * (leaf_width / 2.0) + Vec3::Y * height + across * (LEAF * 0.9);
        mesh.block(
            at,
            facing(open),
            Vec3::new(0.03, 0.12, leaf_width * 0.9),
            [0.0; 8],
            [0.0, 0.0],
            door.map(|c| c * 0.85),
        );
    }
    // A step outside.
    let step = if stone { Surface::Stone } else { Surface::Wood };
    let colour = if stone { builder.colours.walls.map(|c| c * 0.9) } else { builder.colours.trim };
    let lumps = std::array::from_fn(|_| builder.random.signed() * 0.04);
    let centre = start + along * (from + to) / 2.0 + out * 0.18 + Vec3::Y * -0.06;
    builder.mesh(step, Detail::Full, false).block(
        centre,
        facing(along),
        Vec3::new(0.4, 0.2, width + 0.2),
        lumps,
        [0.1, 0.4],
        colour,
    );
}

/// Floors: the ground floor's boards, and each floor above (boards on top, a ceiling under).
pub fn floors(builder: &mut Builder) {
    let rects = wings(&builder.footprint.inside);
    let storey = builder.style.storey;
    let boards = builder.colours.trim.map(|c| c * 1.25);
    for level in 0..builder.floors {
        let height = level as f32 * storey;
        let upper = level > 0;
        for rect in &rects {
            let frame = builder.footprint.frame;
            let corners = |y: f32| {
                [
                    frame.point(rect.low, y),
                    frame.point(Vec2::new(rect.high.x, rect.low.y), y),
                    frame.point(rect.high, y),
                    frame.point(Vec2::new(rect.low.x, rect.high.y), y),
                ]
            };
            let uvs = [
                [rect.low.x, rect.low.y],
                [rect.high.x, rect.low.y],
                [rect.high.x, rect.high.y],
                [rect.low.x, rect.high.y],
            ];
            let top = corners(height + 0.02);
            quad_facing(builder.mesh(Surface::Floor, Detail::Full, upper), top, uvs, boards, Vec3::Y);
            if upper {
                let under = corners(height - FLOOR_THICKNESS);
                let plaster = builder.colours.plaster;
                quad_facing(builder.mesh(Surface::Plaster, Detail::Full, true), under, uvs, plaster, -Vec3::Y);
            }
        }
    }
}

/// Inner walls, storey by storey, with their doorways (every floor has the
/// same plan). Their ends reach into the outer walls.
pub fn inner_walls(builder: &mut Builder, walls: &[InnerWall], doors: &[Vec<Opening>]) {
    let half = builder.style.inner_thickness / 2.0;
    let storey = builder.style.storey;
    let plaster = builder.colours.plaster;
    for (i, &(from, to)) in walls.iter().enumerate() {
        let frame = builder.footprint.frame;
        let (from, to) = (
            snap_to_inside(builder, frame.local(Vec2::from(from))),
            snap_to_inside(builder, frame.local(Vec2::from(to))),
        );
        let length = from.distance(to);
        if length < 0.3 {
            continue;
        }
        let way = (to - from) / length;
        let side = way.perp();
        let along = builder.direction(way);
        for level in 0..builder.floors {
            let upper = level > 0;
            let low = level as f32 * storey;
            let high = if level + 1 == builder.floors {
                builder.eaves()
            } else {
                (level + 1) as f32 * storey - FLOOR_THICKNESS
            };
            let holes: Vec<Hole> = doors
                .get(i)
                .map(|doors| doors.iter().map(|d| (d.from, d.to, low + d.bottom, low + d.top)).collect())
                .unwrap_or_default();
            for sign in [1.0f32, -1.0] {
                let start = builder.point(from + side * half * sign, 0.0);
                let normal = builder.direction(side * sign);
                face(
                    builder.mesh(Surface::Plaster, Detail::Full, upper),
                    start,
                    along,
                    normal,
                    (length, low, high),
                    &holes,
                    0.0,
                    plaster,
                );
            }
            // Its top, and through its doorways.
            let frame = builder.footprint.frame;
            let point = |x: f32, s: f32, y: f32| frame.point(from + way * x + side * half * s, y);
            let top =
                [point(0.0, 1.0, high), point(length, 1.0, high), point(length, -1.0, high), point(0.0, -1.0, high)];
            let mut pieces = vec![(top, Vec3::Y)];
            for &(f, t, b, tp) in &holes {
                pieces.push(([point(f, 1.0, b), point(f, -1.0, b), point(f, -1.0, tp), point(f, 1.0, tp)], along));
                pieces.push(([point(t, 1.0, b), point(t, -1.0, b), point(t, -1.0, tp), point(t, 1.0, tp)], -along));
                pieces.push(([point(f, 1.0, tp), point(t, 1.0, tp), point(t, -1.0, tp), point(f, -1.0, tp)], -Vec3::Y));
            }
            let mesh = builder.mesh(Surface::Plaster, Detail::Full, upper);
            for (corners, toward) in pieces {
                quad_facing(mesh, corners, [[0.0; 2]; 4], plaster, toward);
            }
        }
    }
}

/// An inner wall's end near an outer wall, moved onto that wall's inside
/// (and a little into it, so no gap shows).
fn snap_to_inside(builder: &Builder, point: Vec2) -> Vec2 {
    let inside = &builder.footprint.inside;
    let reach = builder.style.wall_thickness + 0.6;
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    for i in 0..inside.len() {
        let (a, b) = (inside[i], inside[(i + 1) % inside.len()]);
        let run = b - a;
        let t = ((point - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let nearest = a + run * t;
        let distance = nearest.distance(point);
        if best.is_none_or(|(d, _, _)| distance < d) {
            best = Some((distance, nearest, -run.normalize_or(Vec2::X).perp()));
        }
    }
    match best {
        Some((distance, nearest, outward)) if distance < reach => nearest + outward * 0.05,
        _ => point,
    }
}
