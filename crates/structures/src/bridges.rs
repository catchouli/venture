//! Bridges (see `docs/plan-bridges.md`): a stone bridge on an arch with
//! low walls either side, or a rope bridge of planks on ropes between
//! wooden posts, along a drawn path, with its deck curving up (an arch) or
//! hanging down (a rope bridge's sag) between its ends.
//!
//! A bridge is built from [`Piece`]s: skewed boxes, one for each stretch of
//! each part between two samples. Their corners are both its meshes' and
//! its solid shapes' (`venture_collision` makes colliders of them), so what
//! is seen is what is walked on.

use glam::{Mat3, Quat, Vec2, Vec3};
use venture_world::{BakedBridge, BakedWall, BridgeKind, BridgePlan, WallKind, WallSample};

use crate::mesh::{MeshPart, linear};
use crate::path::sample_path;
use crate::random::Random;
use crate::walls::build_wall;

/// The authored bridges file, in the authoring folder.
pub const BRIDGES_FILE: &str = "bridges.ron";
/// How far apart a bridge's deck is sampled, in metres.
pub const BRIDGE_SAMPLE_SPACING: f32 = 0.5;
/// How far a deck's ends sit above the ground, in metres.
const END_LIFT: f32 = 0.1;

/// A stone bridge's deck: how thick, in metres.
const STONE_DECK: f32 = 0.5;
/// A stone bridge's walls (dry-stone walls: see `walls`): how high above
/// the deck, and how thick, in metres.
const PARAPET_HEIGHT: f32 = 1.0;
const PARAPET_THICKNESS: f32 = 0.8;
/// A stone bridge's sides are laid in courses this high, in metres.
const COURSE: f32 = 0.3;
/// How far the stones stand out of a stone bridge's core, in metres.
const PROUD: f32 = 0.04;
/// How far below the deck's top the arch's crown is, in metres.
const ARCH_CROWN: f32 = 1.0;
/// How far into the ground a stone bridge's ends go, in metres.
const FOOTING: f32 = 1.0;

/// A rope bridge's planks: how thick, how long along the bridge, and the gap between them, in metres.
const PLANK_THICKNESS: f32 = 0.08;
const PLANK_LENGTH: f32 = 0.24;
const PLANK_GAP: f32 = 0.06;
/// A rope bridge's handrails: how high above the deck, in metres.
const HANDRAIL_HEIGHT: f32 = 1.1;
/// A rope bridge's sides (the netting between its handrails and deck, solid to walkers): how thick, in metres.
const NETTING_THICKNESS: f32 = 0.1;
/// Rope thickness, in metres.
const ROPE: f32 = 0.045;
/// A rope bridge's posts: how thick and how tall (above the deck), in metres.
const POST_SIZE: f32 = 0.2;
const POST_HEIGHT: f32 = 1.5;
/// How far apart the ropes from handrail to deck are, in metres.
const HANGER_SPACING: f32 = 0.9;

/// What a piece of a bridge is made of (which material it's drawn with).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BridgeSurface {
    /// Dry stone.
    Stone,
    /// Wood: planks and posts.
    Wood,
    /// Rope.
    Rope,
}

/// A solid piece of a bridge: a box, skewed to follow the deck. Its
/// corners are `[x, height, y]` in metres (Y up, as meshes and Rapier have
/// it): the four at its start (left bottom, right bottom, right top, left
/// top), then the same four at its end.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Its corners.
    pub corners: [Vec3; 8],
    /// What it's made of.
    pub surface: BridgeSurface,
    /// Whether its top is walked on (the deck); otherwise it's in the way.
    pub walkable: bool,
    /// Whether it's only for show (ropes): not solid.
    pub decoration: bool,
    /// Whether it's drawn as it is (a stone bridge's walls are drawn as
    /// dry-stone walls instead, and a rope bridge's deck and sides as its
    /// planks and ropes).
    pub drawn: bool,
}

/// A bridge's meshes, by what they're made of (`[x, height, y]`, Y up).
#[derive(Clone, Debug, Default)]
pub struct BridgeMeshes {
    /// Each surface's mesh.
    pub parts: Vec<(BridgeSurface, MeshPart)>,
}

