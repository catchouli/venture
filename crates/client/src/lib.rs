//! The Venture game client, as a library: the game's plugins, grouped by
//! concern, one folder (and one plugin) per concern: [`network`],
//! [`content`], [`world`], [`characters`], [`camera`], [`interaction`],
//! [`ui`], [`sound`], [`dev`], plus [`common`] helpers.
//!
//! The game itself is the `venture_client` binary ([`run`]). The editor
//! (`venture_editor`) uses some of the same plugins, so the world and
//! characters look the same in both.

// Bevy systems receive everything they use as parameters, so long parameter
// lists and query types are normal here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod camera;
pub mod characters;
pub mod common;
pub mod content;
pub mod dev;
pub mod interaction;
pub mod map;
pub mod network;
pub mod sound;
pub mod ui;
pub mod world;

use bevy::prelude::*;

/// Runs the game.
pub fn run() {
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
        map::MapPlugin,
        sound::SoundPlugin,
        dev::DevPlugin,
    ))
    .run();
}
