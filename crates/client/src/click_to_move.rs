//! Clicking the ground to run there: asks the server to move there, and
//! starts running along a locally planned route at once (prediction), with a
//! move marker at the destination.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::ClientMessage;
use venture_simulation::{Movement, plan_route};

use crate::chunk_streaming::{ChunkStore, ObstacleStore, StreamingProgress};
use crate::coords::{from_bevy, to_bevy};
use crate::dev_settings::DevSettings;
use crate::move_marker::{MarkerAssets, MoveMarker, place_marker};
use crate::network::{ClientState, ToServer};
use crate::player::LocalPlayer;
use crate::terrain_query::{ground_height, is_blocked, raycast_ground};
use crate::text_input::KeyboardFocus;

/// Moves the player where the ground is clicked.
pub struct ClickToMovePlugin;

impl Plugin for ClickToMovePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (move_on_click, move_on_startup_if_requested).run_if(in_state(ClientState::Playing)));
    }
}

fn move_on_click(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    ui: Query<&Interaction>,
    mut focus: ResMut<KeyboardFocus>,
    orders: MoveOrders,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // Clicks on the interface (such as the chat box) aren't clicks on the world.
    if ui.iter().any(|interaction| *interaction != Interaction::None) {
        return;
    }
    // Clicking the world stops typing.
    focus.0 = None;
    let Some(cursor) = window.cursor_position() else { return };
    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else { return };
    let Some(point) = raycast_ground(&orders.store, ray) else { return };
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
    store: Res<'w, ChunkStore>,
    obstacles: Res<'w, ObstacleStore>,
    marker_assets: Res<'w, MarkerAssets>,
    markers: Query<'w, 's, Entity, With<MoveMarker>>,
    to_server: MessageWriter<'w, ToServer>,
    player: Option<Single<'w, 's, (&'static Transform, &'static mut LocalPlayer)>>,
}

impl MoveOrders<'_, '_> {
    /// Asks the server to move to `goal`, and predicts the route locally.
    fn send(mut self, commands: &mut Commands, goal: Point) {
        let Some(player) = self.player else { return };
        let (transform, mut player) = player.into_inner();
        self.to_server.write(ToServer(ClientMessage::MoveTo { x: goal.0, y: goal.1 }));

        let start = from_bevy(transform.translation);
        player.movement = plan_route(start, goal, |x, y| is_blocked(&self.obstacles, x, y));
        if let Movement::Route(route) = &player.movement
            && let Some(&(end_x, end_y)) = route.back()
        {
            let end =
                to_bevy(end_x, end_y, ground_height(&self.store, end_x, end_y).unwrap_or(transform.translation.y));
            place_marker(commands, &self.marker_assets, self.markers, end);
        }
    }
}