/// Reads the authored bridges file.
pub fn parse_bridges(text: &str) -> Result<Vec<BridgePlan>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Problems with authored bridges.
pub fn check_bridges(plans: &[BridgePlan]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, plan) in plans.iter().enumerate() {
        let name = if plan.id.is_empty() { format!("bridge {}", i + 1) } else { format!("bridge \"{}\"", plan.id) };
        if plan.id.trim().is_empty() {
            problems.push(format!("{name} has no id"));
        } else if plans[..i].iter().any(|other| other.id == plan.id) {
            problems.push(format!("{name} is there twice"));
        }
        if plan.points.len() < 2 {
            problems.push(format!("{name} needs at least two points"));
        }
        if !(0.8..=12.0).contains(&plan.width) {
            problems.push(format!("{name}'s width must be from 0.8 to 12 metres, not {}", plan.width));
        }
        if !(-20.0..=20.0).contains(&plan.curve) {
            problems.push(format!("{name}'s curve must be from -20 to 20 metres, not {}", plan.curve));
        }
    }
    problems
}

/// A bridge's deck along its path: its ends on the ground (a little above),
/// straight between them, plus its curve (a parabola: most in the middle),
/// but never below the ground under it.
pub fn bake_bridge(plan: &BridgePlan, ground: impl Fn(f32, f32) -> f32) -> BakedBridge {
    let points: Vec<Vec2> = plan.points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    if points.len() < 2 {
        return BakedBridge { plan: plan.clone(), deck: Vec::new(), ground: Vec::new() };
    }
    let samples = sample_path(&points, false, BRIDGE_SAMPLE_SPACING);
    let length = samples.last().map_or(0.0, |sample| sample.distance).max(1e-3);
    let (first, last) = (points[0], points[points.len() - 1]);
    let (start, end) = (ground(first.x, first.y) + END_LIFT, ground(last.x, last.y) + END_LIFT);
    let ground: Vec<f32> = samples.iter().map(|sample| ground(sample.at.x, sample.at.y)).collect();
    // (Never below the ground under it: a sagging bridge runs along its
    // banks, and only hangs over the gap.)
    let deck = samples
        .iter()
        .zip(&ground)
        .map(|(sample, &under)| {
            let t = sample.distance / length;
            let height = start + (end - start) * t + plan.curve * 4.0 * t * (1.0 - t);
            [sample.at.x, sample.at.y, height.max(under + END_LIFT)]
        })
        .collect();
    BakedBridge { plan: plan.clone(), deck, ground }
}

/// The deck's middle at each sample (Y up), the way across it (level, to
/// the right), and the ground under it.
fn frames(bridge: &BakedBridge) -> Vec<(Vec3, Vec3, f32)> {
    let deck = &bridge.deck;
    (0..deck.len())
        .map(|i| {
            let (before, after) = (deck[i.saturating_sub(1)], deck[(i + 1).min(deck.len() - 1)]);
            let along = Vec2::new(after[0] - before[0], after[1] - before[1]).normalize_or(Vec2::X);
            // (To the right of the way along, seen from above.)
            let across = Vec3::new(along.y, 0.0, -along.x);
            let [x, y, height] = deck[i];
            (Vec3::new(x, height, y), across, bridge.ground[i])
        })
        .collect()
}

/// How far along the deck each sample is (0 to 1).
fn fractions(bridge: &BakedBridge) -> Vec<f32> {
    let count = bridge.deck.len().max(2) - 1;
    (0..bridge.deck.len()).map(|i| i as f32 / count as f32).collect()
}

/// A bridge's pieces.
pub fn bridge_pieces(bridge: &BakedBridge) -> Vec<Piece> {
    if bridge.deck.len() < 2 {
        return Vec::new();
    }
    match bridge.plan.kind {
        BridgeKind::Stone => stone_pieces(bridge),
        BridgeKind::Rope => rope_pieces(bridge),
    }
}

