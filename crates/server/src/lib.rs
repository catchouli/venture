//! The authoritative game server.
//!
//! The server owns the game. Clients send it commands, and it decides what
//! happens and tells them. See `docs/client-server.md`.
//!
//! # How a client's message becomes a change to the game
//!
//! Every change to the live game goes through one path, so there's exactly
//! one place to check and secure:
//!
//! 1. [`link`]: a transport (WebSocket, or in-memory in `--local` mode)
//!    delivers raw bytes from a connection.
//! 2. [`requests`]: **the gate.** Decodes the bytes, enforces size and rate
//!    limits, checks the message is allowed at this point in the connection's
//!    life, validates its contents, and turns it into a typed request.
//!    Nothing else reads client messages.
//! 3. [`sessions`]: handles joining and leaving (the only code that creates
//!    and removes players, and loads and saves their profiles through
//!    [`profiles`]).
//! 4. [`gameplay`]: handles in-game requests and runs the simulation (the only
//!    code that changes players in the game).
//!
//! Content scripts follow the same rule from the other side: a script can
//! only queue commands ([`scripting`]), which [`npcs`] checks and applies.
//! [`content`] loads the content pack and reloads it when it changes.
//!
//! The game runs at a fixed tick rate in Bevy's `FixedUpdate` schedule.

// Bevy systems receive everything they use as parameters, so long parameter
// lists and query types are normal here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod config;
pub mod content;
pub mod entities;
pub mod gameplay;
#[cfg(feature = "http")]
pub mod http;
pub mod link;
mod names;
pub mod npcs;
pub mod profiles;
pub mod requests;
pub mod scripting;
pub mod sessions;
pub mod world;

use std::time::Duration;

use bevy::prelude::*;
use venture_simulation::{Rules, TICK_RATE};

pub use config::ServerConfig;
pub use content::{ContentSetup, SharedClientContent};
pub use link::{ConnectionId, Link, LinkEvent, LinkSender};
pub use profiles::ProfileStore;
pub use world::ServerWorld;

/// Everything the game server needs to run.
pub struct GameServer {
    /// The baked world.
    pub world: ServerWorld,
    /// Where player profiles are kept.
    pub profiles: ProfileStore,
    /// Connections to clients.
    pub link: Link,
    /// The game rules (from the config file).
    pub rules: Rules,
    /// The content pack.
    pub content: ContentSetup,
    /// Where new characters start, if not the world's spawn point (for development).
    pub spawn_override: Option<(f32, f32)>,
}

/// The game rules this server plays by.
#[derive(Resource, Clone, Copy)]
pub struct GameRules(pub Rules);

/// The order the server's systems run in, each tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ServerSystems {
    /// Read and validate client messages.
    Requests,
    /// Players joining and leaving.
    Sessions,
    /// In-game requests, simulation and updates to clients.
    Gameplay,
    /// NPCs: matching the content, running their scripts, applying script commands.
    Npcs,
}

impl GameServer {
    /// Adds the game server to a Bevy app. The app can be headless (the
    /// server binary) or the desktop client (`--local` mode).
    pub fn install(self, app: &mut App) {
        let mut scripts = scripting::Scripts::new();
        content::install(app, self.content, &mut scripts);
        app.insert_resource(Time::<Fixed>::from_duration(Duration::from_secs_f32(1.0 / TICK_RATE)))
            .insert_resource(scripts)
            .init_resource::<entities::EntityIds>()
            .insert_resource(self.world)
            .insert_resource(self.profiles)
            .insert_resource(self.link)
            .insert_resource(GameRules(self.rules))
            .insert_resource(sessions::SpawnPoint(self.spawn_override))
            .configure_sets(
                FixedUpdate,
                (ServerSystems::Requests, ServerSystems::Sessions, ServerSystems::Gameplay, ServerSystems::Npcs)
                    .chain(),
            )
            .add_plugins((
                requests::RequestsPlugin,
                sessions::SessionsPlugin,
                gameplay::GameplayPlugin,
                npcs::NpcsPlugin,
                content::ContentPlugin,
            ));
    }
}
