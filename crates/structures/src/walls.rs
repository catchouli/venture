//! Building a baked wall's meshes: a dry stone wall's stones, or a fence's
//! posts and rails, laid along its samples (see `venture_world::BakedWall`).
//!
//! A wall is built in **stretches** about [`STRETCH_LENGTH`] metres long, so
//! each stretch can be culled, and switch between its full and far meshes,
//! on its own. Each stretch's meshes are relative to its origin (y up: x
//! east and z the world's y, as in the game).
//!
//! - **Dry stone:** two faces of stones leaning slightly inwards (the
//!   *batter*), laid course by course with each course's joints offset
//!   from the last, every stone a little irregular (its corners pushed in
//!   or out), tilted and shaded; a dark core between the faces, so no
//!   daylight shows through the joints; upright coping stones along the
//!   top; and wherever the wall ends (at a gap, or the end of its path, but
//!   not where a ring comes back round), a *cheek end* of through stones
//!   across its whole width, one a course. Far away it's the same shape as one plain extruded band.
//! - **Fence:** a post at each end and every [`POST_SPACING`] metres or so between, sunk into the
//!   ground and leaning a little, with rails (two, or three on a tall fence)
//!   running between them with the slope. Fences are light enough to be the
//!   same close up and far away.
//!
//! - **Hedge:** a leafy body (textured; lumpy along its length and in its
//!   outline, full at the shoulders, rounded over a clipped top, and rounded
//!   off at its ends), drawn at any distance, with sprigs of leaves (cards
//!   from a tree species' leaf atlas: see `venture_trees::mesh::sprig_cell`)
//!   lying along its shoulders and top to soften its outline, drawn close
//!   up ([`Stretch::leaves`]). Its texture coordinates are metres along it
//!   and round its outline.
//!
//! Where the wall is open (a gap, such as a gateway for a road), nothing is
//! built.

use glam::{Quat, Vec2, Vec3};
use venture_world::{BakedWall, WALL_SAMPLE_SPACING, WallKind, WallSample};

use crate::mesh::{MeshPart, linear};
use crate::random::Random;

/// How long a stretch is, in metres.
pub const STRETCH_LENGTH: f32 = 16.0;
/// A dry stone wall's half-width at its foot and at its top, in metres.
const FOOT_HALF_WIDTH: f32 = 0.36;
const TOP_HALF_WIDTH: f32 = 0.2;
/// How tall its coping stones are, as a share of the wall's height.
const COPING_SHARE: f32 = 0.2;
/// How far below the ground walls start (so they sit in it, not on it), in metres.
const FOOTING: f32 = 0.1;
/// Roughly how far apart a fence's posts are, in metres.
pub const POST_SPACING: f32 = 2.4;
/// How far a fence post is sunk into the ground, in metres.
const POST_SUNK: f32 = 0.45;
/// How far a dry stone wall's end stones stand past the end of its core, in metres.
const END_PROUD: f32 = 0.03;
/// The stones' colour (sRGB, multiplying their texture) and the wood's.
const STONE: [f32; 3] = [0.92, 0.9, 0.86];
const CORE: [f32; 3] = [0.25, 0.24, 0.23];
const WOOD: [f32; 3] = [0.82, 0.78, 0.72];
/// A hedge's half-width at its shoulder, in metres.
const HEDGE_HALF_WIDTH: f32 = 0.6;
/// Its outline, across (a share of its half-width, from the middle) and up
/// (a share of its height; below 0 is into the ground), from one foot over
/// the top to the other: full at the shoulders, rounded over a clipped top.
const HEDGE_OUTLINE: [(f32, f32); 14] = [
    (-0.82, -0.2),
    (-0.9, 0.12),
    (-0.98, 0.4),
    (-1.0, 0.66),
    (-0.94, 0.84),
    (-0.78, 0.95),
    (-0.4, 1.0),
    (0.4, 1.0),
    (0.78, 0.95),
    (0.94, 0.84),
    (1.0, 0.66),
    (0.98, 0.4),
    (0.9, 0.12),
    (0.82, -0.2),
];
/// How far a hedge's rounded end (a dome closing it) reaches past where it
/// stops, in metres, and how many rings it's made of.
const HEDGE_DOME: f32 = 0.55;
const DOME_RINGS: usize = 5;
/// How many sprigs of leaves a metre of hedge has, along its shoulders and top.
const SPRIGS_PER_METRE: f32 = 26.0;
/// How big a sprig's card is, in metres.
const SPRIG_SIZE: f32 = 0.24;

/// A stretch of wall, ready to draw.
#[derive(Clone, Debug, Default)]
pub struct Stretch {
    /// Where its meshes are relative to: (x, height, y) in the world, in metres.
    pub origin: [f32; 3],
    /// Close up.
    pub full: MeshPart,
    /// Far away.
    pub far: MeshPart,
    /// A hedge's leaves (cards from a leaf atlas), close up.
    pub leaves: MeshPart,
}

/// A wall's path, to look things up along it.
struct Path<'a> {
    samples: &'a [WallSample],
}

