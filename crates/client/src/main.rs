//! The Venture game client.
//!
//! The client is a view of the game on the server: it connects (see
//! [`network`]), streams the baked world's chunks around the camera, shows
//! everyone in the game, and sends the player's commands.
//!
//! Desktop options: `--connect http://host:port` to play on a server (the
//! default is `http://127.0.0.1:8080`), or `--local` to run a server inside
//! the client for development.
//!
//! The code is grouped by concern, one folder (and one plugin) per concern:
//! [`network`], [`content`], [`world`], [`characters`], [`camera`], [`interaction`],
//! [`ui`], [`sound`], [`dev`], plus [`common`] helpers.

// Bevy systems receive everything they use as parameters, so long parameter
// lists and query types are normal here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod camera;
mod characters;
mod common;
mod content;
mod dev;
mod interaction;
mod network;
mod sound;
mod ui;
mod world;

use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    // The content's model files are read from memory, once downloaded (before the asset plugin).
    content::ContentAssets::default().register(&mut app);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Venture".into(),
            // On the web, size the game to fill its canvas's parent element.
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(dev::settings::DevSettings::from_environment())
    .insert_resource(network::launch::ServerChoice::from_environment())
    .add_plugins((
        network::NetworkPlugin,
        content::ContentPlugin,
        world::WorldPlugin,
        characters::CharactersPlugin,
        camera::CameraPlugin,
        interaction::InteractionPlugin,
        ui::UiPlugin,
        sound::SoundPlugin,
        dev::DevPlugin,
    ))
    .run();
}
