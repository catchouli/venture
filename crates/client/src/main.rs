//! The Venture game client.
//!
//! The client is a view of the game on the server: it connects (see
//! [`network`]), streams the baked world's chunks around the camera, shows
//! everyone in the game, and sends the player's commands.
//!
//! Desktop options: `--connect http://host:port` to play on a server (the
//! default is `http://127.0.0.1:8080`), or `--local` to run a server inside
//! the client for development.

// Bevy systems receive everything they use as parameters, so long parameter
// lists and query types are normal here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod camera;
mod character;
mod chat;
mod chunk_streaming;
mod click_to_move;
mod connection;
mod connection_overlay;
mod coords;
mod cursor;
mod debug_hud;
mod dev_settings;
mod launch;
#[cfg(not(target_arch = "wasm32"))]
mod local_server;
mod mesh_data;
mod move_marker;
mod name_prompt;
mod nameplates;
mod network;
mod palette;
mod player;
mod procedural_image;
mod prop_mesh;
mod remote_players;
mod screenshot;
mod terrain_material;
mod terrain_mesh;
mod terrain_query;
mod text_input;
mod token;
mod world;
mod world_source;

use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Venture".into(),
            // On the web, size the game to fill its canvas's parent element.
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(dev_settings::DevSettings::from_environment())
    .insert_resource(launch::ServerChoice::from_environment());
    // Before anything that connects: in `--local` mode this starts the server.
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(local_server::LocalServerPlugin);
    app.add_plugins((network::NetworkPlugin, world_source::WorldSourcePlugin))
        .add_plugins((
            world::WorldPlugin,
            terrain_material::TerrainMaterialPlugin,
            chunk_streaming::ChunkStreamingPlugin,
            character::CharacterPlugin,
            player::PlayerPlugin,
            remote_players::RemotePlayersPlugin,
            nameplates::NameplatesPlugin,
            camera::CameraPlugin,
            click_to_move::ClickToMovePlugin,
            move_marker::MoveMarkerPlugin,
        ))
        .add_plugins((
            text_input::TextInputPlugin,
            chat::ChatPlugin,
            name_prompt::NamePromptPlugin,
            connection_overlay::ConnectionOverlayPlugin,
            cursor::CursorPlugin,
            debug_hud::DebugHudPlugin,
            screenshot::ScreenshotPlugin,
        ))
        .run();
}
