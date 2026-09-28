//! Roofs: a gable or hip over each wing of a building (its ridge along the
//! wing's long side), thick thatch with a rolled ridge, gable walls under
//! the gables, and the chimney.
//!
//! Where a wing meets another, its roof reaches on into the other's (to
//! its middle), so the two roofs cross and make the valley between them.
//! Every plane of a building's roof has the same pitch, so the thatch's top
//! is its underside raised straight up by `thickness / cos(pitch)`: that's
//! the plane moved out along its normal, and neighbouring planes still meet
//! along their ridges and hips.

use glam::{Vec2, Vec3};
use venture_world::RoofShape;

use super::faces::{facing, free_between};
use super::plan::{Rect, wings};
use super::{Builder, Detail, Surface, WallConstruction, quad_facing};

/// How thick a ridge's roll of thatch is (its radius), and a hip's, in metres.
const RIDGE_ROLL: f32 = 0.2;
const HIP_ROLL: f32 = 0.14;
/// How many sides a roll has.
const ROLL_SIDES: usize = 8;
/// Thatch's texture runs this many metres down a slope for each metre of the picture.
const THATCH_SCALE: f32 = 1.0;

/// A wing, in its own terms: `s` along its ridge and `t` across it.
#[derive(Clone, Copy, Debug)]
struct Wing {
    rect: Rect,
    /// Whether its ridge runs along the building's x.
    along_x: bool,
    /// Where it starts and ends along its ridge, and across.
    s: (f32, f32),
    t: (f32, f32),
}

impl Wing {
    fn of(rect: Rect) -> Self {
        let along_x = rect.size().x >= rect.size().y;
        let (s, t) = if along_x {
            ((rect.low.x, rect.high.x), (rect.low.y, rect.high.y))
        } else {
            ((rect.low.y, rect.high.y), (rect.low.x, rect.high.x))
        };
        Self { rect, along_x, s, t }
    }

    /// A point of the building's frame from `s` and `t`.
    fn frame(&self, s: f32, t: f32) -> Vec2 {
        if self.along_x { Vec2::new(s, t) } else { Vec2::new(t, s) }
    }

    fn middle(&self) -> f32 {
        (self.t.0 + self.t.1) / 2.0
    }

    fn half(&self) -> f32 {
        (self.t.1 - self.t.0) / 2.0
    }
}

/// How a wing's end finishes: in a gable (or hip) at an outer wall, or
/// reaching on into another wing by this much.
#[derive(Clone, Copy, Debug, PartialEq)]
enum End {
    Outside,
    Into(f32),
}

/// Every wing's roof, the gable walls under them, and the chimney (if wanted).
pub fn roofs(builder: &mut Builder, shape: RoofShape, chimney: bool) {
    let (low, high) = builder.style.roof.pitch;
    let pitch = builder.random.range(low, high.max(low)).to_radians();
    let mut wings: Vec<Wing> = wings(&builder.footprint.outline).into_iter().map(Wing::of).collect();
    // The biggest first (it gets the chimney).
    wings.sort_by(|a, b| b.rect.area().total_cmp(&a.rect.area()));
    for (k, wing) in wings.iter().enumerate() {
        let ends = [end(builder, wing, false), end(builder, wing, true)];
        match shape {
            RoofShape::Gable => gable_roof(builder, wing, ends, pitch),
            RoofShape::Hip => hip_roof(builder, wing, ends, pitch),
        }
        if k == 0 && chimney {
            add_chimney(builder, wing, ends, pitch, shape);
        }
    }
}