impl Path<'_> {
    /// How long it is, in metres.
    fn length(&self) -> f32 {
        (self.samples.len().saturating_sub(1)) as f32 * WALL_SAMPLE_SPACING
    }

    /// The sample index `distance` metres along, and the share of the way to the next.
    fn index(&self, distance: f32) -> (usize, f32) {
        let at = (distance / WALL_SAMPLE_SPACING).clamp(0.0, (self.samples.len() - 1) as f32);
        let index = (at.floor() as usize).min(self.samples.len().saturating_sub(2));
        (index, at - index as f32)
    }

    /// Where the path is `distance` metres along, and the ground's height there.
    fn at(&self, distance: f32) -> (Vec2, f32) {
        let (i, t) = self.index(distance);
        let (a, b) = (self.samples[i], self.samples[(i + 1).min(self.samples.len() - 1)]);
        (Vec2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t), a.ground + (b.ground - a.ground) * t)
    }

    /// The way the path runs `distance` metres along (a unit vector).
    fn along(&self, distance: f32) -> Vec2 {
        let (i, _) = self.index(distance);
        let (a, b) = (self.samples[i], self.samples[(i + 1).min(self.samples.len() - 1)]);
        Vec2::new(b.x - a.x, b.y - a.y).normalize_or(Vec2::X)
    }

    /// Whether it comes back round to where it started (a ring).
    fn closed(&self) -> bool {
        let (first, last) = (self.samples[0], self.samples[self.samples.len() - 1]);
        Vec2::new(first.x - last.x, first.y - last.y).length() < WALL_SAMPLE_SPACING * 0.5
    }

    /// The unbroken runs of wall between its gaps and ends: where each
    /// starts and ends (in metres along), and whether each end is a real
    /// end (not where a ring comes back round to its start).
    fn runs(&self) -> Vec<Run> {
        let mut runs = Vec::new();
        let mut first: Option<usize> = None;
        for (i, sample) in self.samples.iter().enumerate() {
            match (sample.open, first) {
                (false, None) => first = Some(i),
                (true, Some(from)) => {
                    if i - 1 > from {
                        runs.push((from, i - 1));
                    }
                    first = None;
                }
                _ => {}
            }
        }
        if let Some(from) = first
            && self.samples.len() - 1 > from
        {
            runs.push((from, self.samples.len() - 1));
        }
        let last = self.samples.len() - 1;
        let closed = self.closed();
        runs.into_iter()
            .map(|(from, to)| Run {
                start: from as f32 * WALL_SAMPLE_SPACING,
                end: to as f32 * WALL_SAMPLE_SPACING,
                capped_start: !(closed && from == 0),
                capped_end: !(closed && to == last),
            })
            .collect()
    }
}

/// An unbroken run of wall.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    /// Where it starts and ends, in metres along the path.
    start: f32,
    end: f32,
    /// Whether it ends there (and so needs finishing off), rather than
    /// running on round a ring.
    capped_start: bool,
    capped_end: bool,
}

/// Collects pieces into stretches, relative to each stretch's origin.
struct Builder {
    stretches: Vec<Stretch>,
}

impl Builder {
    fn new(path: &Path) -> Self {
        let count = ((path.length() / STRETCH_LENGTH).ceil() as usize).max(1);
        let stretches = (0..count)
            .map(|i| {
                let (at, ground) = path.at(i as f32 * STRETCH_LENGTH);
                Stretch { origin: [at.x, ground, at.y], ..Stretch::default() }
            })
            .collect();
        Self { stretches }
    }

    /// The stretch a piece `distance` metres along belongs to, and where
    /// `world` (x, height, y) is relative to its origin (as x, up, z).
    fn place(&mut self, distance: f32, world: Vec3) -> (&mut Stretch, Vec3) {
        let index = ((distance / STRETCH_LENGTH) as usize).min(self.stretches.len() - 1);
        let stretch = &mut self.stretches[index];
        let [ox, oh, oy] = stretch.origin;
        (stretch, Vec3::new(world.x - ox, world.y - oh, world.z - oy))
    }
}

/// A 3D point from a place on the ground plan and a height.
fn lift(at: Vec2, height: f32) -> Vec3 {
    Vec3::new(at.x, height, at.y)
}

/// Builds a wall's stretches.
pub fn build_wall(wall: &BakedWall) -> Vec<Stretch> {
    if wall.samples.len() < 2 {
        return Vec::new();
    }
    let path = Path { samples: &wall.samples };
    let mut builder = Builder::new(&path);
    let mut random = Random::new(wall.seed);
    match wall.kind {
        WallKind::DryStone => dry_stone(&path, wall.height, &mut builder, &mut random),
        WallKind::Fence => fence(&path, wall.height, &mut builder, &mut random),
        WallKind::Hedge => hedge(&path, wall.height, &mut builder, &mut random),
    }
    builder
        .stretches
        .retain(|stretch| !stretch.full.is_empty() || !stretch.far.is_empty() || !stretch.leaves.is_empty());
    builder.stretches
}

