//! Questions about the loaded terrain: how high the ground is, where a ray
//! from the camera hits it (or the water on it), and where characters can walk.
//!
//! These read the loaded chunk data (from the server's baked world).

use bevy::math::{Ray3d, Vec3};
use venture_world::{CellPos, FluidKind};

use crate::common::coords::{from_bevy, to_bevy};
use crate::world::streaming::ChunkStore;

/// How far a ray is followed looking for the ground, in metres.
const RAY_MAX_DISTANCE: f32 = 600.0;
/// Step size when following a ray, in metres.
const RAY_STEP: f32 = 0.5;

/// Ground height at world point `(x, y)`, if its chunk is loaded.
pub fn ground_height(store: &ChunkStore, x: f32, y: f32) -> Option<f32> {
    let cell = CellPos::containing(x, y);
    let chunk = store.0.get(&cell.chunk())?;
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    Some(chunk.height_at(x - origin_x as f32, y - origin_y as f32))
}

/// The water's surface at world point `(x, y)`, if there's water there
/// (and its chunk is loaded).
pub fn water_level(store: &ChunkStore, x: f32, y: f32) -> Option<f32> {
    let cell = CellPos::containing(x, y);
    let chunk = store.0.get(&cell.chunk())?;
    let (local_x, local_y) = cell.local();
    chunk.cell(local_x, local_y).fluid.filter(|fluid| fluid.kind == FluidKind::Water).map(|fluid| fluid.level)
}

/// Where a ray first hits loaded ground, if it does. Found by stepping along
/// the ray until it goes below the ground, then narrowing down by bisection.
pub fn raycast_ground(store: &ChunkStore, ray: Ray3d) -> Option<Vec3> {
    raycast(ray, |x, y| ground_height(store, x, y))
}

/// Where a ray first hits loaded ground or water (the water's surface, not
/// the bed under it), if it does.
pub fn raycast_surface(store: &ChunkStore, ray: Ray3d) -> Option<Vec3> {
    raycast(ray, |x, y| {
        let ground = ground_height(store, x, y)?;
        Some(water_level(store, x, y).map_or(ground, |water| water.max(ground)))
    })
}

/// Where a ray first goes below a surface given by `height`.
fn raycast(ray: Ray3d, height: impl Fn(f32, f32) -> Option<f32>) -> Option<Vec3> {
    let above_ground = |distance: f32| {
        let point = ray.get_point(distance);
        let (x, y) = from_bevy(point);
        height(x, y).map(|ground| point.y > ground)
    };
    let mut previous = 0.0;
    let mut distance = RAY_STEP;
    while distance < RAY_MAX_DISTANCE {
        if above_ground(distance) == Some(false) {
            let (mut above, mut below) = (previous, distance);
            for _ in 0..16 {
                let middle = (above + below) / 2.0;
                if above_ground(middle) == Some(false) {
                    below = middle;
                } else {
                    above = middle;
                }
            }
            let (x, y) = from_bevy(ray.get_point(below));
            return Some(to_bevy(x, y, height(x, y)?));
        }
        previous = distance;
        distance += RAY_STEP;
    }
    None
}

/// How high to show a character whose feet are at `feet` (metres) at `(x,
/// y)`: on the ground as it's drawn when they're about on it (the solid
/// ground's triangles and the drawn ground differ by a few centimetres), and
/// at their feet otherwise (jumping, falling, or on a wall).
pub fn shown_height(store: &ChunkStore, x: f32, y: f32, feet: f32) -> f32 {
    const ON_THE_GROUND: f32 = 0.15;
    match ground_height(store, x, y) {
        Some(ground) if (feet - ground).abs() < ON_THE_GROUND => ground,
        _ => feet,
    }
}
