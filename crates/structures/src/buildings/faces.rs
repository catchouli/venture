//! Walls' faces: flat pieces with openings cut out, stones laid in
//! courses (with quoins at the corners), and overlapping boards (with posts
//! at the corners), on a stone plinth, plastered inside.

use glam::{Quat, Vec2, Vec3};

use super::plan::Opening;
use super::{Builder, Detail, Surface, WallConstruction, quad_facing};
use crate::mesh::MeshPart;

/// How far behind a wall's face the mortar between its stones (or the
/// dark between its boards) is, in metres.
const SET_BACK: f32 = 0.03;
/// A quoin's long and short sides, in metres.
const QUOIN: (f32, f32) = (0.52, 0.3);
/// How much a quoin stands proud of the wall, in metres.
const QUOIN_PROUD: f32 = 0.03;
/// How far a lintel reaches past its opening either side, and how tall it is, in metres.
pub const LINTEL: (f32, f32) = (0.18, 0.24);
/// How far up a timber building's stone plinth comes, in metres.
const TIMBER_PLINTH: f32 = 0.35;
/// How deep the timber wall plate along the walls' tops is, in metres.
const PLATE: f32 = 0.14;
/// How tall a board is, in metres, and how far its bottom edge stands out.
const BOARD: (f32, f32) = (0.22, 0.05);
/// How thick a corner post is, in metres.
const POST: f32 = 0.2;

/// A rectangle cut out of a face: along it (from, to) and up it (bottom, top).
pub type Hole = (f32, f32, f32, f32);

