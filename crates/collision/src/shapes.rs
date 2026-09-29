//! The world's solid shapes, as Rapier colliders: each chunk's ground (a
//! heightfield), its trees and rocks, and the walls and buildings.
//!
//! Rapier is Y-up, so a world point `(x, y)` at height `h` is `(x, h, y)`
//! here. (That's a mirror image of the world; nothing measured cares.)

use rapier3d::math::{Pose, Vector};
use rapier3d::parry::utils::Array2;
use rapier3d::prelude::{Collider, ColliderBuilder};
use venture_structures::bridges::bridge_pieces;
use venture_structures::buildings::{BuildingStyle, building_walls};
use venture_world::{BUSH_SCALE, BakedBridge, BakedBuilding, BakedWall, CHUNK_SIZE, Chunk, Prop, PropKind};

/// How far below the ground walls reach (so no slope leaves a gap under them), in metres.
const FOOTING: f32 = 0.5;
/// How much further than its storeys a building's walls reach up (to its eaves), in metres.
const EAVES: f32 = 0.5;
/// How much each piece of a wall overlaps the next (so there are no seams at bends), in metres.
const OVERLAP: f32 = 0.05;
/// How tall a tree trunk's collider is, in metres (taller than anyone jumps).
const TRUNK_HEIGHT: f32 = 4.0;

/// A point in the world (`x`, `y` across; `height` up) as Rapier's vector.
pub fn to_rapier(x: f32, y: f32, height: f32) -> Vector {
    Vector::new(x, height, y)
}

/// A chunk's ground, as a heightfield through its corner heights.
pub fn ground(chunk: &Chunk) -> Collider {
    let corners = CHUNK_SIZE as usize + 1;
    // Rows run along the world's y (Rapier's z), columns along x.
    let heights = Array2::from_fn(corners, corners, |row, column| chunk.corner_height(column, row));
    let size = CHUNK_SIZE as f32;
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    // (A heightfield is centred on its position.)
    let middle = to_rapier(origin_x as f32 + size / 2.0, origin_y as f32 + size / 2.0, 0.0);
    ColliderBuilder::heightfield(heights, Vector::new(size, 1.0, size)).translation(middle).build()
}

/// A prop's collider, if it's in the way: a capsule round a tree's trunk, or
/// a ball for a boulder. Bushes can be pushed through.
pub fn prop(prop: &Prop) -> Option<Collider> {
    let trunk = |radius: f32| {
        let radius = radius * prop.scale;
        let half = TRUNK_HEIGHT / 2.0 - radius;
        ColliderBuilder::capsule_y(half, radius)
            .translation(to_rapier(prop.x, prop.y, prop.ground_height + TRUNK_HEIGHT / 2.0))
            .build()
    };
    match prop.kind {
        PropKind::Tree { .. } if prop.scale < BUSH_SCALE => None,
        // Young, mature and old trees' trunks.
        PropKind::Tree { variant, .. } => Some(trunk([0.3, 0.45, 0.6][usize::from(variant.min(2))])),
        PropKind::DeadTree => Some(trunk(0.35)),
        PropKind::Boulder => {
            let radius = 1.1 * prop.scale;
            Some(ColliderBuilder::ball(radius).translation(to_rapier(prop.x, prop.y, prop.ground_height)).build())
        }
    }
}

/// A box standing on the ground along a line: from `from` to `to` (world
/// `(x, y)`), `half` thick each side, from `bottom` up to `top`.
fn upright_box(from: (f32, f32), to: (f32, f32), half: f32, bottom: f32, top: f32) -> Collider {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    // Turning about Rapier's y takes its x to (cos, 0, -sin): towards the line.
    let angle = (-dy).atan2(dx);
    let middle = to_rapier((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0, (bottom + top) / 2.0);
    ColliderBuilder::cuboid(length / 2.0 + OVERLAP, (top - bottom) / 2.0, half)
        .position(Pose::new(middle, Vector::new(0.0, angle, 0.0)))
        .build()
}

/// A bridge's colliders: a convex hull for each solid piece (its deck is walked on).
pub fn bridge(bridge: &BakedBridge) -> Vec<Collider> {
    bridge_pieces(bridge)
        .into_iter()
        .filter(|piece| !piece.decoration)
        .filter_map(|piece| {
            let points: Vec<Vector> = piece.corners.iter().map(|c| Vector::new(c.x, c.y, c.z)).collect();
            let builder = ColliderBuilder::convex_hull(&points)?;
            Some(builder.user_data(if piece.walkable { crate::DECK } else { 0 }).build())
        })
        .collect()
}

/// A wall's colliders: a box for each stretch between two samples, except where it's open.
pub fn wall(wall: &BakedWall) -> Vec<Collider> {
    let half = wall.kind.half_width();
    wall.samples
        .windows(2)
        .filter(|pair| !pair[0].open && !pair[1].open)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let bottom = a.ground.min(b.ground) - FOOTING;
            let top = a.ground.max(b.ground) + wall.height;
            upright_box((a.x, a.y), (b.x, b.y), half, bottom, top)
        })
        .collect()
}

/// A building's colliders: a box for each solid stretch of each of its
/// walls (so its doorways are open). Its floor is the levelled ground.
pub fn building(building: &BakedBuilding, style: &BuildingStyle) -> Vec<Collider> {
    let Some((frame, walls)) = building_walls(building, style) else { return Vec::new() };
    let scaled = style.scaled();
    let bottom = building.lowest.min(building.floor) - FOOTING;
    let top = building.floor + scaled.storey * building.plan.floors.max(1) as f32 + EAVES;
    let mut colliders = Vec::new();
    for band in &walls {
        let way = (band.to - band.from).normalize_or_zero();
        for (start, end) in band.solid() {
            let (from, to) = (frame.world(band.from + way * start), frame.world(band.from + way * end));
            // (The pieces already reach round the corners, so they needn't overlap more.)
            let inset = OVERLAP.min((end - start) / 2.0);
            let (from, to) =
                (from + (to - from).normalize_or_zero() * inset, to - (to - from).normalize_or_zero() * inset);
            colliders.push(upright_box((from.x, from.y), (to.x, to.y), band.half, bottom, top));
        }
    }
    colliders
}
