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

/// A stretch of wall, ready to draw.
#[derive(Clone, Debug, Default)]
pub struct Stretch {
    /// Where its meshes are relative to: (x, height, y) in the world, in metres.
    pub origin: [f32; 3],
    /// Close up.
    pub full: MeshPart,
    /// Far away.
    pub far: MeshPart,
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
    }
    builder.stretches.retain(|stretch| !stretch.full.is_empty() || !stretch.far.is_empty());
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
}
