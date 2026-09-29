//! Bridges the bake places over rivers, so they can be crossed without a
//! long way round (see `docs/plan-bridges.md`): about one in every
//! [`SPACING`]-metre square of the world that a river runs through, at its
//! narrowest crossing there, square across the river. Near settlements
//! they're stone; further out, rope.
//!
//! (Placed simply: the world's land is likely to be laid out again.)

use std::collections::HashMap;

use glam::Vec2;
use venture_world::{BridgeKind, BridgePlan};

use crate::WorldGenerator;

/// About one bridge in every square this size (metres) that a river crosses.
const SPACING: f32 = 400.0;
/// Rivers wider than this (half, in metres) aren't bridged.
const WIDEST: f32 = 9.0;
/// How far past the water each end reaches, in metres.
const LANDING: f32 = 4.0;
/// How high above the water a stone bridge's middle is, at least, in metres.
const STONE_CLEARANCE: f32 = 1.5;
/// How high above the water a rope bridge's middle hangs, at least, in metres.
const ROPE_CLEARANCE: f32 = 0.8;
/// Bridges this near a settlement are stone, in metres.
const STONE_NEAR: f32 = 220.0;

/// Bridges over the world's rivers.
pub fn place_bridges(world: &WorldGenerator) -> Vec<BridgePlan> {
    let map = world.map();
    // The best crossing in each square: its plan, and the river's half-width there.
    let mut best: HashMap<(i32, i32), (f32, BridgePlan)> = HashMap::new();
    for j in 0..map.size() {
        for i in 0..map.size() {
            let Some(segment) = map.point_river(i, j) else { continue };
            let (a, b) = (Vec2::new(segment.start.x, segment.start.y), Vec2::new(segment.end.x, segment.end.y));
            let middle = (a + b) / 2.0;
            let river = segment.sample(middle.x, middle.y);
            if river.half_width > WIDEST || river.half_width <= 0.0 {
                continue;
            }
            let across = (b - a).normalize_or_zero().perp();
            if across == Vec2::ZERO {
                continue;
            }
            let reach = river.half_width + LANDING;
            let (start, end) = (middle - across * reach, middle + across * reach);
            // Both ends on dry land, and off the map's edges.
            let size = world.config().world_size as f32;
            let inside = |p: Vec2| p.x > 1.0 && p.y > 1.0 && p.x < size - 1.0 && p.y < size - 1.0;
            let (start_ground, end_ground) = (world.ground_height(start.x, start.y), world.ground_height(end.x, end.y));
            if !inside(start) || !inside(end) || start_ground.min(end_ground) < river.surface + 0.3 {
                continue;
            }
            let square = ((middle.x / SPACING).floor() as i32, (middle.y / SPACING).floor() as i32);
            if best.get(&square).is_some_and(|(half_width, _)| *half_width <= river.half_width) {
                continue;
            }
            let near_settlement = map
                .settlements()
                .iter()
                .any(|settlement| Vec2::from(settlement.position).distance(middle) < STONE_NEAR);
            let kind = if near_settlement { BridgeKind::Stone } else { BridgeKind::Rope };
            // How far the straight line between the ends is above the water, in the middle.
            let line = (start_ground + end_ground) / 2.0 - river.surface;
            let curve = match kind {
                BridgeKind::Stone => kind.usual_curve().max(STONE_CLEARANCE - line),
                BridgeKind::Rope => kind.usual_curve().max(ROPE_CLEARANCE - line).min(0.0),
            };
            let plan = BridgePlan {
                id: format!("river_bridge_{i}_{j}"),
                kind,
                points: vec![(start.x, start.y), (end.x, end.y)],
                width: kind.usual_width(),
                curve,
                seed: (i as u64) << 32 | j as u64,
            };
            best.insert(square, (river.half_width, plan));
        }
    }
    let mut plans: Vec<BridgePlan> = best.into_values().map(|(_, plan)| plan).collect();
    plans.sort_by(|a, b| a.id.cmp(&b.id));
    plans
}