/// A band along the bridge: between `inner` and `outer` (across, from the
/// middle), from `bottom` to `top` (heights, per sample), from sample `a` to `b`.
fn band(
    frames: &[(Vec3, Vec3, f32)],
    (a, b): (usize, usize),
    (inner, outer): (f32, f32),
    bottom: impl Fn(usize) -> f32,
    top: impl Fn(usize) -> f32,
) -> [Vec3; 8] {
    let corners = |i: usize| {
        let (middle, across, _) = frames[i];
        let at = |offset: f32, height: f32| {
            let flat = middle + across * offset;
            Vec3::new(flat.x, height, flat.z)
        };
        [at(inner, bottom(i)), at(outer, bottom(i)), at(outer, top(i)), at(inner, top(i))]
    };
    let (start, end) = (corners(a), corners(b));
    [start[0], start[1], start[2], start[3], end[0], end[1], end[2], end[3]]
}

/// A stone bridge's profile at each sample: its deck's top, and its arch's underside.
fn stone_profile(bridge: &BakedBridge, frames: &[(Vec3, Vec3, f32)]) -> Vec<(f32, f32)> {
    let t = fractions(bridge);
    let (first, last) = (frames[0].2, frames[frames.len() - 1].2);
    (0..frames.len())
        .map(|i| {
            let top = frames[i].0.y;
            // The arch: its underside rises from the ground at the ends to just
            // under the deck in the middle (a semi-ellipse, near enough).
            let low = first + (last - first) * t[i] - FOOTING;
            let rise = (std::f32::consts::PI * t[i]).sin().max(0.0).sqrt();
            let crown = top - ARCH_CROWN;
            (top, (low + (crown - low) * rise).min(top - STONE_DECK))
        })
        .collect()
}

fn stone_pieces(bridge: &BakedBridge) -> Vec<Piece> {
    let frames = frames(bridge);
    let profile = stone_profile(bridge, &frames);
    let half = bridge.plan.width / 2.0;
    let deck_top = |i: usize| profile[i].0;
    let underside = |i: usize| profile[i].1;
    let mut pieces = Vec::new();
    for i in 0..frames.len() - 1 {
        let stretch = (i, i + 1);
        let stone = |corners, walkable, drawn| Piece {
            corners,
            surface: BridgeSurface::Stone,
            walkable,
            decoration: false,
            drawn,
        };
        pieces.push(stone(band(&frames, stretch, (-half, half), |i| deck_top(i) - STONE_DECK, deck_top), true, true));
        pieces.push(stone(band(&frames, stretch, (-half, half), underside, |i| deck_top(i) - STONE_DECK), false, true));
        for side in [-1.0, 1.0] {
            let wall = (side * (half - PARAPET_THICKNESS), side * half);
            let wall = if side < 0.0 { (wall.1, wall.0) } else { wall };
            pieces.push(stone(band(&frames, stretch, wall, deck_top, |i| deck_top(i) + PARAPET_HEIGHT), false, false));
        }
    }
    pieces
}

/// Where a stone bridge is at `distance` along it: its deck's middle (at
/// the deck's top), the way across (level), the way along (up its slope),
/// and its deck's top and arch's underside there.
fn stone_at(frames: &[(Vec3, Vec3, f32)], profile: &[(f32, f32)], distance: f32) -> (Vec3, Vec3, Vec3, f32, f32) {
    let last = frames.len() - 1;
    let position = (distance / BRIDGE_SAMPLE_SPACING).clamp(0.0, last as f32 - 1e-3);
    let (i, f) = (position.floor() as usize, position.fract());
    let j = (i + 1).min(last);
    let middle = frames[i].0.lerp(frames[j].0, f);
    let across = frames[i].1.lerp(frames[j].1, f).normalize_or(Vec3::X);
    let along = (frames[j].0 - frames[i].0).normalize_or(Vec3::new(-across.z, 0.0, across.x));
    let top = profile[i].0 + (profile[j].0 - profile[i].0) * f;
    let under = profile[i].1 + (profile[j].1 - profile[i].1) * f;
    (middle, across, along, top, under)
}

/// A stone's turn: its x across the bridge, its z along `along`.
fn stone_turn(across: Vec3, along: Vec3) -> Quat {
    let along = along.normalize_or(Vec3::Z);
    let across = (across - along * across.dot(along)).normalize_or(Vec3::X);
    Quat::from_mat3(&Mat3::from_cols(across, along.cross(across), along))
}