/// A face's half-width `height` metres up a dry stone wall's body of `body` metres.
fn half_width(height: f32, body: f32) -> f32 {
    let share = (height / body.max(0.1)).clamp(0.0, 1.0);
    FOOT_HALF_WIDTH + (TOP_HALF_WIDTH - FOOT_HALF_WIDTH) * share
}

/// The turn that points a block's own z along `along`, and its y up.
fn facing(along: Vec2) -> Quat {
    Quat::from_rotation_y(along.x.atan2(along.y))
}

fn dry_stone(path: &Path, height: f32, builder: &mut Builder, random: &mut Random) {
    let coping = height * COPING_SHARE;
    let body = height - coping;
    let batter = (FOOT_HALF_WIDTH - TOP_HALF_WIDTH).atan2(body);

    let runs = path.runs();

    // Course by course (the same courses both sides, so the ends can bind them).
    let mut bottom = -FOOTING;
    while bottom < body {
        let course = random.range(0.11, 0.19).min(body - bottom + 0.02);
        let up = bottom + course / 2.0;
        for run in &runs {
            // Each end is finished with a through stone across the whole
            // wall (a "cheek end"), hiding the core, alternately long and short
            // so the courses bond.
            let mut from = run.start;
            let mut to = run.end;
            for (capped, at, inwards) in [(run.capped_start, run.start, 1.0f32), (run.capped_end, run.end, -1.0)] {
                if !capped || run.end - run.start < 0.8 {
                    continue;
                }
                let stone_length = random.range(0.3, 0.5);
                // Standing a little proud of the end, so its lumps never uncover the core.
                let middle = at + inwards * (stone_length / 2.0 - END_PROUD);
                let (place, ground) = path.at(middle);
                let along = path.along(middle);
                let centre = lift(place, ground + up);
                let wobble =
                    Quat::from_rotation_x(random.signed() * 0.04) * Quat::from_rotation_z(random.signed() * 0.04);
                let size = Vec3::new(half_width(up, body) * 2.0 + 0.04, course * 0.92, stone_length);
                stone(builder, random, middle, centre, facing(along) * wobble, size);
                if inwards > 0.0 {
                    from = at + stone_length - END_PROUD;
                } else {
                    to = at - stone_length + END_PROUD;
                }
            }
            // The faces between, filled exactly to the ends.
            for side in [-1.0f32, 1.0] {
                // Each course's joints fall somewhere different.
                let mut distance = from;
                let mut stone_length = random.range(0.1, 0.48);
                while to - distance > 0.02 {
                    if to - distance - stone_length < 0.18 {
                        stone_length = to - distance;
                    }
                    let middle = distance + stone_length / 2.0;
                    let (at, ground) = path.at(middle);
                    let along = path.along(middle);
                    let right = Vec2::new(along.y, -along.x);
                    let depth = random.range(0.2, 0.32);
                    let out = half_width(up, body) - depth / 2.0;
                    let centre = lift(at + right * side * out, ground + up);
                    let wobble = Quat::from_euler(
                        glam::EulerRot::XYZ,
                        random.signed() * 0.06,
                        random.signed() * 0.08,
                        random.signed() * 0.06,
                    );
                    let rotation = facing(along) * Quat::from_rotation_z(side * batter) * wobble;
                    let size = Vec3::new(depth, course * 0.92, stone_length * 0.95);
                    stone(builder, random, middle, centre, rotation, size);
                    distance += stone_length;
                    stone_length = random.range(0.22, 0.48);
                }
            }
        }
        bottom += course;
    }

    // The core between the faces, and (far away) the whole wall, as bands along the path.
    band(path, builder, |_| -FOOTING, |_| body, TOP_HALF_WIDTH * 0.8, FOOT_HALF_WIDTH * 0.7, CORE, true);
    band(path, builder, |_| -FOOTING, |_| height, TOP_HALF_WIDTH, FOOT_HALF_WIDTH, STONE, false);

    // Coping stones on edge along the top, end to end along each run.
    for run in &runs {
        let mut distance = run.start + random.range(0.0, 0.03);
        loop {
            let thickness = random.range(0.09, 0.15);
            if distance + thickness > run.end + END_PROUD {
                break;
            }
            let middle = distance + thickness / 2.0;
            let (at, ground) = path.at(middle);
            let along = path.along(middle);
            let stone_height = coping * random.range(0.85, 1.15);
            let centre = lift(at, ground + body + stone_height / 2.0 - 0.03);
            let lean = Quat::from_rotation_x(random.signed() * 0.12) * Quat::from_rotation_z(random.signed() * 0.05);
            let size = Vec3::new(TOP_HALF_WIDTH * 2.0 + random.range(0.0, 0.08), stone_height, thickness);
            let lumps = std::array::from_fn(|_| random.signed() * 0.1);
            let shade = random.range(0.7, 1.0);
            let uv = [random.range(0.0, 4.0), random.range(0.0, 4.0)];
            let (stretch, local) = builder.place(middle, centre);
            stretch.full.block(local, facing(along) * lean, size, lumps, uv, linear(STONE.map(|c| c * shade)));
            distance += thickness + random.range(0.01, 0.03);
        }
    }
}

