//! Roads: planning a network between settlements, and road geometry.
//!
//! The network is a minimum spanning tree of the settlements (so every
//! settlement is reachable), plus a few extra links between close neighbours
//! so there are some loops. Each link is routed across the coarse map with
//! A* search. The cost favours flat ground, avoids river crossings, forbids
//! lakes and the sea, and makes reusing an existing road cheap, so roads join
//! into a network instead of running side by side. Routes are then smoothed
//! into curves with Chaikin's algorithm.

use std::collections::BinaryHeap;

use crate::WorldGenConfig;
use crate::geometry::{Point, chaikin_smooth, closest_on_segment, lerp_point};
use crate::grid::{MinEntry, cell_distance, cell_hash, neighbours};
use crate::world_map::MapGrid;

/// Links shorter than this many times the settlement spacing get an extra road,
/// even if the spanning tree doesn't need one.
const EXTRA_LINK_FACTOR: f32 = 1.6;

/// How many times routes are smoothed.
const SMOOTHING_ITERATIONS: usize = 2;

/// Up to how much extra (as a fraction) each map cell randomly costs to cross,
/// so roads over flat land wander a little instead of running dead straight.
const WANDER: f32 = 0.8;

/// A straight piece of road, in tile coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadSegment {
    /// One end.
    pub start: Point,
    /// The other end.
    pub end: Point,
}

/// A road's centre line at its closest point to some position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadSample {
    /// Distance from the position to the road's centre line.
    pub distance: f32,
    /// The closest point on the centre line.
    pub closest: Point,
}

impl RoadSegment {
    /// Samples the road at its closest point to `(x, y)`.
    pub fn sample(&self, x: f32, y: f32) -> RoadSample {
        let (t, distance) = closest_on_segment(self.start, self.end, (x, y));
        RoadSample { distance, closest: lerp_point(self.start, self.end, t) }
    }
}

/// The road closest to `(x, y)`, if any. Ties go to the first segment.
pub fn nearest_road(segments: &[RoadSegment], x: f32, y: f32) -> Option<RoadSample> {
    let mut nearest: Option<RoadSample> = None;
    for segment in segments {
        let sample = segment.sample(x, y);
        if nearest.is_none_or(|best| sample.distance < best.distance) {
            nearest = Some(sample);
        }
    }
    nearest
}

/// Plans roads between settlements (map point indices). Returns each road as
/// a smoothed polyline in tile coordinates.
pub fn plan_roads(config: &WorldGenConfig, grid: &MapGrid, settlements: &[usize]) -> Vec<Vec<Point>> {
    let mut links = spanning_tree(grid, settlements);
    let extra_distance = EXTRA_LINK_FACTOR * config.settlement_spacing / grid.cell_size;
    for (i, &a) in settlements.iter().enumerate() {
        for &b in &settlements[i + 1..] {
            let is_new = !links.contains(&(a, b)) && !links.contains(&(b, a));
            if is_new && cell_distance(a, b, grid.size) < extra_distance {
                links.push((a, b));
            }
        }
    }
    // Route short links first, so long ones can reuse them.
    links.sort_by(|&(a, b), &(c, d)| {
        cell_distance(a, b, grid.size).total_cmp(&cell_distance(c, d, grid.size)).then((a, b).cmp(&(c, d)))
    });

    let mut is_road = vec![false; grid.size * grid.size];
    let mut roads = Vec::new();
    for (from, to) in links {
        let Some(route) = find_route(config, grid, from, to, &is_road) else { continue };
        for &index in &route {
            is_road[index] = true;
        }
        let points = route.iter().map(|&index| grid.position(index)).collect();
        roads.push(chaikin_smooth(points, SMOOTHING_ITERATIONS));
    }
    roads
}

/// Links forming a minimum spanning tree over the settlements (Prim's algorithm),
/// using straight-line distance.
fn spanning_tree(grid: &MapGrid, settlements: &[usize]) -> Vec<(usize, usize)> {
    let Some((&first, rest)) = settlements.split_first() else {
        return Vec::new();
    };
    let mut in_tree = vec![first];
    let mut outside = rest.to_vec();
    let mut links = Vec::new();
    while !outside.is_empty() {
        let (tree_site, outside_position) = in_tree
            .iter()
            .flat_map(|&a| outside.iter().enumerate().map(move |(i, &b)| (a, i, b)))
            .min_by(|x, y| cell_distance(x.0, x.2, grid.size).total_cmp(&cell_distance(y.0, y.2, grid.size)))
            .map(|(a, i, _)| (a, i))
            .expect("outside is not empty");
        let joined = outside.swap_remove(outside_position);
        links.push((tree_site, joined));
        in_tree.push(joined);
    }
    links
}

/// The cheapest route between two map points using A* search, as a list of
/// map point indices, or `None` if water separates them.
fn find_route(config: &WorldGenConfig, grid: &MapGrid, from: usize, to: usize, is_road: &[bool]) -> Option<Vec<usize>> {
    let cell = grid.cell_size;
    let step_cost = |a: usize, b: usize| {
        let length = cell_distance(a, b, grid.size) * cell;
        let climb = (grid.land_height[b] - grid.land_height[a]).abs();
        let wander = 1.0 + WANDER * cell_hash(b, config.seed);
        let mut cost = length * (wander + config.road_slope_cost * climb / length);
        if grid.is_river[b] {
            cost += config.road_river_crossing_cost;
        }
        if is_road[b] {
            cost *= config.road_reuse_discount;
        }
        cost
    };
    // Never overestimates, because no step can cost less than discounted flat road.
    let estimate = |index: usize| cell_distance(index, to, grid.size) * cell * config.road_reuse_discount;

    let mut cost_so_far = vec![f32::INFINITY; grid.size * grid.size];
    let mut came_from = vec![usize::MAX; grid.size * grid.size];
    let mut queue = BinaryHeap::from([MinEntry { key: estimate(from), index: from }]);
    cost_so_far[from] = 0.0;

    while let Some(MinEntry { index, .. }) = queue.pop() {
        if index == to {
            let mut route = vec![to];
            while let Some(&last) = route.last()
                && last != from
            {
                route.push(came_from[last]);
            }
            route.reverse();
            return Some(route);
        }
        for next in neighbours(index, grid.size) {
            if grid.is_water(next) {
                continue;
            }
            let cost = cost_so_far[index] + step_cost(index, next);
            if cost < cost_so_far[next] {
                cost_so_far[next] = cost;
                came_from[next] = index;
                queue.push(MinEntry { key: cost + estimate(next), index: next });
            }
        }
    }
    None
}