/// A stone bridge's stones: its sides laid in courses, a ring of stones
/// round its arch, and flagstones on its deck (its walls are dry-stone
/// walls: see [`build_bridge`]).
fn stone_dressing(bridge: &BakedBridge, part: &mut MeshPart) {
    let frames = frames(bridge);
    let profile = stone_profile(bridge, &frames);
    let half = bridge.plan.width / 2.0;
    let length = (frames.len() - 1) as f32 * BRIDGE_SAMPLE_SPACING;
    let mut random = Random::new(bridge.plan.seed ^ 0x0570_14e5);
    let deepest = profile.iter().map(|(top, under)| top - under).fold(0.0, f32::max);
    let tint = |random: &mut Random| {
        let shade = random.range(0.8, 1.05);
        linear([0.92 * shade, 0.9 * shade, 0.86 * shade])
    };
    let lumps = |random: &mut Random| std::array::from_fn(|_| random.range(-0.08, 0.08));
    // The sides, in courses down from the deck (above the arch).
    for side in [-1.0_f32, 1.0] {
        for row in 0..(deepest / COURSE).ceil() as usize {
            let mut along = -random.range(0.0, 0.4);
            while along < length {
                let stone = random.range(0.35, 0.75);
                let at = (along + stone / 2.0).clamp(0.0, length);
                along += stone;
                let (middle, across, way, top, under) = stone_at(&frames, &profile, at);
                let high = top - row as f32 * COURSE;
                if high - COURSE < under + 0.05 {
                    continue;
                }
                let face = middle + across * side * (half - 0.15 + PROUD);
                let centre = Vec3::new(face.x, high - COURSE / 2.0, face.z);
                let size = Vec3::new(0.3, COURSE * 0.92, stone * 0.94);
                let uv = [random.range(0.0, 4.0), random.range(0.0, 4.0)];
                part.block(centre, stone_turn(across, way), size, lumps(&mut random), uv, tint(&mut random));
            }
        }
        // The arch's ring, along its underside (where it's clear of the ground).
        let mut along = 0.0;
        while along < length {
            let (middle, across, _, top, under) = stone_at(&frames, &profile, along);
            let (_, _, _, _, next) = stone_at(&frames, &profile, along + 0.1);
            let (_, _, _, _, before) = stone_at(&frames, &profile, (along - 0.1).max(0.0));
            let way = Vec3::new(-across.z, 0.0, across.x);
            let tangent = (way * 0.2 + Vec3::Y * (next - before)).normalize_or(way);
            let outward = stone_turn(across, tangent) * Vec3::Y;
            if under < top - STONE_DECK - 0.05 {
                let face = middle + across * side * (half - 0.2 + PROUD);
                let centre = Vec3::new(face.x, under, face.z) + outward * 0.22;
                let size = Vec3::new(0.4, 0.46, 0.3);
                let uv = [random.range(0.0, 4.0), random.range(0.0, 4.0)];
                part.block(centre, stone_turn(across, tangent), size, lumps(&mut random), uv, tint(&mut random));
            }
            along += 0.32;
        }
    }
    // Flagstones on the deck, between its walls.
    let inside = half - PARAPET_THICKNESS;
    let mut along = 0.0;
    while along < length {
        let stone = random.range(0.45, 0.7);
        let (middle, across, way, top, _) = stone_at(&frames, &profile, along + stone / 2.0);
        let mut offset = -inside;
        while offset < inside {
            let width = random.range(0.4, 0.8).min(inside - offset);
            let flat = middle + across * (offset + width / 2.0);
            let centre = Vec3::new(flat.x, top - 0.03 + random.range(0.0, 0.015), flat.z);
            let size = Vec3::new(width * 0.95, 0.1, stone * 0.95);
            let uv = [random.range(0.0, 4.0), random.range(0.0, 4.0)];
            let flat_lumps = std::array::from_fn(|_| random.range(-0.02, 0.02));
            part.block(centre, stone_turn(across, way), size, flat_lumps, uv, tint(&mut random));
            offset += width;
        }
        along += stone;
    }
}

