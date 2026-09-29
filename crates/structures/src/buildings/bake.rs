//! Baking buildings into the world: their floor's level (the ground
//! under them is levelled to it, blending into the land around), the
//! cells their walls block (with their doorways open), and the cells
//! cleared of trees and rocks.

use glam::Vec2;
use venture_world::{BakedBuilding, BuildingPlan, WallRef};

use super::plan::{Footprint, contains, doors_in, snap_inner};
use super::style::BuildingStyle;

/// How far round a building the ground blends back into the land, in metres.
const LEVEL_BLEND: f32 = 2.5;
/// How far past its outline the ground is quite level, in metres.
const LEVEL_EDGE: f32 = 0.4;
/// Walls block cells this far past their faces (so characters don't
/// clip into them), in metres.
const WALL_REACH: f32 = 0.25;

/// A wall as it blocks cells: along its middle from one point to another,
/// its half-thickness, and its doorways (from and to, along it).
type WallBand = (Vec2, Vec2, f32, Vec<(f32, f32)>);

/// Reads the authored buildings file.
pub fn parse_buildings(text: &str) -> Result<Vec<BuildingPlan>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Problems with authored buildings (ids missing or twice, and each plan's).
pub fn check_buildings(plans: &[BuildingPlan], styles: &[BuildingStyle]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, plan) in plans.iter().enumerate() {
        if plan.id.trim().is_empty() {
            problems.push(format!("building {} has no id", i + 1));
        } else if plans[..i].iter().any(|other| other.id == plan.id) {
            problems.push(format!("building \"{}\" is there twice", plan.id));
        }
        problems.extend(super::check_plan(plan, styles.iter().find(|style| style.id == plan.style)));
    }
    problems
}

/// Bakes a building: its floor is the ground's average height under it,
/// and the lowest the ground goes under it is kept (for its footing).
pub fn bake_building(plan: &BuildingPlan, ground: impl Fn(f32, f32) -> f32) -> BakedBuilding {
    let points: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let (low, high) = points.iter().fold((points[0], points[0]), |(low, high), &p| (low.min(p), high.max(p)));
    let mut heights = Vec::new();
    let mut y = low.y;
    while y <= high.y {
        let mut x = low.x;
        while x <= high.x {
            if contains(&points, Vec2::new(x, y)) {
                heights.push(ground(x, y));
            }
            x += 1.0;
        }
        y += 1.0;
    }
    heights.extend(points.iter().map(|p| ground(p.x, p.y)));
    let floor = heights.iter().sum::<f32>() / heights.len() as f32;
    let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
    BakedBuilding { plan: plan.clone(), floor, lowest }
}

/// A building's world outline (as drawn).
fn outline(building: &BakedBuilding) -> Vec<Vec2> {
    building.plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
}