/// How a wing's end finishes (`high`: its far end along the ridge).
fn end(builder: &Builder, wing: &Wing, high: bool) -> End {
    let s = if high { wing.s.1 } else { wing.s.0 };
    let middle = wing.frame(s, wing.middle());
    let outline = &builder.footprint.outline;
    let on_outline = (0..outline.len()).any(|i| {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        let run = b - a;
        let t = ((middle - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
        (a + run * t).distance(middle) < 1e-2
    });
    if on_outline {
        return End::Outside;
    }
    // How far the building goes on past this end, along the ridge.
    let onward = if high { 1.0 } else { -1.0 };
    let mut depth = 0.0;
    while depth < 60.0 && builder.footprint.contains(wing.frame(s + onward * (depth + 0.25), wing.middle())) {
        depth += 0.25;
    }
    End::Into(depth / 2.0)
}

/// Where a wing's roof reaches along its ridge: past its ends by the verge
/// (or `eaves`, for a hip), or on into its neighbours.
fn reach(wing: &Wing, ends: [End; 2], outside: f32) -> (f32, f32) {
    let by = |end: End| match end {
        End::Outside => outside,
        End::Into(depth) => depth,
    };
    (wing.s.0 - by(ends[0]), wing.s.1 + by(ends[1]))
}

/// Everything about a building's roof that's the same for each plane.
struct Slope {
    /// The eaves' height.
    eaves: f32,
    /// tan and cos of the pitch.
    tan: f32,
    cos: f32,
    /// The thatch's thickness.
    thickness: f32,
}

impl Slope {
    fn new(builder: &Builder, pitch: f32) -> Self {
        Self { eaves: builder.eaves(), tan: pitch.tan(), cos: pitch.cos(), thickness: builder.style.roof.thickness }
    }

    /// How far the thatch's top is above its underside.
    fn lift(&self) -> f32 {
        self.thickness / self.cos
    }

    /// The underside's height `inward` metres in from the wall's face (negative: out past it).
    fn height(&self, inward: f32) -> f32 {
        self.eaves + inward * self.tan
    }
}

/// A plane of thatch: its underside's corners (building space, anticlockwise
/// from above), which of its edges are open (eaves and verges, not shared
/// with another plane), and how its texture's laid (along its eaves, and
/// down from its top).
fn thatch(builder: &mut Builder, slope: &Slope, corners: &[Vec3], open: &[bool], downhill: Vec3) {
    let lift = Vec3::Y * slope.lift();
    let roof = builder.colours.roof;
    let under = roof.map(|c| c * 0.45);
    let across = downhill.cross(Vec3::Y).normalize_or(Vec3::X);
    let top_height = corners.iter().map(|c| c.y).fold(f32::MIN, f32::max);
    let uv =
        |p: Vec3| [p.dot(across) / THATCH_SCALE, (top_height - p.y) / (slope.tan.max(0.1) * slope.cos) / THATCH_SCALE];
    // The top, and the underside, as a fan (every plane is convex).
    for detail in [Detail::Full, Detail::Far] {
        let upper = detail == Detail::Full;
        let mesh = builder.mesh(Surface::Thatch, detail, upper);
        let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize_or(Vec3::Y);
        let normal = if normal.y < 0.0 { -normal } else { normal };
        for k in 1..corners.len() - 1 {
            let (a, b, c) = (corners[0] + lift, corners[k] + lift, corners[k + 1] + lift);
            quad_facing(mesh, [a, b, c, c], [uv(a), uv(b), uv(c), uv(c)], roof, normal);
            if detail == Detail::Full {
                let (a, b, c) = (corners[0], corners[k], corners[k + 1]);
                quad_facing(mesh, [a, b, c, c], [uv(a), uv(b), uv(c), uv(c)], under, -normal);
            }
        }
        // Its open edges: the thatch's cut ends.
        let centre = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
        for i in 0..corners.len() {
            if !open[i] {
                continue;
            }
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let middle = (a + b) / 2.0;
            let outward = Vec3::new(middle.x - centre.x, 0.0, middle.z - centre.z);
            let length = a.distance(b);
            quad_facing(
                mesh,
                [a, b, b + lift, a + lift],
                [[0.0, 0.0], [length, 0.0], [length, 0.35], [0.0, 0.35]],
                roof.map(|c| c * 0.8),
                outward,
            );
        }
    }
}

/// A roll of thatch along a ridge or hip, from `a` to `b` (on the thatch's top).
fn roll(builder: &mut Builder, a: Vec3, b: Vec3, radius: f32) {
    let run = b - a;
    if run.length() < 1e-3 {
        return;
    }
    let way = run.normalize();
    let side = way.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = side.cross(way).normalize_or(Vec3::Y);
    let roof = builder.colours.roof.map(|c| c * 1.05);
    let length = run.length();
    for detail in [Detail::Full, Detail::Far] {
        let upper = detail == Detail::Full;
        let mesh = builder.mesh(Surface::Thatch, detail, upper);
        let ring = |k: usize| {
            let angle = k as f32 / ROLL_SIDES as f32 * std::f32::consts::TAU;
            side * angle.cos() * radius + up * angle.sin() * radius
        };
        for k in 0..ROLL_SIDES {
            let (p, q) = (ring(k), ring(k + 1));
            let outward = (p + q) / 2.0;
            let u = |k: usize| k as f32 / ROLL_SIDES as f32;
            quad_facing(
                mesh,
                [a + p, b + p, b + q, a + q],
                [[0.0, u(k)], [length, u(k)], [length, u(k + 1)], [0.0, u(k + 1)]],
                roof,
                outward,
            );
            // Its ends.
            quad_facing(mesh, [a, a + p, a + q, a + q], [[0.0; 2]; 4], roof, -way);
            quad_facing(mesh, [b, b + q, b + p, b + p], [[0.0; 2]; 4], roof, way);
        }
    }
}

/// A gable roof over a wing: two slopes, a rolled ridge, and gable walls at its outer ends.
fn gable_roof(builder: &mut Builder, wing: &Wing, ends: [End; 2], pitch: f32) {
    let slope = Slope::new(builder, pitch);
    let eaves = builder.style.roof.eaves;
    let (from, to) = reach(wing, ends, builder.style.roof.verge);
    let (middle, half) = (wing.middle(), wing.half());
    let ridge = slope.height(half);
    let frame = builder.footprint.frame;
    let point = |s: f32, t: f32, y: f32| frame.point(wing.frame(s, t), y);
    for side in [-1.0f32, 1.0] {
        let t_eave = middle + side * (half + eaves);
        let y_eave = slope.height(-eaves);
        let corners = [
            point(from, t_eave, y_eave),
            point(to, t_eave, y_eave),
            point(to, middle, ridge),
            point(from, middle, ridge),
        ];
        let downhill = builder.direction(wing.frame(0.0, side));
        thatch(builder, &slope, &corners, &[true, true, false, true], downhill);
    }
    let lift = slope.lift();
    let (a, b) = (point(from, middle, ridge + lift), point(to, middle, ridge + lift));
    roll(builder, a, b, RIDGE_ROLL);
    for (k, end) in ends.iter().enumerate() {
        if *end == End::Outside {
            gable_wall(builder, wing, k == 1, &slope);
        }
    }
}

/// A hip roof over a wing: sloping at its outer ends too (where it meets
/// another wing, it reaches on into it instead, as a gable does).
fn hip_roof(builder: &mut Builder, wing: &Wing, ends: [End; 2], pitch: f32) {
    let slope = Slope::new(builder, pitch);
    let eaves = builder.style.roof.eaves;
    let (middle, half) = (wing.middle(), wing.half());
    let ridge = slope.height(half);
    let y_eave = slope.height(-eaves);
    let frame = builder.footprint.frame;
    let point = |s: f32, t: f32, y: f32| frame.point(wing.frame(s, t), y);
    // Where the eaves reach along the ridge, and where the ridge starts and ends.
    let (from, to) = reach(wing, ends, eaves);
    let hipped = [ends[0] == End::Outside, ends[1] == End::Outside];
    let mut ridge_from = if hipped[0] { wing.s.0 + half } else { from };
    let mut ridge_to = if hipped[1] { wing.s.1 - half } else { to };
    if ridge_from > ridge_to {
        let centre = (ridge_from + ridge_to) / 2.0;
        (ridge_from, ridge_to) = (centre, centre);
    }
    for side in [-1.0f32, 1.0] {
        let t_eave = middle + side * (half + eaves);
        let corners = [
            point(from, t_eave, y_eave),
            point(to, t_eave, y_eave),
            point(ridge_to, middle, ridge),
            point(ridge_from, middle, ridge),
        ];
        let open = [true, !hipped[1], false, !hipped[0]];
        let downhill = builder.direction(wing.frame(0.0, side));
        thatch(builder, &slope, &corners, &open, downhill);
    }
    let lift = slope.lift();
    for (k, &hip) in hipped.iter().enumerate() {
        if !hip {
            continue;
        }
        let (s_eave, s_ridge, onward) = if k == 0 { (from, ridge_from, -1.0) } else { (to, ridge_to, 1.0) };
        let (t0, t1) = (middle - half - eaves, middle + half + eaves);
        let corners = [point(s_eave, t1, y_eave), point(s_eave, t0, y_eave), point(s_ridge, middle, ridge)];
        let downhill = builder.direction(wing.frame(onward, 0.0));
        thatch(builder, &slope, &corners, &[true, false, false], downhill);
        let top = point(s_ridge, middle, ridge + lift);
        for t in [t0, t1] {
            roll(builder, point(s_eave, t, y_eave + lift), top, HIP_ROLL);
        }
    }
    roll(builder, point(ridge_from, middle, ridge + lift), point(ridge_to, middle, ridge + lift), RIDGE_ROLL);
}

/// A gable wall: the triangle of wall at a wing's outer end, from the
/// eaves up under the roof, in the building's construction.
fn gable_wall(builder: &mut Builder, wing: &Wing, high: bool, slope: &Slope) {
    let thickness = builder.style.wall_thickness;
    let s = if high { wing.s.1 } else { wing.s.0 };
    let outward_s = if high { 1.0 } else { -1.0 };
    let (middle, half) = (wing.middle(), wing.half());
    let (eaves, ridge) = (slope.eaves, slope.height(half));
    let walls = builder.colours.walls;
    let plaster = builder.colours.plaster;
    let surface = match builder.style.walls {
        WallConstruction::Stone => Surface::Stone,
        WallConstruction::Timber => Surface::Boards,
    };
    // Its outer face, as courses (or boards) narrowing as it rises; its inside, plaster.
    let along = builder.direction(wing.frame(0.0, 1.0));
    let out = builder.direction(wing.frame(outward_s, 0.0));
    let start = builder.point(wing.frame(s, middle - half), 0.0);
    let inside_start = builder.point(wing.frame(s - outward_s * thickness, middle - half), 0.0);
    let width = 2.0 * half;
    let mut y = eaves;
    while y < ridge - 0.02 {
        let height = builder.random.range(0.16, 0.26).min(ridge - y);
        let top = y + height;
        // How far in from each side the roof comes down at this height.
        let inset = (top - eaves) / slope.tan.max(0.1);
        let span = (inset, width - inset);
        if span.1 - span.0 < 0.05 {
            break;
        }
        for (a, b) in free_between(span, (y, top), &[]) {
            match builder.style.walls {
                WallConstruction::Stone => {
                    let mut x = a;
                    while b - x > 0.03 {
                        let mut length = builder.random.range(0.2, 0.5);
                        if b - x - length < 0.15 {
                            length = b - x;
                        }
                        let depth = builder.random.range(0.14, 0.22);
                        let centre = start
                            + along * (x + length / 2.0)
                            + out * (0.01 - depth / 2.0)
                            + Vec3::Y * (y + height / 2.0);
                        let lumps = std::array::from_fn(|_| builder.random.signed() * 0.08);
                        let shade = builder.random.range(0.72, 1.06);
                        let uv = [builder.random.range(0.0, 4.0), builder.random.range(0.0, 4.0)];
                        let colour = [walls[0] * shade, walls[1] * shade, walls[2] * shade, 1.0];
                        builder.mesh(Surface::Stone, Detail::Full, true).block(
                            centre,
                            facing(along),
                            Vec3::new(depth, height * 0.93, length * 0.96),
                            lumps,
                            uv,
                            colour,
                        );
                        x += length;
                    }
                }
                WallConstruction::Timber => {
                    let at = |x: f32, h: f32, depth: f32| start + along * x + Vec3::Y * h + out * depth;
                    let corners = [at(a, y, 0.035), at(b, y, 0.035), at(b, top, 0.0), at(a, top, 0.0)];
                    let uvs = [[a, -y * 3.0], [b, -y * 3.0], [b, -top * 3.0], [a, -top * 3.0]];
                    quad_facing(builder.mesh(Surface::Boards, Detail::Full, true), corners, uvs, walls, out);
                }
            }
        }
        y = top;
    }
    // Behind them, the triangle whole (the mortar, and far away); and inside, plaster.
    let triangle = |from: Vec3, depth: Vec3| {
        [
            from + depth + Vec3::Y * eaves,
            from + depth + along * width + Vec3::Y * eaves,
            from + depth + along * half + Vec3::Y * ridge,
        ]
    };
    let [a, b, c] = triangle(start, -out * 0.03);
    quad_facing(
        builder.mesh(surface, Detail::Full, true),
        [a, b, c, c],
        [[0.0, 0.0], [width, 0.0], [half, -(ridge - eaves)], [half, -(ridge - eaves)]],
        walls.map(|c| c * 0.4),
        out,
    );
    let [a, b, c] = triangle(start, Vec3::ZERO);
    quad_facing(
        builder.mesh(surface, Detail::Far, false),
        [a, b, c, c],
        [[0.0, 0.0], [width, 0.0], [half, -(ridge - eaves)], [half, -(ridge - eaves)]],
        walls,
        out,
    );
    let [a, b, c] = triangle(inside_start, Vec3::ZERO);
    quad_facing(builder.mesh(Surface::Plaster, Detail::Full, true), [a, b, c, c], [[0.0; 2]; 4], plaster, -out);
}

/// A chimney stack of stone, with a cap and pots: in a gable end (inside
/// its wall), or through the ridge of a hip roof.
fn add_chimney(builder: &mut Builder, wing: &Wing, ends: [End; 2], pitch: f32, shape: RoofShape) {
    let slope = Slope::new(builder, pitch);
    let (width, depth) = builder.style.chimney.size;
    let (middle, half) = (wing.middle(), wing.half());
    let ridge = slope.height(half) + slope.lift();
    let outer_ends: Vec<usize> = (0..2).filter(|&k| ends[k] == End::Outside).collect();
    let s = match (shape, outer_ends.as_slice()) {
        (RoofShape::Gable, []) => return,
        (RoofShape::Gable, choices) => {
            let k = choices[(builder.random.next_u64() % choices.len() as u64) as usize];
            if k == 1 { wing.s.1 - depth / 2.0 } else { wing.s.0 + depth / 2.0 }
        }
        (RoofShape::Hip, _) => {
            let reach = (wing.s.1 - wing.s.0) / 2.0 - half;
            (wing.s.0 + wing.s.1) / 2.0 + builder.random.signed() * reach.max(0.0) * 0.6
        }
    };
    let bottom = slope.eaves - 0.4;
    let top = ridge + builder.style.chimney.above_ridge;
    let stone = builder.colours.walls.map(|c| c * 0.95);
    let along = builder.direction(wing.frame(1.0, 0.0));
    let frame = builder.footprint.frame;
    let at = |y: f32| frame.point(wing.frame(s, middle), y);
    let lumps = [0.0; 8];
    for detail in [Detail::Full, Detail::Far] {
        let upper = detail == Detail::Full;
        let mesh = builder.mesh(Surface::Stone, detail, upper);
        mesh.block(
            at((bottom + top) / 2.0),
            facing(along),
            Vec3::new(width, top - bottom, depth),
            lumps,
            [0.0, 0.0],
            stone,
        );
        mesh.block(
            at(top + 0.06),
            facing(along),
            Vec3::new(width + 0.12, 0.12, depth + 0.12),
            lumps,
            [0.5, 0.5],
            stone.map(|c| c * 1.1),
        );
    }
    // Pots: one or two, terracotta, dark inside.
    let pots = 1 + usize::from(builder.random.unit() < 0.5);
    let pot = [0.55, 0.22, 0.12, 1.0];
    for k in 0..pots {
        let offset = if pots == 1 { 0.0 } else { (k as f32 - 0.5) * width * 0.45 };
        let centre = builder.point(wing.frame(s, middle + offset), top + 0.12 + 0.2);
        let mesh = builder.mesh(Surface::Stone, Detail::Full, true);
        mesh.block(centre, facing(along), Vec3::new(0.24, 0.4, 0.24), [0.0; 8], [0.0, 0.0], pot);
        let opening = centre + Vec3::Y * 0.201;
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| opening + builder.direction(wing.frame(x * 0.08, z * 0.08)));
        quad_facing(
            builder.mesh(Surface::Dark, Detail::Full, true),
            corners,
            [[0.0; 2]; 4],
            [0.01, 0.01, 0.01, 1.0],
            Vec3::Y,
        );
    }
}
