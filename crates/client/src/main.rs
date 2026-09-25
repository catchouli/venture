//! The Venture game client.
//!
//! For now this is a prototype: it generates the world locally, streams
//! terrain chunks around the camera, and lets the player click to run around.
//! Later, chunk data will come from the server instead.

mod camera;
mod character;
mod chunk_streaming;
mod click_to_move;
mod coords;
mod cursor;
mod debug_hud;
mod dev_settings;
mod mesh_data;
mod move_marker;
mod palette;
mod player;
mod procedural_image;
mod prop_mesh;
mod screenshot;
mod terrain_material;
mod terrain_mesh;
mod terrain_query;
mod world;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Venture".into(),
                // On the web, size the game to fill its canvas's parent element.
                fit_canvas_to_parent: true,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(dev_settings::DevSettings::from_environment())
        .add_plugins((
            world::WorldPlugin,
            terrain_material::TerrainMaterialPlugin,
            chunk_streaming::ChunkStreamingPlugin,
            character::CharacterPlugin,
            player::PlayerPlugin,
            camera::CameraPlugin,
            click_to_move::ClickToMovePlugin,
            move_marker::MoveMarkerPlugin,
            cursor::CursorPlugin,
            debug_hud::DebugHudPlugin,
            screenshot::ScreenshotPlugin,
        ))
        .run();
}
