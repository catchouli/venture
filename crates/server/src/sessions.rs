//! Players joining and leaving. The only code that creates and removes
//! players, and that loads and saves their profiles.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{EntityKind, ServerMessage};

use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::link::{ConnectionId, Link};
use crate::names::random_name;
use crate::npcs::Npc;
use crate::profiles::{Profile, ProfileKey, ProfileStore};
use crate::requests::{Connections, SessionRequest, Stage};
use crate::world::ServerWorld;
use crate::{GameRules, ServerSystems};

/// How often everyone's profile is saved while they play, in seconds.
const AUTOSAVE_INTERVAL: f32 = 60.0;

/// Handles joining, leaving and saving.
pub struct SessionsPlugin;

impl Plugin for SessionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnlineProfiles>()
            .init_resource::<AutosaveTimer>()
            .add_systems(FixedUpdate, (handle_session_requests, autosave).chain().in_set(ServerSystems::Sessions))
            .add_systems(Last, save_everyone_on_exit);
    }
}

/// Where new characters start, if not the world's spawn point.
#[derive(Resource)]
pub struct SpawnPoint(pub Option<Point>);

/// The profiles currently in the game, so a character can't be logged in twice.
#[derive(Resource, Default)]
struct OnlineProfiles(std::collections::HashSet<ProfileKey>);

#[derive(Resource)]
struct AutosaveTimer(Timer);

impl Default for AutosaveTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(AUTOSAVE_INTERVAL, TimerMode::Repeating))
    }
}

/// The resources joining and leaving need.
#[derive(bevy::ecs::system::SystemParam)]
struct Sessions<'w, 's> {
    commands: Commands<'w, 's>,
    link: ResMut<'w, Link>,
    connections: ResMut<'w, Connections>,
    store: Res<'w, ProfileStore>,
    world: Res<'w, ServerWorld>,
    spawn_point: Res<'w, SpawnPoint>,
    ids: ResMut<'w, EntityIds>,
    online: ResMut<'w, OnlineProfiles>,
    rules: Res<'w, GameRules>,
    players: Query<'w, 's, (Entity, &'static Player, &'static NetworkId, &'static DisplayName, &'static Position)>,
    npcs: Query<'w, 's, (&'static Npc, &'static NetworkId, &'static DisplayName, &'static Position)>,
}

fn handle_session_requests(mut requests: MessageReader<SessionRequest>, mut sessions: Sessions) {
    for request in requests.read() {
        match request {
            SessionRequest::Hello { connection, profile } => sessions.log_in(*connection, profile),
            SessionRequest::ChooseName { connection, profile, name } => {
                let position = sessions.spawn_point.0.unwrap_or(sessions.world.manifest.spawn_point);
                let new_profile = Profile { name: name.clone(), position };
                sessions.store.save(profile, &new_profile);
                sessions.enter_game(*connection, profile.clone(), new_profile);
            }
            SessionRequest::Leave { connection } => sessions.leave(*connection),
        }
    }
}

impl Sessions<'_, '_> {
    /// Loads the player's profile and enters the game, or asks for a name if
    /// they're new.
    fn log_in(&mut self, connection: ConnectionId, profile: &ProfileKey) {
        if self.online.0.contains(profile) {
            let rejected = ServerMessage::Rejected { reason: "This character is already in the game.".to_string() };
            self.link.send(connection, &rejected);
            self.link.close(connection);
            return;
        }
        match self.store.load(profile) {
            Some(saved) => self.enter_game(connection, profile.clone(), saved),
            None => {
                self.set_stage(connection, Stage::AwaitingName { profile: profile.clone() });
                self.link.send(connection, &ServerMessage::NameNeeded { suggestion: random_name() });
            }
        }
    }

    /// Puts a character into the game and tells everyone.
    fn enter_game(&mut self, connection: ConnectionId, profile: ProfileKey, saved: Profile) {
        let id = self.ids.allocate();
        let position = Position(if self.world.contains(saved.position) {
            saved.position
        } else {
            self.world.manifest.spawn_point
        });
        let name = DisplayName(saved.name);

        // Tell the newcomer about themselves, and everyone and everything already here.
        let welcome =
            ServerMessage::Welcome { you: id, name: name.0.clone(), position: position.0, rules: self.rules.0 };
        self.link.send(connection, &welcome);
        for (_, _, &NetworkId(other), other_name, &other_position) in &self.players {
            self.link.send(connection, &appeared(other, other_name, EntityKind::Player, other_position));
        }
        for (npc, &NetworkId(npc_id), npc_name, &npc_position) in &self.npcs {
            self.link.send(connection, &appeared(npc_id, npc_name, npc.kind(), npc_position));
        }
        // Tell everyone else about the newcomer.
        let others: Vec<ConnectionId> = self.players.iter().map(|(_, player, ..)| player.connection).collect();
        self.link.broadcast(others.iter().copied(), &appeared(id, &name, EntityKind::Player, position));
        let notice = ServerMessage::Notice { text: format!("{} has joined.", name.0) };
        self.link.broadcast(others.into_iter().chain([connection]), &notice);

        info!("{} joined as {id:?}.", name.0);
        self.online.0.insert(profile.clone());
        let player =
            self.commands.spawn((Player { connection, profile }, NetworkId(id), name, position, Mover::default())).id();
        self.set_stage(connection, Stage::Playing { player });
    }

    /// Saves and removes a player whose connection has closed, and tells everyone.
    fn leave(&mut self, connection: ConnectionId) {
        let Some(state) = self.connections.0.remove(&connection) else { return };
        let Stage::Playing { player } = state.stage else { return };
        let Ok((entity, leaver, &NetworkId(id), name, &Position(position))) = self.players.get(player) else { return };

        self.store.save(&leaver.profile, &Profile { name: name.0.clone(), position });
        self.online.0.remove(&leaver.profile);
        let others: Vec<ConnectionId> =
            self.players.iter().filter(|(e, ..)| *e != entity).map(|(_, player, ..)| player.connection).collect();
        self.link.broadcast(others.iter().copied(), &ServerMessage::EntityLeft { id });
        self.link.broadcast(others, &ServerMessage::Notice { text: format!("{} has left.", name.0) });
        info!("{} left.", name.0);
        self.commands.entity(entity).despawn();
    }

    fn set_stage(&mut self, connection: ConnectionId, stage: Stage) {
        if let Some(state) = self.connections.0.get_mut(&connection) {
            state.stage = stage;
        }
    }
}

fn autosave(
    time: Res<Time>,
    mut timer: ResMut<AutosaveTimer>,
    store: Res<ProfileStore>,
    players: Query<(&Player, &DisplayName, &Position)>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        save_all(&store, &players);
    }
}

/// Saves everyone when the server shuts down (such as on Ctrl+C).
fn save_everyone_on_exit(
    mut exits: MessageReader<AppExit>,
    store: Option<Res<ProfileStore>>,
    players: Query<(&Player, &DisplayName, &Position)>,
) {
    if exits.read().next().is_some()
        && let Some(store) = store
    {
        save_all(&store, &players);
        info!("Saved everyone's profiles.");
    }
}

fn save_all(store: &ProfileStore, players: &Query<(&Player, &DisplayName, &Position)>) {
    for (player, name, &Position(position)) in players {
        store.save(&player.profile, &Profile { name: name.0.clone(), position });
    }
}
