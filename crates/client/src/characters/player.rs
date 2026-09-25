//! The player's own character: shown where the server says it is, and moved
//! instantly by the player's input using the shared movement rules
//! (prediction), then quietly corrected towards the server's position.
//!
//! Input here only *asks* the server to move (with [`ToServer`] commands).
//! See "Movement" in `docs/client-server.md`.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{ClientMessage, PlayerId, ServerMessage};
use venture_simulation::{Follow, FollowOutcome, Movement, Rules, step};

use crate::camera::{CameraHeading, CameraMode, wasd_direction};
use crate::characters::model::{CharacterAssets, Gait, spawn_character};
use crate::characters::nameplates::Nameplate;
use crate::common::coords::{from_bevy, to_bevy};
use crate::interaction::targeting::Target;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::text_input::KeyboardFocus;
use crate::world::query::{ground_height, is_blocked};
use crate::world::streaming::{ChunkStore, ObstacleStore};

/// How quickly the character turns to face where it's going (per second).
const TURN_RATE: f32 = 12.0;
/// While moving, the prediction may run this far ahead of the server (in
/// metres) before it's snapped back. Running ahead is normal: the server
/// hears about each move a little later.
const MAX_PREDICTION_ERROR: f32 = 3.0;
/// When standing still, the character glides to the server's position at this rate (per second).
const SETTLE_RATE: f32 = 8.0;
/// Differences smaller than this (in metres) are ignored.
const SETTLE_TOLERANCE: f32 = 0.02;
/// The fewest seconds between steering updates sent to the server.
const STEERING_SEND_INTERVAL: f32 = 0.1;

/// Shows and moves the player's character.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SteeringSent>().init_resource::<GameRules>().add_systems(
            Update,
            (
                place_on_welcome,
                steer_with_keyboard.run_if(in_state(ClientState::Playing)),
                stop_following_when_deselected,
                predict_following,
                predict_movement,
                correct_towards_server,
            )
                .chain()
                .in_set(PlayerSystems),
        );
    }
}

/// The systems that move the player's character. The camera follows after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSystems;

/// The player's own character.
#[derive(Component)]
pub struct LocalPlayer {
    /// The player's id this session.
    pub id: PlayerId,
    /// How the character is moving, as predicted locally.
    pub movement: Movement,
    /// Who the character is following, if anyone.
    pub following: Option<Following>,
}

/// Following another character.
pub struct Following {
    /// The character entity being followed.
    pub target: Entity,
    /// The state of the chase.
    pub follow: Follow,
}

/// The server's game rules, received when joining.
#[derive(Resource, Default)]
pub struct GameRules(pub Rules);

/// The steering direction last sent to the server, and how long ago.
#[derive(Resource, Default)]
struct SteeringSent {
    direction: Option<Point>,
    seconds_since: f32,
}

/// Creates the character on the first welcome, and puts it where the server
/// says on every welcome (such as after reconnecting).
fn place_on_welcome(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    assets: Res<CharacterAssets>,
    store: Res<ChunkStore>,
    mut rules: ResMut<GameRules>,
    mut players: Query<(&mut LocalPlayer, &mut Transform, &mut Nameplate)>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Welcome { you, name, position: (x, y), rules: server_rules } = message else { continue };
        rules.0 = *server_rules;
        let translation = to_bevy(*x, *y, ground_height(&store, *x, *y).unwrap_or(0.0));
        if let Ok((mut player, mut transform, mut nameplate)) = players.single_mut() {
            player.id = *you;
            player.movement = Movement::Idle;
            player.following = None;
            transform.translation = translation;
            nameplate.0.clone_from(name);
            continue;
        }
        let character = spawn_character(&mut commands, &assets, Transform::from_translation(translation), you.0);
        commands.entity(character).insert((
            Name::new("Player"),
            LocalPlayer { id: *you, movement: Movement::Idle, following: None },
            Nameplate(name.clone()),
        ));
    }
}

/// Turns WASD (relative to the camera) into steering, and tells the server
/// when it changes. Paused while typing and in free-camera mode.
fn steer_with_keyboard(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<CameraMode>,
    heading: Res<CameraHeading>,
    focus: Res<KeyboardFocus>,
    mut sent: ResMut<SteeringSent>,
    mut to_server: MessageWriter<ToServer>,
    mut player: Single<&mut LocalPlayer>,
) {
    sent.seconds_since += time.delta_secs();
    let input = if *mode == CameraMode::Follow && !focus.is_typing() { wasd_direction(&keys) } else { Vec2::ZERO };
    let steering = (heading.forward * input.y + heading.right * input.x).try_normalize().map(|d| (d.x, d.y));

    match steering {
        Some(direction) => {
            // Steering takes over from following (the server does the same).
            player.following = None;
            player.movement = Movement::Steer(direction);
        }
        // Releasing the keys stops steering, but not a route from a click.
        None if matches!(player.movement, Movement::Steer(_)) => player.movement = Movement::Idle,
        None => {}
    }

    let changed = match (steering, sent.direction) {
        (Some(new), Some(old)) => (new.0 - old.0).abs() + (new.1 - old.1).abs() > 0.05,
        (a, b) => a.is_some() != b.is_some(),
    };
    // Stopping is always sent at once; direction changes at most every interval.
    let due = steering.is_none() || sent.seconds_since >= STEERING_SEND_INTERVAL;
    if changed && due {
        let command = match steering {
            Some((x, y)) => ClientMessage::Steer { x, y },
            None => ClientMessage::Stop,
        };
        to_server.write(ToServer(command));
        sent.direction = steering;
        sent.seconds_since = 0.0;
    }
}

