//! Other players: shown where the server says they are.
//!
//! Positions arrive in snapshots 20 times a second. To look smooth, other
//! players are shown slightly in the past, interpolating between the two
//! snapshots around that moment (standard entity interpolation).

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{PlayerId, ServerMessage};

use crate::character::{CharacterAssets, Gait, spawn_character};
use crate::chunk_streaming::ChunkStore;
use crate::coords::{from_bevy, to_bevy};
use crate::nameplates::Nameplate;
use crate::network::{ClientState, FromServer};
use crate::player::LocalPlayer;
use crate::terrain_query::ground_height;

/// How far in the past other players are shown, in seconds (two server ticks).
const INTERPOLATION_DELAY: f32 = 0.1;
/// How many snapshots to remember per player.
const MAX_SAMPLES: usize = 16;
/// How quickly other characters turn to face where they're going (per second).
const TURN_RATE: f32 = 12.0;

/// Shows other players.
pub struct RemotePlayersPlugin;

impl Plugin for RemotePlayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemotePlayers>()
            .add_systems(Update, (follow_server_events, interpolate).chain())
            .add_systems(OnEnter(ClientState::Disconnected), remove_everyone);
    }
}

/// The character entity for each other player.
#[derive(Resource, Default)]
struct RemotePlayers(HashMap<PlayerId, Entity>);

/// Another player's character.
#[derive(Component)]
struct RemotePlayer {
    /// Positions from recent snapshots, with the (client) time each arrived, oldest first.
    samples: VecDeque<(f32, Point)>,
}

fn follow_server_events(
    mut commands: Commands,
    time: Res<Time>,
    mut events: MessageReader<FromServer>,
    assets: Res<CharacterAssets>,
    store: Res<ChunkStore>,
    mut remotes: ResMut<RemotePlayers>,
    me: Option<Single<&LocalPlayer>>,
    mut players: Query<&mut RemotePlayer>,
) {
    let now = time.elapsed_secs();
    let my_id = me.map(|me| me.id);
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::PlayerJoined { id, name, position: (x, y) } => {
                if remotes.0.contains_key(id) || Some(*id) == my_id {
                    continue;
                }
                let translation = to_bevy(*x, *y, ground_height(&store, *x, *y).unwrap_or(0.0));
                let entity = spawn_character(&mut commands, &assets, Transform::from_translation(translation), id.0);
                commands.entity(entity).insert((
                    Name::new(format!("Player {name}")),
                    RemotePlayer { samples: VecDeque::from([(now, (*x, *y))]) },
                    Nameplate(name.clone()),
                ));
                remotes.0.insert(*id, entity);
            }
            ServerMessage::PlayerLeft { id } => {
                if let Some(entity) = remotes.0.remove(id) {
                    commands.entity(entity).despawn();
                }
            }
            ServerMessage::Snapshot { players: states, .. } => {
                for state in states {
                    let Some(mut player) = remotes.0.get(&state.id).and_then(|&e| players.get_mut(e).ok()) else {
                        continue;
                    };
                    player.samples.push_back((now, (state.x, state.y)));
                    if player.samples.len() > MAX_SAMPLES {
                        player.samples.pop_front();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Places each other player where they were [`INTERPOLATION_DELAY`] ago.
fn interpolate(
    time: Res<Time>,
    store: Res<ChunkStore>,
    mut players: Query<(&RemotePlayer, &mut Transform, &mut Gait)>,
) {
    let dt = time.delta_secs();
    let shown_time = time.elapsed_secs() - INTERPOLATION_DELAY;
    for (player, mut transform, mut gait) in &mut players {
        let (x, y) = position_at(&player.samples, shown_time);
        let previous = from_bevy(transform.translation);
        let (moved_x, moved_y) = (x - previous.0, y - previous.1);
        let moved = moved_x.hypot(moved_y);

        let height = ground_height(&store, x, y).unwrap_or(transform.translation.y);
        transform.translation = to_bevy(x, y, height);
        if moved > 1e-4 {
            let facing = Quat::from_rotation_y(moved_x.atan2(moved_y));
            transform.rotation = transform.rotation.slerp(facing, (TURN_RATE * dt).min(1.0));
        }
        gait.speed = if dt > 0.0 { moved / dt } else { 0.0 };
    }
}

/// The position at `time`, interpolated between the samples around it (or
/// the nearest end if `time` is outside them).
fn position_at(samples: &VecDeque<(f32, Point)>, time: f32) -> Point {
    let Some(&(_, last)) = samples.back() else { return (0.0, 0.0) };
    for pair in samples.iter().collect::<Vec<_>>().windows(2) {
        let (&(t0, p0), &(t1, p1)) = (pair[0], pair[1]);
        if time >= t0 && time <= t1 {
            let f = if t1 > t0 { (time - t0) / (t1 - t0) } else { 1.0 };
            return (p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f);
        }
    }
    if samples.front().is_some_and(|&(t0, _)| time < t0) { samples[0].1 } else { last }
}

fn remove_everyone(mut commands: Commands, mut remotes: ResMut<RemotePlayers>) {
    for (_, entity) in remotes.0.drain() {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_interpolated_between_samples() {
        let samples = VecDeque::from([(0.0, (0.0, 0.0)), (1.0, (10.0, 0.0))]);
        assert_eq!(position_at(&samples, 0.5), (5.0, 0.0));
        assert_eq!(position_at(&samples, -1.0), (0.0, 0.0));
        assert_eq!(position_at(&samples, 2.0), (10.0, 0.0));
    }
}