/// A stone bridge's walls, as dry-stone walls along its deck's edges.
fn stone_walls(bridge: &BakedBridge) -> Vec<BakedWall> {
    let frames = frames(bridge);
    let half = bridge.plan.width / 2.0;
    [-1.0_f32, 1.0]
        .iter()
        .enumerate()
        .map(|(n, &side)| BakedWall {
            id: format!("{}_wall_{n}", bridge.plan.id),
            kind: WallKind::DryStone,
            height: PARAPET_HEIGHT,
            seed: bridge.plan.seed.wrapping_add(n as u64 + 1),
            samples: frames
                .iter()
                .map(|&(middle, across, _)| {
                    let at = middle + across * side * (half - PARAPET_THICKNESS / 2.0);
                    WallSample { x: at.x, y: at.z, ground: middle.y, open: false }
                })
                .collect(),
        })
        .collect()
}

fn rope_pieces(bridge: &BakedBridge) -> Vec<Piece> {
    let frames = frames(bridge);
    let half = bridge.plan.width / 2.0;
    let deck_top = |i: usize| frames[i].0.y;
    let mut random = Random::new(bridge.plan.seed);
    let mut pieces = Vec::new();
    // (What's solid, a rope bridge's deck and sides, isn't drawn: its planks, ropes and posts are.)
    let piece = |corners, surface, walkable, decoration| Piece {
        corners,
        surface,
        walkable,
        decoration,
        drawn: decoration || surface == BridgeSurface::Wood && !walkable,
    };
    for i in 0..frames.len() - 1 {
        let stretch = (i, i + 1);
        // The deck (solid and walked on; drawn as the planks, below).
        pieces.push(piece(
            band(&frames, stretch, (-half, half), |i| deck_top(i) - PLANK_THICKNESS, deck_top),
            BridgeSurface::Wood,
            true,
            false,
        ));
        for side in [-1.0_f32, 1.0] {
            // The netting between handrail and deck: solid, but not drawn.
            let (a, b) = (side * half, side * (half - NETTING_THICKNESS));
            let (inner, outer) = if side < 0.0 { (a, b) } else { (b, a) };
            pieces.push(piece(
                band(&frames, stretch, (inner, outer), deck_top, |i| deck_top(i) + HANDRAIL_HEIGHT),
                BridgeSurface::Rope,
                false,
                false,
            ));
            // The ropes along each side: the handrail, and one along the deck's edge.
            for height in [HANDRAIL_HEIGHT, 0.0] {
                let (inner, outer) = (side * half - ROPE / 2.0, side * half + ROPE / 2.0);
                pieces.push(piece(
                    band(&frames, stretch, (inner, outer), |i| deck_top(i) + height, |i| deck_top(i) + height + ROPE),
                    BridgeSurface::Rope,
                    false,
                    true,
                ));
            }
        }
    }
    // The planks, across the deck, a little uneven.
    let spacing = PLANK_LENGTH + PLANK_GAP;
    let length = (bridge.deck.len() - 1) as f32 * BRIDGE_SAMPLE_SPACING;
    let mut along = PLANK_GAP;
    while along + PLANK_LENGTH < length {
        let at = |distance: f32| {
            let position = (distance / BRIDGE_SAMPLE_SPACING).min((frames.len() - 1) as f32 - 1e-3);
            let (i, f) = (position.floor() as usize, position.fract());
            let (a, b) = (frames[i], frames[(i + 1).min(frames.len() - 1)]);
            (a.0.lerp(b.0, f), a.1.lerp(b.1, f).normalize_or(Vec3::X))
        };
        let lift = random.range(-0.01, 0.01);
        let reach = half * random.range(0.97, 1.02);
        let (start, start_across) = at(along);
        let (end, end_across) = at(along + PLANK_LENGTH);
        let corners = |middle: Vec3, across: Vec3| {
            let (left, right) = (middle - across * reach, middle + across * reach);
            let down = Vec3::Y * (PLANK_THICKNESS - lift);
            [left - down, right - down, right + Vec3::Y * lift, left + Vec3::Y * lift]
        };
        let (s, e) = (corners(start, start_across), corners(end, end_across));
        pieces.push(piece([s[0], s[1], s[2], s[3], e[0], e[1], e[2], e[3]], BridgeSurface::Wood, false, true));
        along += spacing;
    }
    // The ropes from handrail down to the deck's edge.
    let mut along = HANGER_SPACING / 2.0;
    while along < length {
        let i = ((along / BRIDGE_SAMPLE_SPACING) as usize).min(frames.len() - 1);
        let (middle, across, _) = frames[i];
        for side in [-1.0, 1.0] {
            let foot = middle + across * side * half;
            pieces.push(upright(foot, deck_top(i), HANDRAIL_HEIGHT, ROPE, across, BridgeSurface::Rope, true));
        }
        along += HANGER_SPACING;
    }
    // The posts at each end (solid: in the way).
    for i in [0, frames.len() - 1] {
        let (middle, across, ground) = frames[i];
        for side in [-1.0, 1.0] {
            let foot = middle + across * side * (half + POST_SIZE / 2.0);
            let bottom = ground - 0.3;
            let height = deck_top(i) + POST_HEIGHT - bottom;
            pieces.push(upright(foot, bottom, height, POST_SIZE, across, BridgeSurface::Wood, false));
        }
    }
    pieces
}