/// Adds a stone: a lumpy block, shaded a little differently from the next.
fn stone(builder: &mut Builder, random: &mut Random, distance: f32, centre: Vec3, rotation: Quat, size: Vec3) {
    let lumps = std::array::from_fn(|_| random.signed() * 0.12);
    let shade = random.range(0.72, 1.05);
    let tint = [STONE[0] * shade, STONE[1] * shade * random.range(0.97, 1.02), STONE[2] * shade];
    let uv = [random.range(0.0, 4.0), random.range(0.0, 4.0)];
    let (stretch, local) = builder.place(distance, centre);
    stretch.full.block(local, rotation, size, lumps, uv, linear(tint));
}

/// Adds a band along the path, from `bottom` to `top` (metres above the
/// ground at each point), `top_half` wide at the top and `foot_half` at
/// the foot, either into the full meshes (`core`, the hidden fill) or the
/// far ones (the whole wall). It stops at gaps, with the ends closed.
#[allow(clippy::too_many_arguments)]
fn band(
    path: &Path,
    builder: &mut Builder,
    bottom: impl Fn(f32) -> f32,
    top: impl Fn(f32) -> f32,
    top_half: f32,
    foot_half: f32,
    colour: [f32; 3],
    core: bool,
) {
    let colour = linear(colour);
    let count = path.samples.len();
    for i in 0..count - 1 {
        let (a, b) = (path.samples[i], path.samples[i + 1]);
        if a.open || b.open {
            continue;
        }
        let (da, db) = (i as f32 * WALL_SAMPLE_SPACING, (i + 1) as f32 * WALL_SAMPLE_SPACING);
        let along = Vec2::new(b.x - a.x, b.y - a.y).normalize_or(Vec2::X);
        let right = Vec2::new(along.y, -along.x);
        let corner = |sample: WallSample, distance: f32, side: f32, upper: bool| {
            let half = if upper { top_half } else { foot_half };
            let height = sample.ground + if upper { top(distance) } else { bottom(distance) };
            lift(Vec2::new(sample.x, sample.y) + right * side * half, height)
        };
        let (stretch, _) = builder.place((da + db) / 2.0, Vec3::ZERO);
        let origin = Vec3::from(stretch.origin);
        let relative = |p: Vec3| Vec3::new(p.x - origin.x, p.y - origin.y, p.z - origin.z);
        let mesh = if core { &mut stretch.full } else { &mut stretch.far };
        let tall = top(da) - bottom(da);
        // Left face, top, right face (anticlockwise from outside).
        let faces = [
            [
                corner(a, da, -1.0, false),
                corner(b, db, -1.0, false),
                corner(b, db, -1.0, true),
                corner(a, da, -1.0, true),
            ],
            [corner(a, da, -1.0, true), corner(b, db, -1.0, true), corner(b, db, 1.0, true), corner(a, da, 1.0, true)],
            [corner(b, db, 1.0, false), corner(a, da, 1.0, false), corner(a, da, 1.0, true), corner(b, db, 1.0, true)],
        ];
        for face in faces {
            let uvs = [[da, tall], [db, tall], [db, 0.0], [da, 0.0]];
            mesh.quad(face.map(relative), uvs, colour);
        }
        // Ends, where the wall stops (its first or last sample, or a gap).
        let starts = i == 0 || path.samples[i - 1].open;
        let ends = i + 2 == count || path.samples[i + 2].open;
        if starts {
            let end = [
                corner(a, da, 1.0, false),
                corner(a, da, -1.0, false),
                corner(a, da, -1.0, true),
                corner(a, da, 1.0, true),
            ];
            mesh.quad(end.map(relative), [[0.0, tall], [1.0, tall], [1.0, 0.0], [0.0, 0.0]], colour);
        }
        if ends {
            let end = [
                corner(b, db, -1.0, false),
                corner(b, db, 1.0, false),
                corner(b, db, 1.0, true),
                corner(b, db, -1.0, true),
            ];
            mesh.quad(end.map(relative), [[0.0, tall], [1.0, tall], [1.0, 0.0], [0.0, 0.0]], colour);
        }
    }
}

