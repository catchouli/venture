//! Clicking the ground to run there: finds the clicked point, plans a route
//! around obstacles, and drops a move marker at the destination.

use bevy::prelude::*;
use venture_navigation::{Point, find_path};

use crate::chunk_streaming::{ChunkStore, ObstacleStore, StreamingProgress};
use crate::coords::{from_bevy, to_bevy};
use crate::dev_settings::DevSettings;
use crate::move_marker::{MarkerAssets, MoveMarker, place_marker};
use crate::player::Player;
use crate::terrain_query::{ground_height, is_blocked, raycast_ground};
use crate::world::GameWorld;

/// The most terrain cells the pathfinder may explore for one click.
const MAX_SEARCH_CELLS: usize = 40_000;

/// Moves the player where the ground is clicked.
pub struct ClickToMovePlugin;

impl Plugin for ClickToMovePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (move_on_click, move_on_startup_if_requested));
    }
}

fn move_on_click(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    store: Res<ChunkStore>,
    orders: MoveOrders,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = window.cursor_position() else { return };
    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else { return };
    let Some(point) = raycast_ground(&store, ray) else { return };
    orders.send(&mut commands, from_bevy(point));
}

/// For development: runs to `VENTURE_MOVE_TO` once the world around the player has loaded.
fn move_on_startup_if_requested(
    mut commands: Commands,
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    orders: MoveOrders,
    mut done: Local<bool>,
) {
    let Some(goal) = dev.move_to else { return };
    if *done || !progress.measured || progress.waiting > 0 {
        return;
    }
    orders.send(&mut commands, goal);
    *done = true;
}

/// Everything needed to send the player somewhere.
#[derive(bevy::ecs::system::SystemParam)]
struct MoveOrders<'w, 's> {
    world: Res<'w, GameWorld>,
    store: Res<'w, ChunkStore>,
    obstacles: Res<'w, ObstacleStore>,
    marker_assets: Res<'w, MarkerAssets>,
    markers: Query<'w, 's, Entity, With<MoveMarker>>,
    player: Single<'w, 's, (&'static Transform, &'static mut Player)>,
}

impl MoveOrders<'_, '_> {
    /// Plans a route to `goal` and marks where the player will stop.
    fn send(self, commands: &mut Commands, goal: Point) {
        let (transform, mut player) = self.player.into_inner();
        let start = from_bevy(transform.translation);
        let route = find_path(start, goal, |x, y| is_blocked(&self.obstacles, x, y), MAX_SEARCH_CELLS);
        let Some(&(end_x, end_y)) = route.last() else { return };
        let end = to_bevy(end_x, end_y, ground_height(&self.store, &self.world.0, end_x, end_y));
        place_marker(commands, &self.marker_assets, self.markers, end);
        player.route = route.into();
    }
}