/// Adds a flat face: `length` along `along` from `start` (building space),
/// from `low` to `high` up it, facing `normal`, less `holes`. Its texture
/// is laid a unit a metre, from `u` along.
#[allow(clippy::too_many_arguments)]
pub fn face(
    mesh: &mut MeshPart,
    start: Vec3,
    along: Vec3,
    normal: Vec3,
    (length, low, high): (f32, f32, f32),
    holes: &[Hole],
    u: f32,
    colour: [f32; 4],
) {
    if length <= 1e-3 || high <= low {
        return;
    }
    let mut cuts = vec![0.0, length];
    for &(from, to, bottom, top) in holes {
        if top > low && bottom < high {
            cuts.extend([from.clamp(0.0, length), to.clamp(0.0, length)]);
        }
    }
    cuts.sort_by(f32::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    // Wound to face `normal`.
    let flipped = along.cross(Vec3::Y).dot(normal) < 0.0;
    for pair in cuts.windows(2) {
        let (x0, x1) = (pair[0], pair[1]);
        let middle = (x0 + x1) / 2.0;
        // What's left of this column once the holes in it are taken out.
        let mut spans = vec![(low, high)];
        for &(from, to, bottom, top) in holes {
            if from <= middle && middle <= to {
                spans = spans
                    .into_iter()
                    .flat_map(|(a, b)| if top <= a || bottom >= b { vec![(a, b)] } else { vec![(a, bottom), (top, b)] })
                    .filter(|(a, b)| b - a > 1e-4)
                    .collect();
            }
        }
        for (y0, y1) in spans {
            let at = |x: f32, y: f32| start + along * x + Vec3::Y * y;
            let (xa, xb) = if flipped { (x1, x0) } else { (x0, x1) };
            mesh.quad(
                [at(xa, y0), at(xb, y0), at(xb, y1), at(xa, y1)],
                [[u + xa, -y0], [u + xb, -y0], [u + xb, -y1], [u + xa, -y1]],
                colour,
            );
        }
    }
}

/// The stone courses of a building, from its footing to above its eaves
/// (every wall uses the same, so quoins line up round corners).
pub fn courses(builder: &mut Builder) -> Vec<(f32, f32)> {
    let top = builder.eaves() + 3.0 * builder.style.storey;
    let mut courses = Vec::new();
    let mut bottom = 0.0;
    while bottom < top {
        let height = builder.random.range(0.15, 0.26);
        courses.push((bottom, bottom + height));
        bottom += height;
    }
    courses
}

/// The outer walls, storey by storey, with their openings cut out (and a
/// far version: flat, whole).
#[allow(clippy::needless_range_loop)]
pub fn outer_walls(builder: &mut Builder, openings: &[Vec<Opening>]) {
    let count = builder.footprint.outline.len();
    let courses = courses(builder);
    let eaves = builder.eaves();
    let storey = builder.style.storey;
    let footing = builder.footing;
    let construction = builder.style.walls;
    let (walls, plaster) = (builder.colours.walls, builder.colours.plaster);
    let dark = walls.map(|c| c * 0.4);
    let wall_surface = match construction {
        WallConstruction::Stone => Surface::Stone,
        WallConstruction::Timber => Surface::Boards,
    };
    let mut u = 0.0;
    for i in 0..count {
        let (a, b) = builder.footprint.edge(i);
        let length = a.distance(b);
        let way = (b - a) / length;
        let outward = -way.perp();
        let (along, out) = (builder.direction(way), builder.direction(outward));
        let start = builder.point(a, 0.0);
        // A corner's turn: how sharply the outline turns there (positive
        // turning in: a convex corner). Quoins only go on convex right angles.
        let turn = |corner: usize| {
            let before = builder.footprint.outline[(corner + count - 1) % count];
            let (here, after) =
                (builder.footprint.outline[corner % count], builder.footprint.outline[(corner + 1) % count]);
            let (d0, d1) = ((here - before).normalize_or(Vec2::X), (after - here).normalize_or(Vec2::X));
            d0.perp_dot(d1).atan2(d0.dot(d1))
        };
        let convex = |corner: usize| (turn(corner) - std::f32::consts::FRAC_PI_2).abs() < 0.05;
        let (convex_start, convex_end) = (convex(i), convex(i + 1));
        let holes: Vec<Hole> = openings[i].iter().map(|o| (o.from, o.to, o.bottom, o.top)).collect();

        // The plinth, standing a little proud, from the footing up.
        let plinth_top = if construction == WallConstruction::Timber { TIMBER_PLINTH } else { 0.08 };
        let proud = 0.05;
        // (Reaching round the corner to meet the next wall's plinth, at whatever angle.)
        let (s0, s1) = (-proud * (turn(i) / 2.0).tan(), proud * (turn(i + 1) / 2.0).tan());
        for detail in [Detail::Full, Detail::Far] {
            let mesh = builder.mesh(Surface::Stone, detail, false);
            let from = start + out * proud + along * s0;
            let extent = (length - s0 + s1, -footing, plinth_top);
            face(mesh, from, along, out, extent, &[], u, walls.map(|c| c * 0.8));
            // Its top, stepping back to the wall.
            let top = |x: f32, depth: f32| start + along * x + out * depth + Vec3::Y * plinth_top;
            let corners = [top(s0, proud), top(s0, 0.0), top(length + s1, 0.0), top(length + s1, proud)];
            quad_facing(mesh, corners, [[0.0; 2]; 4], walls, Vec3::Y);
        }

        // Far away: the whole wall, flat, with its openings dark.
        let far_surface = wall_surface;
        {
            let mesh = builder.mesh(far_surface, Detail::Far, false);
            face(mesh, start, along, out, (length, plinth_top, eaves), &[], u, walls);
            for opening in &openings[i] {
                let dark_mesh = builder.mesh(Surface::Dark, Detail::Far, false);
                let front = start + out * 0.01;
                face(
                    dark_mesh,
                    front + along * opening.from,
                    along,
                    out,
                    (opening.to - opening.from, opening.bottom, opening.top),
                    &[],
                    0.0,
                    [0.02, 0.02, 0.02, 1.0],
                );
            }
        }

        // Close up, storey by storey.
        for level in 0..builder.floors {
            let (low, high) = (level as f32 * storey, (level + 1) as f32 * storey);
            let low = low.max(plinth_top);
            let upper = level > 0;
            match construction {
                WallConstruction::Stone => {
                    // The mortar behind the stones, then the stones.
                    let mesh = builder.mesh(Surface::Stone, Detail::Full, upper);
                    face(mesh, start - out * SET_BACK, along, out, (length, low, high), &holes, u, dark);
                    let reserve = |k: usize, corner_convex: bool, at_start: bool| {
                        if !corner_convex {
                            0.0
                        } else if k.is_multiple_of(2) == at_start {
                            QUOIN.0
                        } else {
                            QUOIN.1
                        }
                    };
                    // Openings, with room for their lintels (and windows' sills).
                    let kept_clear: Vec<Hole> = openings[i]
                        .iter()
                        .flat_map(|o| {
                            let sill = if o.bottom > 0.01 { 0.1 } else { 0.0 };
                            [
                                (o.from, o.to, o.bottom, o.top),
                                (o.from - LINTEL.0, o.to + LINTEL.0, o.top, o.top + LINTEL.1),
                                (o.from - 0.08, o.to + 0.08, o.bottom - sill, o.bottom),
                            ]
                        })
                        .collect();
                    for (k, &(y0, y1)) in courses.iter().enumerate() {
                        if y1 <= low || y0 >= high {
                            continue;
                        }
                        let (y0, y1) = (y0.max(low), y1.min(high));
                        let from = reserve(k, convex_start, true);
                        let to = length - reserve(k, convex_end, false);
                        lay_course(builder, (start, along, out), (from, to), (y0, y1), &kept_clear, upper);
                        if convex_start {
                            quoin(builder, i, k, (y0, y1), upper);
                        }
                    }
                }
                WallConstruction::Timber => {
                    let mesh = builder.mesh(Surface::Boards, Detail::Full, upper);
                    face(mesh, start - out * SET_BACK, along, out, (length, low, high), &holes, u, dark);
                    let post = |convex: bool| if convex { POST / 2.0 } else { 0.0 };
                    let (from, to) = (post(convex_start), length - post(convex_end));
                    let mut y = low;
                    while y < high - 1e-3 {
                        let top = (y + BOARD.0).min(high);
                        let clear: Vec<Hole> =
                            holes.iter().map(|&(f, t, b, tp)| (f - 0.09, t + 0.09, b - 0.09, tp + 0.09)).collect();
                        for (x0, x1) in free_between((from, to), (y, top), &clear) {
                            let shade = builder.random.range(0.85, 1.05);
                            let colour = walls.map(|c| c * shade);
                            let at =
                                |x: f32, height: f32, depth: f32| start + along * x + Vec3::Y * height + out * depth;
                            let mesh = builder.mesh(Surface::Boards, Detail::Full, upper);
                            let corners = [at(x0, y, BOARD.1), at(x1, y, BOARD.1), at(x1, top, 0.0), at(x0, top, 0.0)];
                            let uvs =
                                [[u + x0, -y * 3.0], [u + x1, -y * 3.0], [u + x1, -top * 3.0], [u + x0, -top * 3.0]];
                            quad_facing(mesh, corners, uvs, colour, out);
                        }
                        y = top;
                    }
                    if convex_start {
                        let before = builder.footprint.outline[(i + count - 1) % count];
                        let out_before = builder.direction(-(a - before).normalize_or(Vec2::X).perp());
                        let centre = start + (out + out_before) * (POST / 2.0 - 0.06) + Vec3::Y * (low + high) / 2.0;
                        let trim = builder.colours.trim;
                        let mesh = builder.mesh(Surface::Wood, Detail::Full, upper);
                        mesh.block(
                            centre,
                            facing(along),
                            Vec3::new(POST, high - low, POST),
                            [0.0; 8],
                            [0.0, low],
                            trim,
                        );
                    }
                }
            }
            // Inside: plaster, from the floor to the ceiling.
            let (inside_a, inside_b) = (builder.footprint.inside[i], builder.footprint.inside[(i + 1) % count]);
            let shift = (inside_a - a).dot(way);
            let inside_length = inside_a.distance(inside_b);
            let inside_holes: Vec<Hole> = holes.iter().map(|&(f, t, b, tp)| (f - shift, t - shift, b, tp)).collect();
            let inside_start = builder.point(inside_a, 0.0);
            let floor_low = level as f32 * storey;
            let mesh = builder.mesh(Surface::Plaster, Detail::Full, upper);
            face(mesh, inside_start, along, -out, (inside_length, floor_low, high), &inside_holes, 0.0, plaster);
        }

        // The top of the wall, under the roof.
        let (inside_a, inside_b) = (builder.footprint.inside[i], builder.footprint.inside[(i + 1) % count]);
        let top = [
            builder.point(a, eaves),
            builder.point(inside_a, eaves),
            builder.point(inside_b, eaves),
            builder.point(b, eaves),
        ];
        let top_upper = builder.floors > 1;
        let mesh = builder.mesh(wall_surface, Detail::Full, top_upper);
        quad_facing(mesh, top, [[0.0; 2]; 4], walls, Vec3::Y);
        // A timber wall plate along it, for the roof to sit on.
        let middle = builder.point((a + b + inside_a + inside_b) / 4.0, eaves + PLATE / 2.0);
        let trim = builder.colours.trim;
        let size = Vec3::new(builder.style.wall_thickness * 0.8, PLATE, length);
        builder.mesh(Surface::Wood, Detail::Full, top_upper).block(
            middle,
            facing(along),
            size,
            [0.0; 8],
            [0.0, 0.0],
            trim,
        );
        u += length;
    }
}

/// The stretches from `from` to `to` along a wall that are clear of
/// `holes` between `low` and `high` up it.
pub fn free_between((from, to): (f32, f32), (low, high): (f32, f32), holes: &[Hole]) -> Vec<(f32, f32)> {
    let mut free = vec![(from, to)];
    for &(f, t, bottom, top) in holes {
        if top <= low || bottom >= high {
            continue;
        }
        free = free
            .into_iter()
            .flat_map(|(a, b)| if t <= a || f >= b { vec![(a, b)] } else { vec![(a, f), (t, b)] })
            .filter(|(a, b)| b - a > 0.02)
            .collect();
    }
    free
}

/// The turn that points a block's own z along `along` (a horizontal direction).
pub fn facing(along: Vec3) -> Quat {
    Quat::from_rotation_y(along.x.atan2(along.z))
}

/// Lays one course of stones along a wall's face, filling what's clear.
fn lay_course(
    builder: &mut Builder,
    (start, along, out): (Vec3, Vec3, Vec3),
    span: (f32, f32),
    (y0, y1): (f32, f32),
    kept_clear: &[Hole],
    upper: bool,
) {
    let walls = builder.colours.walls;
    let height = y1 - y0;
    for (from, to) in free_between(span, (y0, y1), kept_clear) {
        let mut x = from;
        let mut length = builder.random.range(0.18, 0.5);
        while to - x > 0.03 {
            if to - x - length < 0.16 {
                length = to - x;
            }
            let middle = x + length / 2.0;
            let depth = builder.random.range(0.14, 0.22);
            let proud = builder.random.range(0.0, 0.025);
            let centre = start + along * middle + out * (proud - depth / 2.0) + Vec3::Y * (y0 + height / 2.0);
            let wobble = Quat::from_euler(
                glam::EulerRot::XYZ,
                builder.random.signed() * 0.04,
                builder.random.signed() * 0.05,
                builder.random.signed() * 0.04,
            );
            let size = Vec3::new(depth, height * 0.93, length * 0.96);
            let lumps = std::array::from_fn(|_| builder.random.signed() * 0.08);
            let shade = builder.random.range(0.72, 1.06);
            let colour = [walls[0] * shade, walls[1] * shade * builder.random.range(0.97, 1.02), walls[2] * shade, 1.0];
            let uv = [builder.random.range(0.0, 4.0), builder.random.range(0.0, 4.0)];
            builder.mesh(Surface::Stone, Detail::Full, upper).block(
                centre,
                facing(along) * wobble,
                size,
                lumps,
                uv,
                colour,
            );
            x += length;
            length = builder.random.range(0.22, 0.52);
        }
    }
}

/// A quoin at corner `corner` (where outer wall `corner` starts), course `k`:
/// long along one wall and short along the other, alternating.
fn quoin(builder: &mut Builder, corner: usize, k: usize, (y0, y1): (f32, f32), upper: bool) {
    let count = builder.footprint.outline.len();
    let before = builder.footprint.outline[(corner + count - 1) % count];
    let (here, after) = (builder.footprint.outline[corner], builder.footprint.outline[(corner + 1) % count]);
    let next = (after - here).normalize_or(Vec2::X);
    let back = (before - here).normalize_or(Vec2::X);
    let (long, short) = if k.is_multiple_of(2) { (next, back) } else { (back, next) };
    let size = Vec2::new(QUOIN.0, QUOIN.1) + QUOIN_PROUD;
    let middle = here + long * (size.x / 2.0 - QUOIN_PROUD) + short * (size.y / 2.0 - QUOIN_PROUD);
    let centre = builder.point(middle, (y0 + y1) / 2.0);
    let lumps = std::array::from_fn(|_| builder.random.signed() * 0.05);
    let shade = builder.random.range(0.85, 1.08);
    let colour = builder.colours.walls.map(|c| c * shade);
    let rotation = facing(builder.direction(long));
    let uv = [builder.random.range(0.0, 4.0), builder.random.range(0.0, 4.0)];
    builder.mesh(Surface::Stone, Detail::Full, upper).block(
        centre,
        rotation,
        Vec3::new(size.y, (y1 - y0) * 0.95, size.x),
        lumps,
        uv,
        [colour[0], colour[1], colour[2], 1.0],
    );
}