fn fence(path: &Path, height: f32, builder: &mut Builder, random: &mut Random) {
    let rails: &[f32] = if height > 1.15 { &[0.3, 0.6, 0.9] } else { &[0.35, 0.82] };
    for run in path.runs() {
        // Posts at each end of the run (so each side of a gap), and evenly
        // between, give or take. Where a ring comes back round, its end is
        // its start's post, so there's no second post there.
        let spans = ((run.end - run.start) / POST_SPACING).round().max(1.0);
        let spacing = (run.end - run.start) / spans;
        let mut posts = vec![run.start];
        posts.extend((1..spans as usize).map(|i| run.start + (i as f32 + random.signed() * 0.08) * spacing));
        posts.push(run.end);
        let drawn = if run.capped_end { posts.len() } else { posts.len() - 1 };
        for &at in &posts[..drawn] {
            let (place, ground) = path.at(at);
            let along = path.along(at);
            let post_height = (height + POST_SUNK) * random.range(0.96, 1.04);
            let centre = lift(place, ground - POST_SUNK + post_height / 2.0);
            let lean = Quat::from_rotation_x(random.signed() * 0.035) * Quat::from_rotation_z(random.signed() * 0.035);
            let size = Vec3::new(random.range(0.11, 0.14), post_height, random.range(0.11, 0.14));
            wood(builder, random, at, centre, facing(along) * lean, size);
        }

        // Rails from post to post, nailed to one side.
        for pair in posts.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let ((a, ground_a), (b, ground_b)) = (path.at(from), path.at(to));
            let side = path.along(from).perp() * -0.075;
            for &share in rails {
                let rise = height * share + random.signed() * 0.02;
                let start = lift(a + side, ground_a + rise);
                let end = lift(b + side, ground_b + rise);
                let run = end - start;
                let rotation = Quat::from_rotation_arc(Vec3::Z, run.normalize_or(Vec3::Z))
                    * Quat::from_rotation_z(random.signed() * 0.04);
                let size = Vec3::new(0.05, random.range(0.09, 0.12), run.length() + 0.12);
                wood(builder, random, (from + to) / 2.0, (start + end) / 2.0, rotation, size);
            }
        }
    }
}