/// An upright post or rope: `size` square, from `bottom` up `height`, at `foot` (across and along).
fn upright(
    foot: Vec3,
    bottom: f32,
    height: f32,
    size: f32,
    across: Vec3,
    surface: BridgeSurface,
    decoration: bool,
) -> Piece {
    let along = Vec3::new(-across.z, 0.0, across.x);
    let (x, z) = (across * size / 2.0, along * size / 2.0);
    let at = |offset: Vec3, height: f32| Vec3::new(foot.x + offset.x, height, foot.z + offset.z);
    let (low, high) = (bottom, bottom + height);
    let start = [at(-x - z, low), at(x - z, low), at(x - z, high), at(-x - z, high)];
    let end = [at(-x + z, low), at(x + z, low), at(x + z, high), at(-x + z, high)];
    Piece {
        corners: [start[0], start[1], start[2], start[3], end[0], end[1], end[2], end[3]],
        surface,
        walkable: false,
        decoration,
        drawn: true,
    }
}

/// A bridge's colours, by surface (multiplying their textures).
fn colour(surface: BridgeSurface) -> [f32; 4] {
    match surface {
        BridgeSurface::Stone => linear([0.92, 0.9, 0.86]),
        BridgeSurface::Wood => linear([0.85, 0.75, 0.62]),
        BridgeSurface::Rope => linear([0.62, 0.52, 0.36]),
    }
}

/// A bridge's meshes.
pub fn build_bridge(bridge: &BakedBridge) -> BridgeMeshes {
    let mut parts: Vec<(BridgeSurface, MeshPart)> = Vec::new();
    for piece in bridge_pieces(bridge) {
        if !piece.drawn {
            continue;
        }
        let part = match parts.iter_mut().find(|(surface, _)| *surface == piece.surface) {
            Some((_, part)) => part,
            None => {
                parts.push((piece.surface, MeshPart::default()));
                &mut parts.last_mut().expect("just added").1
            }
        };
        skewed_box(part, &piece.corners, colour(piece.surface));
    }
    if bridge.plan.kind == BridgeKind::Stone && bridge.deck.len() >= 2 {
        let mut stones = MeshPart::default();
        stone_dressing(bridge, &mut stones);
        for wall in stone_walls(bridge) {
            for stretch in build_wall(&wall) {
                let origin = Vec3::from(stretch.origin);
                let mut full = stretch.full;
                for position in &mut full.positions {
                    *position = (Vec3::from(*position) + origin).to_array();
                }
                append(&mut stones, &full);
            }
        }
        match parts.iter_mut().find(|(surface, _)| *surface == BridgeSurface::Stone) {
            Some((_, part)) => append(part, &stones),
            None => parts.push((BridgeSurface::Stone, stones)),
        }
    }
    BridgeMeshes { parts }
}

/// Adds one mesh's triangles to another's.
fn append(to: &mut MeshPart, from: &MeshPart) {
    let first = to.positions.len() as u32;
    to.positions.extend_from_slice(&from.positions);
    to.normals.extend_from_slice(&from.normals);
    to.uvs.extend_from_slice(&from.uvs);
    to.colours.extend_from_slice(&from.colours);
    to.indices.extend(from.indices.iter().map(|i| i + first));
}

