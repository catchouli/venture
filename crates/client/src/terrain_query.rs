//! Questions about the loaded terrain: how high the ground is, where a ray
//! from the camera hits it, and where characters can walk.
//!
//! These read loaded chunk data, not the world generator, because in the
//! full game chunk data will come from the server.

use bevy::math::{Ray3d, Vec3};
use venture_world::{CHUNK_SIZE, CellPos};
use venture_worldgen::WorldGenerator;

use crate::chunk_streaming::{ChunkStore, ObstacleStore};
use crate::coords::{from_bevy, to_bevy};

/// How far a ray is followed looking for the ground, in metres.
const RAY_MAX_DISTANCE: f32 = 600.0;
/// Step size when following a ray, in metres.
const RAY_STEP: f32 = 0.5;

/// Ground height at world point `(x, y)` from loaded chunks, if its chunk is loaded.
pub fn loaded_ground_height(store: &ChunkStore, x: f32, y: f32) -> Option<f32> {
    let cell = CellPos::containing(x, y);
    let chunk = store.0.get(&cell.chunk())?;
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    Some(chunk.height_at(x - origin_x as f32, y - origin_y as f32))
}

/// Ground height at `(x, y)`: from loaded chunks, or from the generator
/// while the chunk is still loading.
pub fn ground_height(store: &ChunkStore, world: &WorldGenerator, x: f32, y: f32) -> f32 {
    loaded_ground_height(store, x, y).unwrap_or_else(|| world.ground_height(x, y))
}

/// Where a ray first hits loaded ground, if it does. Found by stepping along
/// the ray until it goes below the ground, then narrowing down by bisection.
pub fn raycast_ground(store: &ChunkStore, ray: Ray3d) -> Option<Vec3> {
    let above_ground = |distance: f32| {
        let point = ray.get_point(distance);
        let (x, y) = from_bevy(point);
        loaded_ground_height(store, x, y).map(|ground| point.y > ground)
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
            return Some(to_bevy(x, y, loaded_ground_height(store, x, y)?));
        }
        previous = distance;
        distance += RAY_STEP;
    }
    None
}

/// Whether terrain cell `(x, y)` is blocked. Unloaded cells count as blocked,
/// so paths stay on loaded ground.
pub fn is_blocked(obstacles: &ObstacleStore, x: i32, y: i32) -> bool {
    let cell = CellPos::new(x, y);
    let Some(chunk) = obstacles.0.get(&cell.chunk()) else { return true };
    let (local_x, local_y) = cell.local();
    debug_assert!(local_x < CHUNK_SIZE as usize && local_y < CHUNK_SIZE as usize);
    chunk.is_blocked(local_x, local_y)
}