fn hedge(path: &Path, height: f32, builder: &mut Builder, random: &mut Random) {
    // Each point of the outline wobbles in and out along the hedge on its own
    // (smooth lumps a metre or two long), and the top rises and falls a little.
    let phases: [f32; 8] = std::array::from_fn(|_| random.range(0.0, std::f32::consts::TAU));
    let wobble = |distance: f32, point: usize| {
        let phase = |k: usize| phases[(point + k) % phases.len()] + point as f32 * 0.9;
        (distance * 1.3 + phase(0)).sin() * 0.5
            + (distance * 2.9 + phase(3)).sin() * 0.3
            + (distance * 0.55 + phase(5)).sin() * 0.2
    };
    let top = |distance: f32| height * (1.0 + (distance * 0.9 + phases[7]).sin() * 0.03);

    for run in path.runs() {
        // The outline `distance` along: each point's place, its normal, and how
        // far round the outline it is. Past an end (`past` metres on, of the
        // dome's `HEDGE_DOME` radius) the outline pulls in and down to close it.
        let ring = |distance: f32, past: f32| {
            let (at, ground) = path.at(distance);
            let along = path.along(distance);
            let right = Vec2::new(along.y, -along.x);
            let right3 = Vec3::new(right.x, 0.0, right.y);
            let onward = Vec3::new(along.x, 0.0, along.y) * past.signum();
            // How far round the dome (0 where the hedge ends, a right angle where it closes).
            let angle = (past.abs() / HEDGE_DOME).clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
            let (pull, lean) = (angle.cos(), angle.sin());
            let tall = top(distance);
            let points: Vec<(f32, f32)> = HEDGE_OUTLINE
                .iter()
                .enumerate()
                .map(|(i, &(across, up))| {
                    let lump = 1.0 + 0.09 * wobble(distance, i);
                    let x = across * HEDGE_HALF_WIDTH * lump * pull;
                    // (The feet stay in the ground; the rest comes down as it closes.)
                    let y = if up < 0.0 {
                        up * height
                    } else {
                        up * tall * (1.0 + 0.03 * wobble(distance, i + 4)) * (0.35 + 0.65 * pull)
                    };
                    (x, y)
                })
                .collect();
            let mut around = 0.0;
            points
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| {
                    if i > 0 {
                        let (px, py) = points[i - 1];
                        around += (x - px).hypot(y - py);
                    }
                    // The outline's normal (out from it), softened towards out from
                    // the middle, and turned onwards round the dome.
                    let (bx, by) = points[i.saturating_sub(1)];
                    let (ax, ay) = points[(i + 1).min(points.len() - 1)];
                    let (tx, ty) = (ax - bx, ay - by);
                    let flat = Vec2::new(-ty, tx).normalize_or(Vec2::Y);
                    let soft =
                        (flat * 0.7 + Vec2::new(x, y - tall * 0.45).normalize_or(Vec2::Y) * 0.3).normalize_or(Vec2::Y);
                    let side = (right3 * soft.x + Vec3::Y * soft.y).normalize_or(Vec3::Y);
                    let normal = (side * pull + onward * lean).normalize_or(Vec3::Y);
                    let place = lift(at + right * x, ground + y) + onward * HEDGE_DOME * lean * 0.8;
                    (place, normal, around)
                })
                .collect::<Vec<_>>()
        };

        // Its rings, end to end: a dome closing each end (unless it runs on
        // round a ring), and one a sample along between.
        let steps = ((run.end - run.start) / WALL_SAMPLE_SPACING).round().max(1.0) as usize;
        let mut rings: Vec<(f32, f32)> = Vec::new();
        let dome = |from_end: usize| HEDGE_DOME * (DOME_RINGS - from_end) as f32 / DOME_RINGS as f32;
        if run.capped_start {
            rings.extend((0..DOME_RINGS).map(|k| (run.start, -dome(k))));
        }
        rings.extend((0..=steps).map(|step| (run.start + (run.end - run.start) * step as f32 / steps as f32, 0.0)));
        if run.capped_end {
            rings.extend((0..DOME_RINGS).rev().map(|k| (run.end, dome(k))));
        }
        // Each ring's points, with their texture's `u`: metres along the hedge,
        // and round a dome, the distance over its surface from where the
        // hedge ends (so the leaves keep their size as it curves round), with
        // `around` measured from the top (so rings that shrink do so evenly).
        let mut built: Vec<Vec<(Vec3, Vec3, f32, f32)>> = rings
            .iter()
            .map(|&(distance, past)| {
                let points = ring(distance, past);
                let middle = points.last().map_or(0.0, |point| point.2) / 2.0;
                points.into_iter().map(|(place, normal, around)| (place, normal, around - middle, distance)).collect()
            })
            .collect();
        let first_body = if run.capped_start { DOME_RINGS } else { 0 };
        let last_body = built.len() - 1 - if run.capped_end { DOME_RINGS } else { 0 };
        for k in (0..first_body).rev() {
            for i in 0..built[k].len() {
                let (next, here) = (built[k + 1][i], built[k][i]);
                built[k][i].3 = next.3 - here.0.distance(next.0);
            }
        }
        for k in last_body + 1..built.len() {
            for i in 0..built[k].len() {
                let (before, here) = (built[k - 1][i], built[k][i]);
                built[k][i].3 = before.3 + here.0.distance(before.0);
            }
        }
        for (k, pair) in built.windows(2).enumerate() {
            let (previous, next) = (&pair[0], &pair[1]);
            let (stretch, _) = builder.place((rings[k].0 + rings[k + 1].0) / 2.0, Vec3::ZERO);
            let origin = Vec3::from(stretch.origin);
            let body = &mut stretch.full;
            for i in 0..previous.len() - 1 {
                let first = body.positions.len() as u32;
                for (place, normal, around, u) in [previous[i], next[i], next[i + 1], previous[i + 1]] {
                    // Darker low down, where less light gets in, and patchy over a metre or two.
                    let height_share = ((place.y - origin.y) / height.max(0.1)).clamp(0.0, 1.0);
                    let patches =
                        0.88 + 0.12 * ((u * 0.8 + phases[1]).sin() * (around * 1.1 + u * 0.3 + phases[2]).sin());
                    let shade = (0.5 + 0.5 * height_share.sqrt()) * patches;
                    body.positions.push((place - origin).to_array());
                    body.normals.push(normal.to_array());
                    body.uvs.push([u, around]);
                    body.colours.push([shade, shade, shade, 1.0]);
                }
                body.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
            }
        }

        // Sprigs of leaves along its shoulders and top, lying close, to soften its outline.
        let count = ((run.end - run.start) * SPRIGS_PER_METRE) as usize;
        for _ in 0..count {
            let distance = random.range(run.start, run.end);
            let points = ring(distance, 0.0);
            // A point on the upper part of the outline (from the shoulders over the top).
            let shoulders: Vec<usize> =
                (0..points.len() - 1).filter(|&i| HEDGE_OUTLINE[i].1 > 0.55 && HEDGE_OUTLINE[i + 1].1 > 0.55).collect();
            let Some(&i) = shoulders.get((random.next_u64() % shoulders.len().max(1) as u64) as usize) else {
                continue;
            };
            let t = random.unit();
            let surface = points[i].0.lerp(points[i + 1].0, t);
            let out = points[i].1.lerp(points[i + 1].1, t).normalize_or(Vec3::Y);
            let along_path = path.along(distance);
            let along_path = Vec3::new(along_path.x, 0.0, along_path.y);
            let tilt = Vec3::new(random.signed(), random.signed(), random.signed()) * 0.25;
            // Mostly along the surface, tipped out a little.
            let lying = (along_path * random.signed().signum() + out * 0.45 + tilt).normalize_or(Vec3::Y);
            let across = lying.cross(out).normalize_or(along_path);
            let size = SPRIG_SIZE * random.range(0.7, 1.2);
            let base = surface - out * 0.05;
            let tip = base + lying * size;
            let half_width = across * size * 0.45;
            let shade = random.range(0.7, 0.9);
            let colour = [shade, shade, shade, 1.0];
            let [u0, u1, v_top, v_bottom] = venture_trees::mesh::sprig_cell(random.next_u64() as u32);
            let (stretch, _) = builder.place(distance, Vec3::ZERO);
            let origin = Vec3::from(stretch.origin);
            let leaves = &mut stretch.leaves;
            let first = leaves.positions.len() as u32;
            for (corner, uv) in [
                (base - half_width, [u0, v_bottom]),
                (base + half_width, [u1, v_bottom]),
                (tip + half_width, [u1, v_top]),
                (tip - half_width, [u0, v_top]),
            ] {
                leaves.positions.push((corner - origin).to_array());
                leaves.normals.push(out.to_array());
                leaves.uvs.push(uv);
                leaves.colours.push(colour);
            }
            leaves.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
}

/// Adds a piece of a fence (the same close up and far away).
fn wood(builder: &mut Builder, random: &mut Random, distance: f32, centre: Vec3, rotation: Quat, size: Vec3) {
    let lumps = std::array::from_fn(|_| random.signed() * 0.05);
    let shade = random.range(0.8, 1.05);
    let uv = [random.range(0.0, 4.0), random.range(0.0, 4.0)];
    let (stretch, local) = builder.place(distance, centre);
    let tint = linear(WOOD.map(|c| c * shade));
    stretch.full.block(local, rotation, size, lumps, uv, tint);
    stretch.far.block(local, rotation, size, lumps, uv, tint);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(kind: WallKind, metres: usize, gap: Option<std::ops::Range<usize>>) -> BakedWall {
        let samples = (0..=metres * 2)
            .map(|i| WallSample {
                x: i as f32 * WALL_SAMPLE_SPACING,
                y: 0.0,
                ground: i as f32 * 0.05,
                open: gap.as_ref().is_some_and(|gap| gap.contains(&i)),
            })
            .collect();
        BakedWall { id: "test".to_string(), kind, height: 1.3, seed: 3, samples }
    }

    #[test]
    fn walls_are_built_in_stretches_within_a_budget() {
        let wall = build_wall(&straight(WallKind::DryStone, 40, None));
        assert_eq!(wall.len(), 3, "40 m makes three 16 m stretches");
        let triangles: usize = wall.iter().map(|stretch| stretch.full.triangles()).sum();
        assert!(triangles < 40 * 2000, "{triangles} triangles for 40 m");
        assert!(wall.iter().all(|stretch| !stretch.far.is_empty()));
        let far: usize = wall.iter().map(|stretch| stretch.far.triangles()).sum();
        assert!(far * 20 < triangles, "far away it's much lighter ({far})");
    }

    #[test]
    fn walls_follow_the_ground_and_leave_gaps() {
        let wall = build_wall(&straight(WallKind::DryStone, 16, Some(12..20)));
        let stretch = &wall[0];
        let origin = Vec3::from(stretch.origin);
        let world: Vec<Vec3> = stretch.full.positions.iter().map(|p| Vec3::from(*p) + origin).collect();
        // Nothing stands in the gap (6 to 10 m along)...
        assert!(!world.iter().any(|p| p.x > 6.6 && p.x < 9.4), "the gap is open");
        // ...and the far end stands higher, as the ground rises.
        let top_near = world.iter().filter(|p| p.x < 2.0).map(|p| p.y).fold(f32::MIN, f32::max);
        let top_far = world.iter().filter(|p| p.x > 14.0).map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(top_far > top_near + 0.5, "{top_near} then {top_far}");
    }

    #[test]
    fn fences_have_posts_and_rails_and_the_same_seed_builds_the_same() {
        let fence = build_wall(&straight(WallKind::Fence, 10, None));
        let blocks = fence[0].full.triangles() / 12;
        // Five or six posts, and three rails between each pair (a 1.3 m fence).
        assert!((13..=24).contains(&blocks), "{blocks} posts and rails");
        let again = build_wall(&straight(WallKind::Fence, 10, None));
        assert_eq!(fence[0].full, again[0].full);
    }

    /// The world positions of a stretch's full mesh whose colour isn't the core's (stones or wood).
    fn pieces(stretch: &Stretch) -> Vec<Vec3> {
        let origin = Vec3::from(stretch.origin);
        let core = linear(CORE);
        stretch
            .full
            .positions
            .iter()
            .zip(&stretch.full.colours)
            .filter(|(_, colour)| **colour != core)
            .map(|(p, _)| Vec3::from(*p) + origin)
            .collect()
    }

    #[test]
    fn stones_finish_the_ends_so_the_core_never_shows() {
        // Open from 6 to 10 m, so the runs are 0 to 5.5 m and 10 to 16 m.
        let wall = build_wall(&straight(WallKind::DryStone, 16, Some(12..20)));
        let stones: Vec<Vec3> = wall.iter().flat_map(pieces).collect();
        for (end, beyond) in [(0.0, -1.0), (5.5, 1.0), (10.0, -1.0), (16.0, 1.0)] {
            // The stones at the end reach past the core's end, all the way across and up.
            let at_end: Vec<Vec3> = stones
                .iter()
                .filter(|p| (p.x - end) * beyond > -0.01 && (p.x - end) * beyond < 0.12)
                .copied()
                .collect();
            let across = at_end.iter().map(|p| p.z).fold(f32::MIN, f32::max)
                - at_end.iter().map(|p| p.z).fold(f32::MAX, f32::min);
            let ground = end * 0.1;
            let top = at_end.iter().map(|p| p.y - ground).fold(f32::MIN, f32::max);
            assert!(across > 0.55, "the end at {end} m is covered across ({across} m)");
            assert!(top > 0.9, "and up ({top} m)");
            let past = |p: &&Vec3| (0.15..2.0).contains(&((p.x - end) * beyond));
            assert!(!stones.iter().any(|p| past(&p)), "nothing well past the end at {end} m");
        }
    }

    #[test]
    fn rings_have_no_ends_where_they_come_back_round() {
        let samples: Vec<WallSample> = (0..=40)
            .map(|i| {
                let angle = i as f32 / 40.0 * std::f32::consts::TAU;
                WallSample { x: angle.cos() * 3.2, y: angle.sin() * 3.2, ground: 0.0, open: false }
            })
            .collect();
        let path = Path { samples: &samples };
        let runs = path.runs();
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].capped_start && !runs[0].capped_end);
        let line = straight(WallKind::DryStone, 10, None);
        let runs = Path { samples: &line.samples }.runs();
        assert!(runs[0].capped_start && runs[0].capped_end);
    }

    #[test]
    fn fences_have_a_post_each_side_of_a_gap() {
        let fence = build_wall(&straight(WallKind::Fence, 16, Some(12..20)));
        let wood: Vec<Vec3> = fence.iter().flat_map(pieces).collect();
        for end in [0.0, 5.5, 10.0, 16.0] {
            assert!(wood.iter().any(|p| (p.x - end).abs() < 0.1 && p.y < end * 0.1), "a post at {end} m");
        }
        assert!(!wood.iter().any(|p| p.x > 5.7 && p.x < 9.8), "nothing across the gap");
    }

    #[test]
    fn hedges_are_the_same_both_sides_and_go_into_the_ground() {
        for (&(x, y), &(mx, my)) in HEDGE_OUTLINE.iter().zip(HEDGE_OUTLINE.iter().rev()) {
            assert!((x + mx).abs() < 1e-6 && (y - my).abs() < 1e-6, "({x}, {y}) mirrors ({mx}, {my})");
        }
        assert!(HEDGE_OUTLINE[0].1 < 0.0, "its feet are in the ground");
    }

    #[test]
    fn hedges_are_leafy_face_outwards_and_leave_gaps() {
        let wall = build_wall(&straight(WallKind::Hedge, 16, Some(12..20)));
        let leaves: usize = wall.iter().map(|stretch| stretch.leaves.triangles()).sum();
        let body: usize = wall.iter().map(|stretch| stretch.full.triangles()).sum();
        assert!(leaves > 200 && leaves < 16 * 1500, "{leaves} leaf triangles");
        assert!(body > 0 && wall.iter().all(|stretch| stretch.far.is_empty()), "one body, at every distance");
        for stretch in &wall {
            let origin = Vec3::from(stretch.origin);
            let part = &stretch.full;
            for triangle in part.indices.as_chunks::<3>().0 {
                let [a, b, c] = triangle.map(|i| Vec3::from(part.positions[i as usize]) + origin);
                let face = (b - a).cross(c - a);
                // (Slivers where the domes pinch shut don't show.)
                if face.length() < 2e-3 {
                    continue;
                }
                // Wound the way its normals face...
                let normals: Vec3 = triangle.iter().map(|&i| Vec3::from(part.normals[i as usize])).sum();
                assert!(face.dot(normals) > 0.0, "wound to face its normals at {a}");
                // ...which, along the middle of a run, are out from the hedge's line.
                let middle = (a + b + c) / 3.0;
                if (1.0..4.5).contains(&middle.x) || (11.0..15.0).contains(&middle.x) {
                    let away = Vec3::new(0.0, middle.y - (0.05 * middle.x + 0.8), middle.z);
                    assert!(face.dot(away) > 0.0, "the body faces out at {middle}");
                }
            }
            let world: Vec<Vec3> = stretch.leaves.positions.iter().map(|p| Vec3::from(*p) + origin).collect();
            assert!(!world.iter().any(|p| p.x > 6.2 && p.x < 9.3), "no leaves in the gap");
        }
        // Each end is closed by a dome reaching a little past it, pinched shut at its tip.
        let body: Vec<Vec3> = wall
            .iter()
            .flat_map(|stretch| stretch.full.positions.iter().map(|p| Vec3::from(*p) + Vec3::from(stretch.origin)))
            .collect();
        for (end, onward) in [(0.0, -1.0), (5.5, 1.0), (10.0, -1.0), (16.0, 1.0)] {
            let past = |p: &Vec3| (p.x - end) * onward;
            let furthest = body.iter().map(past).filter(|&d| d < 1.5).fold(f32::MIN, f32::max);
            assert!((0.35..0.5).contains(&furthest), "the dome past {end} reaches {furthest}");
            let tip: Vec<&Vec3> = body.iter().filter(|p| (past(p) - furthest).abs() < 1e-3).collect();
            let wide =
                tip.iter().map(|p| p.z).fold(f32::MIN, f32::max) - tip.iter().map(|p| p.z).fold(f32::MAX, f32::min);
            assert!(wide < 1e-3, "pinched shut at its tip ({wide} m wide)");
        }
    }
}
