//! Players joining and leaving. The only code that creates and removes
//! players, and that loads and saves their profiles.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{PlayerId, ServerMessage};

use crate::ServerSystems;
use crate::link::{ConnectionId, Link};
use crate::names::random_name;
use crate::players::{Mover, Player, Position};
use crate::profiles::{Profile, ProfileKey, ProfileStore};
use crate::requests::{Connections, SessionRequest, Stage};
use crate::world::ServerWorld;

/// How often everyone's profile is saved while they play, in seconds.
const AUTOSAVE_INTERVAL: f32 = 60.0;

/// Handles joining, leaving and saving.
pub struct SessionsPlugin;

impl Plugin for SessionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NextPlayerId>()
            .init_resource::<OnlineProfiles>()
            .init_resource::<AutosaveTimer>()
            .add_systems(FixedUpdate, (handle_session_requests, autosave).chain().in_set(ServerSystems::Sessions))
            .add_systems(Last, save_everyone_on_exit);
    }
}

/// Where new characters start, if not the world's spawn point.
#[derive(Resource)]
pub struct SpawnPoint(pub Option<Point>);

#[derive(Resource, Default)]
struct NextPlayerId(u32);

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
    next_id: ResMut<'w, NextPlayerId>,
    online: ResMut<'w, OnlineProfiles>,
    players: Query<'w, 's, (Entity, &'static Player, &'static Position)>,
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
            self.link.send(
                connection,
                &ServerMessage::Rejected { reason: "This character is already in the game.".to_string() },
            );
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
        self.next_id.0 += 1;
        let id = PlayerId(self.next_id.0);
        let position =
            if self.world.contains(saved.position) { saved.position } else { self.world.manifest.spawn_point };
        let name = saved.name;

        // Tell the newcomer about themselves and everyone already here.
        self.link.send(connection, &ServerMessage::Welcome { you: id, name: name.clone(), position });
        let others: Vec<ConnectionId> = self.players.iter().map(|(_, player, _)| player.connection).collect();
        for (_, player, Position(their_position)) in &self.players {
            let joined =
                ServerMessage::PlayerJoined { id: player.id, name: player.name.clone(), position: *their_position };
            self.link.send(connection, &joined);
        }
        // Tell everyone else about the newcomer.
        self.link.broadcast(others.iter().copied(), &ServerMessage::PlayerJoined { id, name: name.clone(), position });
        let notice = ServerMessage::Notice { text: format!("{name} has joined.") };
        self.link.broadcast(others.into_iter().chain([connection]), &notice);

        info!("{name} joined as {id:?}.");
        self.online.0.insert(profile.clone());
        let player =
            self.commands.spawn((Player { id, connection, name, profile }, Position(position), Mover::default())).id();
        self.set_stage(connection, Stage::Playing { player });
    }

    /// Saves and removes a player whose connection has closed, and tells everyone.
    fn leave(&mut self, connection: ConnectionId) {
        let Some(state) = self.connections.0.remove(&connection) else { return };
        let Stage::Playing { player } = state.stage else { return };
        let Ok((entity, leaver, &Position(position))) = self.players.get(player) else { return };

        self.store.save(&leaver.profile, &Profile { name: leaver.name.clone(), position });
        self.online.0.remove(&leaver.profile);
        let others: Vec<ConnectionId> =
            self.players.iter().filter(|(e, ..)| *e != entity).map(|(_, player, _)| player.connection).collect();
        self.link.broadcast(others.iter().copied(), &ServerMessage::PlayerLeft { id: leaver.id });
        self.link.broadcast(others, &ServerMessage::Notice { text: format!("{} has left.", leaver.name) });
        info!("{} left.", leaver.name);
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
    players: Query<(&Player, &Position)>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        save_all(&store, &players);
    }
}

/// Saves everyone when the server shuts down (such as on Ctrl+C).
fn save_everyone_on_exit(
    mut exits: MessageReader<AppExit>,
    store: Option<Res<ProfileStore>>,
    players: Query<(&Player, &Position)>,
) {
    if exits.read().next().is_some()
        && let Some(store) = store
    {
        save_all(&store, &players);
        info!("Saved everyone's profiles.");
    }
}

fn save_all(store: &ProfileStore, players: &Query<(&Player, &Position)>) {
    for (player, &Position(position)) in players {
        store.save(&player.profile, &Profile { name: player.name.clone(), position });
    }
}