/// Stops following when the followed character is no longer the target
/// (deselected with ✕ or Escape, another target chosen, or they left), and
/// tells the server.
fn stop_following_when_deselected(
    target: Res<Target>,
    mut to_server: MessageWriter<ToServer>,
    player: Option<Single<&mut LocalPlayer>>,
) {
    let Some(mut player) = player else { return };
    let Some(following) = &player.following else { return };
    if target.0 != Some(following.target) {
        player.following = None;
        player.movement = Movement::Idle;
        to_server.write(ToServer(ClientMessage::Stop));
    }
}

/// Predicts following: runs towards the followed character with the same
/// shared rules the server uses.
fn predict_following(
    rules: Res<GameRules>,
    obstacles: Res<ObstacleStore>,
    targets: Query<&Transform, Without<LocalPlayer>>,
    player: Option<Single<(&Transform, &mut LocalPlayer)>>,
) {
    let Some(player) = player else { return };
    let (transform, mut player) = player.into_inner();
    let LocalPlayer { following, movement, .. } = &mut *player;
    let Some(chase) = following else { return };
    let Ok(target) = targets.get(chase.target) else {
        *following = None;
        return;
    };
    let (position, target) = (from_bevy(transform.translation), from_bevy(target.translation));
    let outcome =
        chase.follow.update(position, target, movement, rules.0.max_move_distance, |x, y| is_blocked(&obstacles, x, y));
    if outcome == FollowOutcome::Stop {
        *following = None;
    }
}

/// Moves the character locally with the shared movement rules, following
/// the ground and turning to face the way it's going.
fn predict_movement(
    time: Res<Time>,
    store: Res<ChunkStore>,
    obstacles: Res<ObstacleStore>,
    player: Option<Single<(&mut Transform, &mut LocalPlayer, &mut Gait)>>,
) {
    let Some(player) = player else { return };
    let (mut transform, mut player, mut gait) = player.into_inner();
    let dt = time.delta_secs();
    let start = from_bevy(transform.translation);
    let (x, y) = step(start, &mut player.movement, dt, |x, y| is_blocked(&obstacles, x, y));

    let (moved_x, moved_y) = (x - start.0, y - start.1);
    let moved = moved_x.hypot(moved_y);
    let height = ground_height(&store, x, y).unwrap_or(transform.translation.y);
    transform.translation = to_bevy(x, y, height);
    if moved > 0.0 {
        // The character faces +Z in its own space, which is world +y.
        let facing = Quat::from_rotation_y(moved_x.atan2(moved_y));
        transform.rotation = transform.rotation.slerp(facing, (TURN_RATE * dt).min(1.0));
    }
    gait.speed = if dt > 0.0 { moved / dt } else { 0.0 };
}

/// Compares the prediction with the server's position in each snapshot. A
/// large difference snaps the character to the server; when both have
/// stopped, it glides the last bit.
fn correct_towards_server(
    time: Res<Time>,
    mut events: MessageReader<FromServer>,
    mut server_position: Local<Option<(Point, bool)>>,
    player: Option<Single<(&mut Transform, &LocalPlayer)>>,
) {
    let Some(player) = player else { return };
    let (mut transform, player) = player.into_inner();
    for FromServer(message) in events.read() {
        if let ServerMessage::Snapshot { players, .. } = message
            && let Some(me) = players.iter().find(|state| state.id == player.id)
        {
            *server_position = Some(((me.x, me.y), me.moving));
        }
    }
    let Some(((server_x, server_y), server_moving)) = *server_position else { return };

    let (x, y) = from_bevy(transform.translation);
    let error = (server_x - x).hypot(server_y - y);
    if error > MAX_PREDICTION_ERROR {
        transform.translation.x = server_x;
        transform.translation.z = server_y;
    } else if !server_moving && !player.movement.is_moving() && error > SETTLE_TOLERANCE {
        let blend = (SETTLE_RATE * time.delta_secs()).min(1.0);
        transform.translation.x += (server_x - x) * blend;
        transform.translation.z += (server_y - y) * blend;
    }
}
