//! `--local` mode (desktop only): runs the real game server inside the
//! client, connected through an in-memory link instead of the network. For
//! working on the game without starting a separate server.

use std::path::PathBuf;

use bevy::prelude::*;
use venture_server::{GameServer, Link, ProfileStore, ServerConfig, ServerWorld};

use crate::dev::settings::DevSettings;
use crate::network::LocalServerLink;
use crate::network::launch::ServerChoice;

/// Where `--local` mode keeps profiles.
const PROFILE_FOLDER: &str = "local_data/profiles";

/// The server settings file `--local` mode reads its game rules from, so it
/// plays by the same rules as the real server.
const CONFIG_FILE: &str = "server.ron";

/// Starts the in-process server when the client runs in `--local` mode.
pub struct LocalServerPlugin;

impl Plugin for LocalServerPlugin {
    fn build(&self, app: &mut App) {
        let ServerChoice::Local { world_folder } = app.world().resource::<ServerChoice>().clone() else { return };
        let dev = app.world().resource::<DevSettings>();
        let spawn_override = dev.start_position;
        // Automated runs get a fresh profile each time, so they start at the requested spot.
        let profile_folder = if dev.automated {
            std::env::temp_dir().join(format!("venture-automated-{}", std::process::id()))
        } else {
            PathBuf::from(PROFILE_FOLDER)
        };

        let world = match ServerWorld::load(&world_folder) {
            Ok(world) => world,
            Err(error) => {
                error!("The local server can't start: {error}");
                return;
            }
        };
        let profiles = match ProfileStore::new(&profile_folder) {
            Ok(profiles) => profiles,
            Err(error) => {
                error!("The local server can't use {}: {error}", profile_folder.display());
                return;
            }
        };
        let rules = match ServerConfig::load(CONFIG_FILE.as_ref()) {
            Ok(config) => config.rules(),
            Err(error) => {
                error!("The local server can't read its settings: {error}");
                return;
            }
        };
        let (link_sender, link) = Link::new();
        app.insert_resource(LocalServerLink(link_sender));
        GameServer { world, profiles, link, rules, spawn_override }.install(app);
    }
}
