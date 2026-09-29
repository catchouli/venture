//! Bridges the bake places over rivers, so they can be crossed without a
//! long way round (see `docs/plan-bridges.md`):
//! - **where roads cross rivers:** wide stone bridges carrying the road
//!   straight over;
//! - **elsewhere:** about one in every [`SPACING`]-metre square of the world
//!   that a river runs through (and that has no road bridge near), at its
//!   narrowest crossing there, square across the river. Near settlements
//!   they're stone; further out, rope.
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

/// A road bridge reaches this far past the water at each end, in metres.
const ROAD_LANDING: f32 = 5.0;
/// Road bridges are this much wider than the road, in metres (for their walls).
const ROAD_BRIDGE_MARGIN: f32 = 3.0;
/// Rivers crossed along more than this (in metres) aren't bridged by roads.
const LONGEST_ROAD_CROSSING: f32 = 40.0;
/// Other bridges are left out this near a road bridge, in metres.
const ROAD_BRIDGE_ROOM: f32 = 250.0;

/// Bridges over the world's rivers: road bridges, then the rest.
pub fn place_bridges(world: &WorldGenerator) -> Vec<BridgePlan> {
    let roads = road_bridges(world);
    let middle = |plan: &BridgePlan| {
        let (a, b) = (Vec2::from(plan.points[0]), Vec2::from(plan.points[plan.points.len() - 1]));
        (a + b) / 2.0
    };
    let others = river_bridges(world)
        .into_iter()
        .filter(|plan| roads.iter().all(|road| middle(road).distance(middle(plan)) > ROAD_BRIDGE_ROOM));
    let mut all = roads.clone();
    all.extend(others);
    all
}

/// A stone bridge wherever a road crosses a river, along the road.
fn road_bridges(world: &WorldGenerator) -> Vec<BridgePlan> {
    let map = world.map();
    let config = world.config();
    let mut plans: Vec<BridgePlan> = Vec::new();
    for road in map.all_roads() {
        let (a, b) = (Vec2::from(road.start), Vec2::from(road.end));
        let length = a.distance(b);
        if length < 1e-3 {
            continue;
        }
        let way = (b - a) / length;
        // A little past each end, so crossings at a joint between segments are found whole.
        let (from, to) = (-LONGEST_ROAD_CROSSING / 2.0, length + LONGEST_ROAD_CROSSING / 2.0);
        let reach = config.river_max_half_width + 2.0;
        let rivers =
            map.rivers_near(((a.x.min(b.x) - to), (a.y.min(b.y) - to)), ((a.x.max(b.x) + to), (a.y.max(b.y) + to)));
        let wet = |along: f32| {
            let at = a + way * along;
            crate::rivers::nearest_river(&rivers, at.x, at.y)
                .filter(|river| river.distance < river.half_width + 0.5 && river.distance < reach)
        };
        // Each stretch of this segment that's over water (starting within it).
        let mut along = 0.0;
        while along <= length {
            if wet(along).is_none() || wet(along - 1.0).is_some() {
                along += 1.0;
                continue;
            }
            let start = along;
            let mut end = along;
            while end < to && wet(end + 1.0).is_some() {
                end += 1.0;
            }
            along = end + 1.0;
            if end - start > LONGEST_ROAD_CROSSING || start < from {
                continue;
            }
            let Some(river) = wet((start + end) / 2.0) else { continue };
            let (first, last) = (a + way * (start - ROAD_LANDING), a + way * (end + 1.0 + ROAD_LANDING));
            let middle = (first + last) / 2.0;
            if plans.iter().any(|plan| {
                let (p, q) = (Vec2::from(plan.points[0]), Vec2::from(plan.points[1]));
                ((p + q) / 2.0).distance(middle) < 20.0
            }) {
                continue;
            }
            let (first_ground, last_ground) =
                (world.ground_height(first.x, first.y), world.ground_height(last.x, last.y));
            let line = (first_ground + last_ground) / 2.0 - river.surface;
            let kind = BridgeKind::Stone;
            plans.push(BridgePlan {
                id: String::new(),
                kind,
                points: vec![(first.x, first.y), (last.x, last.y)],
                width: config.road_half_width * 2.0 + ROAD_BRIDGE_MARGIN,
                curve: kind.usual_curve().max(STONE_CLEARANCE - line),
                seed: (middle.x as u64) << 32 | middle.y as u64,
            });
        }
    }
    plans.sort_by(|p, q| p.points[0].partial_cmp(&q.points[0]).unwrap_or(std::cmp::Ordering::Equal));
    for (n, plan) in plans.iter_mut().enumerate() {
        plan.id = format!("road_bridge_{n}");
    }
    plans
}

/// About one bridge in every square a river crosses.
fn river_bridges(world: &WorldGenerator) -> Vec<BridgePlan> {
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
