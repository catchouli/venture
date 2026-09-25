//! The player's character: where it is, and how it moves. It either runs
//! along a route (from clicking the ground), or is steered with WASD
//! relative to the camera. Steering cancels any route.

use std::collections::VecDeque;

use bevy::prelude::*;
use venture_navigation::Point;

use crate::camera::{CameraHeading, CameraMode, wasd_direction};
use crate::character::{CharacterAssets, Gait, RUN_SPEED, spawn_character};
use crate::chunk_streaming::{ChunkStore, ObstacleStore};
use crate::coords::{from_bevy, to_bevy};
use crate::dev_settings::DevSettings;
use crate::terrain_query::{ground_height, is_blocked};
use crate::world::GameWorld;

/// How quickly the character turns to face where it's going (per second).
const TURN_RATE: f32 = 12.0;

/// Spawns the player's character and moves it along its route.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player).add_systems(Update, (steer_with_keyboard, run_along_route).chain());
    }
}

/// The player's character.
#[derive(Component, Default)]
pub struct Player {
    /// Waypoints still to run to, in order, in metres.
    pub route: VecDeque<Point>,
    /// The direction the player is steering with the keyboard, in world
    /// `(x, y)`, or zero when not steering.
    pub steering: Vec2,
}

impl Player {
    /// Whether the character has somewhere to go.
    pub fn is_moving(&self) -> bool {
        !self.route.is_empty() || self.steering != Vec2::ZERO
    }
}

fn spawn_player(mut commands: Commands, world: Res<GameWorld>, assets: Res<CharacterAssets>, dev: Res<DevSettings>) {
    let (x, y) = dev.start_position.unwrap_or_else(|| world.0.spawn_point());
    let position = to_bevy(x, y, world.0.ground_height(x, y));
    let character = spawn_character(&mut commands, &assets, Transform::from_translation(position));
    commands.entity(character).insert((Name::new("Player"), Player::default()));
}

/// Reads WASD into the player's steering direction, relative to the camera.
/// Steering cancels any route from a click. Only while following the
/// character: in free-camera mode WASD moves the camera instead.
fn steer_with_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<CameraMode>,
    heading: Res<CameraHeading>,
    mut player: Single<&mut Player>,
) {
    let input = if *mode == CameraMode::Follow { wasd_direction(&keys) } else { Vec2::ZERO };
    player.steering = (heading.forward * input.y + heading.right * input.x).normalize_or_zero();
    if player.steering != Vec2::ZERO {
        player.route.clear();
    }
}

/// Moves the character (steered, or along its route), following the ground
/// and turning to face the way it's going.
pub fn run_along_route(
    time: Res<Time>,
    store: Res<ChunkStore>,
    obstacles: Res<ObstacleStore>,
    world: Res<GameWorld>,
    player: Single<(&mut Transform, &mut Player, &mut Gait)>,
) {
    let (mut transform, mut player, mut gait) = player.into_inner();
    let dt = time.delta_secs();
    let start = from_bevy(transform.translation);
    let step = RUN_SPEED * dt;

    let (x, y) = if player.steering != Vec2::ZERO {
        steer(start, player.steering * step, |x, y| is_blocked(&obstacles, x, y))
    } else {
        follow_route(start, step, &mut player.route)
    };

    let (moved_x, moved_y) = (x - start.0, y - start.1);
    let moved = moved_x.hypot(moved_y);
    transform.translation = to_bevy(x, y, ground_height(&store, &world.0, x, y));
    if moved > 0.0 {
        // The character faces +Z in its own space, which is world +y.
        let facing = Quat::from_rotation_y(moved_x.atan2(moved_y));
        transform.rotation = transform.rotation.slerp(facing, (TURN_RATE * dt).min(1.0));
    }
    gait.speed = if dt > 0.0 { moved / dt } else { 0.0 };
}

/// Moves up to `distance` metres along `route` from `(x, y)`, dropping
/// waypoints as they are reached.
fn follow_route((mut x, mut y): Point, distance: f32, route: &mut VecDeque<Point>) -> Point {
    let mut remaining = distance;
    while remaining > 0.0 {
        let Some(&(target_x, target_y)) = route.front() else { break };
        let to_target = (target_x - x).hypot(target_y - y);
        if to_target <= remaining {
            (x, y) = (target_x, target_y);
            remaining -= to_target;
            route.pop_front();
        } else {
            x += (target_x - x) / to_target * remaining;
            y += (target_y - y) / to_target * remaining;
            remaining = 0.0;
        }
    }
    (x, y)
}

/// Moves from `(x, y)` by `offset`, without entering blocked cells. If the
/// full move is blocked, slides along whichever axis is free, so the
/// character glides along walls and shores instead of sticking to them.
fn steer((x, y): Point, offset: Vec2, is_blocked: impl Fn(i32, i32) -> bool) -> Point {
    let open = |(px, py): Point| !is_blocked(px.floor() as i32, py.floor() as i32);
    [(x + offset.x, y + offset.y), (x + offset.x, y), (x, y + offset.y)]
        .into_iter()
        .find(|&candidate| open(candidate))
        .unwrap_or((x, y))
}
