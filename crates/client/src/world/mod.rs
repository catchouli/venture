//! The world as the client shows it: the baked world's chunks, loaded around
//! the camera and turned into terrain, water, lava, props and walls, under the sky and sun.

pub mod buildings;
pub mod collision;
pub mod daylight;
pub mod lighting;
pub mod props;
pub mod query;
pub mod sky;
pub mod source;
pub mod streaming;
pub mod surfaces;
pub mod terrain_material;
pub mod terrain_mesh;
pub mod time_of_day;
pub mod trees;
pub mod walls;
pub mod water_material;

use bevy::prelude::*;

/// Loads and shows the world.
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            source::WorldSourcePlugin,
            lighting::LightingPlugin,
            sky::SkyPlugin,
            time_of_day::TimeOfDayPlugin,
            terrain_material::TerrainMaterialPlugin,
            water_material::WaterMaterialPlugin,
            streaming::ChunkStreamingPlugin,
            trees::TreesPlugin,
            walls::WallsPlugin,
            buildings::BuildingsPlugin,
            surfaces::SurfacesPlugin,
            collision::CollisionPlugin,
        ));
    }
}
