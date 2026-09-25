//! The world as the client shows it: the baked world's chunks, loaded around
//! the camera and turned into terrain, liquids and props, under the sky and sun.

pub mod lighting;
pub mod props;
pub mod query;
pub mod source;
pub mod streaming;
pub mod terrain_material;
pub mod terrain_mesh;

use bevy::prelude::*;

/// Loads and shows the world.
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            source::WorldSourcePlugin,
            lighting::LightingPlugin,
            terrain_material::TerrainMaterialPlugin,
            streaming::ChunkStreamingPlugin,
        ));
    }
}