/// Adds a skewed box's six faces, facing out, textured a unit a metre.
fn skewed_box(part: &mut MeshPart, c: &[Vec3; 8], colour: [f32; 4]) {
    let centre = c.iter().copied().sum::<Vec3>() / 8.0;
    let faces = [
        [c[0], c[1], c[2], c[3]], // start
        [c[5], c[4], c[7], c[6]], // end
        [c[3], c[2], c[6], c[7]], // top
        [c[1], c[0], c[4], c[5]], // bottom
        [c[0], c[3], c[7], c[4]], // one side
        [c[2], c[1], c[5], c[6]], // the other
    ];
    for mut face in faces {
        let normal = (face[1] - face[0]).cross(face[3] - face[0]);
        let middle = face.iter().copied().sum::<Vec3>() / 4.0;
        if normal.dot(middle - centre) < 0.0 {
            face.reverse();
        }
        if normal.length_squared() < 1e-10 {
            continue;
        }
        // Texture: across the face's longest level direction, and up.
        let u_axis = {
            let a = face[1] - face[0];
            let b = face[3] - face[0];
            let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
            if flat(a).length() >= flat(b).length() {
                flat(a).normalize_or(Vec3::X)
            } else {
                flat(b).normalize_or(Vec3::X)
            }
        };
        let flat_face = normal.normalize_or(Vec3::Y).y.abs() > 0.7;
        let v_axis = if flat_face { u_axis.cross(Vec3::Y) } else { Vec3::Y };
        let uvs = face.map(|p| [p.dot(u_axis), -p.dot(v_axis)]);
        part.quad(face, uvs, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(kind: BridgeKind, curve: f32) -> BridgePlan {
        BridgePlan {
            id: "b".into(),
            kind,
            points: vec![(0.0, 0.0), (20.0, 0.0)],
            width: kind.usual_width(),
            curve,
            seed: 1,
        }
    }

    #[test]
    fn decks_start_and_end_on_the_ground_and_curve_between() {
        let baked = bake_bridge(&plan(BridgeKind::Stone, 2.0), |x, _| x * 0.1);
        let first = baked.deck[0];
        let last = *baked.deck.last().unwrap();
        assert!((first[2] - END_LIFT).abs() < 1e-3);
        assert!((last[2] - (2.0 + END_LIFT)).abs() < 0.05, "{last:?}");
        let middle = baked.deck[baked.deck.len() / 2];
        assert!((middle[2] - (1.0 + END_LIFT + 2.0)).abs() < 0.1, "risen by the curve: {middle:?}");
        // Over a gap 3 m deep, a rope bridge hangs down; over its banks, it stays on them.
        let gap = |x: f32, _: f32| if (4.0..16.0).contains(&x) { -3.0 } else { 0.0 };
        let sagging = bake_bridge(&plan(BridgeKind::Rope, -1.0), gap);
        assert!(sagging.deck[sagging.deck.len() / 2][2] < -0.8, "hangs down");
        assert!(sagging.deck[2][2] >= END_LIFT - 1e-4, "on the bank: {:?}", sagging.deck[2]);
    }

    #[test]
    fn both_kinds_build_closed_meshes_and_walkable_decks() {
        for kind in BridgeKind::ALL {
            let baked = bake_bridge(&plan(kind, kind.usual_curve()), |_, _| 0.0);
            let pieces = bridge_pieces(&baked);
            assert!(pieces.iter().any(|piece| piece.walkable), "{kind:?} has a deck");
            assert!(pieces.iter().any(|piece| !piece.walkable && !piece.decoration), "{kind:?} has sides");
            let meshes = build_bridge(&baked);
            assert!(!meshes.parts.is_empty());
            for (_, part) in &meshes.parts {
                assert!(part.triangles() > 0);
                assert!(part.normals.iter().all(|n| (Vec3::from(*n).length() - 1.0).abs() < 1e-3));
            }
        }
    }

    #[test]
    fn bad_plans_are_found() {
        let mut short = plan(BridgeKind::Stone, 1.0);
        short.points.truncate(1);
        short.width = 50.0;
        let problems = check_bridges(&[short.clone(), short]);
        assert!(problems.iter().any(|p| p.contains("two points")));
        assert!(problems.iter().any(|p| p.contains("width")));
        assert!(problems.iter().any(|p| p.contains("twice")));
    }
}