/// How far a point is from a polygon's edge (0 inside it).
fn distance_outside(polygon: &[Vec2], point: Vec2) -> f32 {
    if contains(polygon, point) {
        return 0.0;
    }
    (0..polygon.len())
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            let run = b - a;
            let t = ((point - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (a + run * t).distance(point)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The ground's height at `(x, y)` with the building's ground levelled
/// (`natural` there otherwise), or `None` if it's too far to matter.
pub fn levelled(building: &BakedBuilding, x: f32, y: f32, natural: f32) -> Option<f32> {
    let distance = distance_outside(&outline(building), Vec2::new(x, y));
    if distance >= LEVEL_EDGE + LEVEL_BLEND {
        return None;
    }
    let share = ((distance - LEVEL_EDGE) / LEVEL_BLEND).clamp(0.0, 1.0);
    let blend = share * share * (3.0 - 2.0 * share);
    Some(building.floor + (natural - building.floor) * blend)
}

/// The world's cells (1 m squares, by their corner) a building's
/// levelling, clearing (`reach` metres round it) and walls touch: from and
/// to, inclusive.
pub fn cell_bounds(building: &BakedBuilding, reach: f32) -> ((i32, i32), (i32, i32)) {
    let points = outline(building);
    let (low, high) = points.iter().fold((points[0], points[0]), |(low, high), &p| (low.min(p), high.max(p)));
    let pad = reach.max(LEVEL_EDGE + LEVEL_BLEND) + 1.0;
    let (low, high) = (low - pad, high + pad);
    ((low.x.floor() as i32, low.y.floor() as i32), (high.x.floor() as i32, high.y.floor() as i32))
}

/// The cells a building's walls stand on (so can't be walked through),
/// except its doorways. Inside is open.
pub fn building_cells(building: &BakedBuilding, style: &BuildingStyle) -> Vec<(i32, i32)> {
    let style = &style.scaled();
    let plan = &super::plan::anticlockwise(&building.plan);
    let Some(footprint) = Footprint::of(plan, style.wall_thickness) else { return Vec::new() };
    let frame = footprint.frame;
    // Every wall, along its middle, with its half-thickness and its doorways (along it).
    let mut walls: Vec<WallBand> = Vec::new();
    let doors_on = |wall: WallRef| -> Vec<f32> {
        plan.doors.iter().filter(|door| door.wall == wall).map(|door| door.along).collect()
    };
    let half = style.wall_thickness / 2.0;
    for i in 0..footprint.outline.len() {
        let (a, b) = footprint.edge(i);
        let inward = (b - a).normalize_or(Vec2::X).perp() * half;
        let doors = doors_in(a.distance(b), &doors_on(WallRef::Outer(i as u32)), style);
        walls.push((a + inward, b + inward, half, doors.iter().map(|d| (d.from, d.to)).collect()));
    }
    for (i, &(from, to)) in plan.inner_walls.iter().enumerate() {
        let (from, to) = (frame.local(Vec2::from(from)), frame.local(Vec2::from(to)));
        let (from, to) = (snap_inner(&footprint, style, from), snap_inner(&footprint, style, to));
        let doors = doors_in(from.distance(to), &doors_on(WallRef::Inner(i as u32)), style);
        walls.push((from, to, style.inner_thickness / 2.0, doors.iter().map(|d| (d.from, d.to)).collect()));
    }
    let ((x0, y0), (x1, y1)) = cell_bounds(building, 0.0);
    let mut cells = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let centre = frame.local(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            let blocked = walls.iter().any(|&(a, b, half, ref doors)| {
                let run = b - a;
                let length = run.length().max(1e-4);
                let along = (centre - a).dot(run / length);
                let across = (centre - a).perp_dot(run / length).abs();
                let on = (-half..length + half).contains(&along) && across <= half + WALL_REACH;
                let in_doorway = doors.iter().any(|&(from, to)| along > from + 0.05 && along < to - 0.05);
                on && !in_doorway
            });
            if blocked {
                cells.push((x, y));
            }
        }
    }
    cells
}

/// The cells within `reach` metres of a building (or under it), cleared of trees and rocks.
pub fn building_clearing(building: &BakedBuilding, reach: f32) -> Vec<(i32, i32)> {
    let points = outline(building);
    let ((x0, y0), (x1, y1)) = cell_bounds(building, reach);
    let mut cells = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            if distance_outside(&points, Vec2::new(x as f32 + 0.5, y as f32 + 0.5)) <= reach {
                cells.push((x, y));
            }
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::tests::{cottage, styles};
    use venture_world::RoofShape;

    #[test]
    fn buildings_are_baked_level_on_the_average_ground() {
        let plan = cottage("stone", RoofShape::Gable, 1).plan;
        let baked = bake_building(&plan, |x, _| x * 0.1);
        assert!((baked.floor - 0.45).abs() < 0.1, "{}", baked.floor);
        assert!(baked.lowest <= 0.0 + 1e-4);
        // Level under it, the land's own height well away, and between in between.
        assert_eq!(levelled(&baked, 4.0, 2.0, 0.4), Some(baked.floor));
        assert_eq!(levelled(&baked, 30.0, 2.0, 3.0), None);
        let between = levelled(&baked, 9.0 + LEVEL_EDGE + LEVEL_BLEND / 2.0, 2.0, 5.0).unwrap();
        assert!(between > baked.floor && between < 5.0);
    }

    #[test]
    fn walls_block_but_doorways_and_inside_are_open() {
        let styles = styles();
        let building = cottage("stone", RoofShape::Gable, 1);
        let cells = building_cells(&building, &styles[0]);
        // The cottage is 9 × 5.5 m from (0, 0): its south wall runs along y = 0.
        assert!(cells.contains(&(1, 0)) && cells.contains(&(7, 0)), "the south wall");
        assert!(!cells.contains(&(2, 2)), "inside is open");
        // Its door is 30% along the south wall (2.7 m), and in the inner wall at x = 4.5.
        assert!(!cells.contains(&(2, 0)), "the front door is open");
        assert!(cells.contains(&(4, 1)) || cells.contains(&(4, 4)), "the inner wall");
        assert!(!cells.contains(&(4, 2)) || !cells.contains(&(4, 3)), "with a doorway");
        let cleared = building_clearing(&building, 1.5);
        assert!(cleared.contains(&(-1, 3)) && !cleared.contains(&(-3, 3)));
    }
}
