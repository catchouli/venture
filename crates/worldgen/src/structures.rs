//! Authored structures (walls and fences, from `venture_structures`) baked
//! against this world: the ground's height along them, and gaps where roads
//! cross. The `bake` tool and the editor both bake them here, so what the
//! editor shows is what the bake makes.

use venture_structures::bridges::bake_bridge;
use venture_structures::{BakedWall, WallDef, bake_wall};
use venture_world::{BakedBridge, BridgePlan};

use crate::WorldGenerator;

/// Walls open where a road passes within this much of the road's edge, in metres.
const ROAD_GAP_MARGIN: f32 = 0.5;

/// Bakes a bridge against the world (its deck's ends on the ground).
pub fn bake_authored_bridge(world: &WorldGenerator, plan: &BridgePlan) -> BakedBridge {
    bake_bridge(plan, |x, y| world.ground_height(x, y))
}

/// Bakes an authored wall against the world.
pub fn bake_authored_wall(world: &WorldGenerator, def: &WallDef) -> BakedWall {
    let road = world.config().road_half_width + ROAD_GAP_MARGIN;
    bake_wall(def, |x, y| world.ground_height(x, y), |x, y| world.road_distance(x, y, road).is_some())
}
