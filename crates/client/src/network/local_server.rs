//! `--local` mode (desktop only): runs the real game server inside the
//! client, connected through an in-memory link instead of the network. For
//! working on the game without starting a separate server.

use std::path::PathBuf;

use bevy::prelude::*;
use venture_content::ContentPack;
use venture_server::{
    ContentSetup, GameServer, Link, ProfileKey, ProfileStore, ServerConfig, ServerWorld, SharedClientContent,
};

use crate::content::LocalContent;

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
        let ServerChoice::Local { world_folder, new_character, admin } = app.world().resource::<ServerChoice>().clone()
        else {
            return;
        };
        let dev = app.world().resource::<DevSettings>();
        let spawn_override = dev.start_position;
        let dev_profile = dev.profile.clone();
        // Automated runs get a fresh profile each time, so they start at the
        // requested spot, unless they name one (VENTURE_PROFILE): then it's
        // kept between runs, so a check can go on over several runs.
        let profile_folder = if dev.automated {
            let run = dev.profile.clone().unwrap_or_else(|| std::process::id().to_string());
            std::env::temp_dir().join(format!("venture-automated-{run}"))
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
        let config = match ServerConfig::load(CONFIG_FILE.as_ref()) {
            Ok(config) => config,
            Err(error) => {
                error!("The local server can't read its settings: {error}");
                return;
            }
        };
        let pack = match ContentPack::load(&config.paths.content) {
            Ok(pack) => pack,
            Err(errors) => {
                for problem in &errors {
                    error!("{problem}");
                }
                error!("The local server can't start: the content has {} problem(s).", errors.len());
                return;
            }
        };
        let client_content = SharedClientContent::default();
        app.insert_resource(LocalContent(client_content.clone()));
        let content = ContentSetup { folder: config.paths.content.clone(), pack, client: client_content, watch: true };

        let (link_sender, link) = Link::new();
        app.insert_resource(LocalServerLink(link_sender));
        // `--new-character`: this client's character goes, so it's asked for a new one.
        if new_character {
            let slot = super::token_slot(dev_profile, true);
            profiles.remove(&ProfileKey::from_token(&super::token::load_or_create(slot.as_deref())));
        }
        let admins = config.admins.clone();
        let everyone_is_admin = admin;
        let item_cooldown = config.player.item_cooldown;
        let party = config.party.clone();
        GameServer {
            world,
            profiles,
            link,
            rules: config.rules(),
            item_cooldown,
            party,
            content,
            spawn_override,
            admins,
            everyone_is_admin,
        }
        .install(app);
    }
}
