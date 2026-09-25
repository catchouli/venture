//! What a left click on the world does:
//!
//! - on a character: select it, or, if it's the selected player already,
//!   follow them;
//! - on the ground: run there (at most the server's move distance away),
//!   with a move marker at the destination.
//!
//! Moving and following are asked of the server ([`ToServer`]) and predicted
//! locally at once, so they feel instant.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::ClientMessage;
use venture_simulation::{Follow, Movement, clamp_move_goal, plan_route};

use crate::characters::player::{Following, GameRules, LocalPlayer};
use crate::common::coords::{from_bevy, to_bevy};
use crate::dev::settings::DevSettings;
use crate::interaction::move_marker::{MarkerAssets, MoveMarker, place_marker};
use crate::interaction::targeting::{Hovered, Target, TargetKind, Targetable};
use crate::network::{ClientState, ToServer};
use crate::ui::text_input::KeyboardFocus;
use crate::world::query::{ground_height, is_blocked, raycast_ground};
use crate::world::streaming::{ChunkStore, ObstacleStore, StreamingProgress};

/// Handles left clicks on the world.
pub struct ClickPlugin;

impl Plugin for ClickPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (handle_click, move_on_startup_if_requested).run_if(in_state(ClientState::Playing)));
    }
}

fn handle_click(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    ui: Query<&Interaction>,
    hovered: Res<Hovered>,
    targetables: Query<&Targetable>,
    mut target: ResMut<Target>,
    mut focus: ResMut<KeyboardFocus>,
    mut orders: Orders,
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

    if let Some(clicked) = hovered.0 {
        match targetables.get(clicked) {
            Ok(Targetable(TargetKind::Player(id))) if target.0 == Some(clicked) => orders.follow(clicked, *id),
            _ => target.0 = Some(clicked),
        }
        return;
    }

    let Some(cursor) = window.cursor_position() else { return };
    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else { return };
    let Some(point) = raycast_ground(&orders.store, ray) else { return };
    orders.move_to(&mut commands, from_bevy(point));
}

/// For development: runs to `VENTURE_MOVE_TO` once the world around the player has loaded.
fn move_on_startup_if_requested(
    mut commands: Commands,
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    mut orders: Orders,
    mut done: Local<bool>,
) {
    let Some(goal) = dev.move_to else { return };
    if *done || !progress.measured || progress.waiting > 0 {
        return;
    }
    orders.move_to(&mut commands, goal);
    *done = true;
}

/// Everything needed to give the player's character orders.
#[derive(bevy::ecs::system::SystemParam)]
struct Orders<'w, 's> {
    store: Res<'w, ChunkStore>,
    obstacles: Res<'w, ObstacleStore>,
    rules: Res<'w, GameRules>,
    marker_assets: Res<'w, MarkerAssets>,
    markers: Query<'w, 's, Entity, With<MoveMarker>>,
    to_server: MessageWriter<'w, ToServer>,
    player: Option<Single<'w, 's, (&'static Transform, &'static mut LocalPlayer)>>,
}

impl Orders<'_, '_> {
    /// Asks the server to move towards `goal` (at most the allowed distance
    /// away), and predicts the route locally.
    fn move_to(&mut self, commands: &mut Commands, goal: Point) {
        let Some(player) = self.player.as_mut() else { return };
        let (transform, player) = &mut **player;
        let start = from_bevy(transform.translation);
        let goal = clamp_move_goal(start, goal, self.rules.0.max_move_distance);
        self.to_server.write(ToServer(ClientMessage::MoveTo { x: goal.0, y: goal.1 }));

        player.following = None;
        player.movement = plan_route(start, goal, |x, y| is_blocked(&self.obstacles, x, y));
        if let Movement::Route(route) = &player.movement
            && let Some(&(end_x, end_y)) = route.back()
        {
            let end =
                to_bevy(end_x, end_y, ground_height(&self.store, end_x, end_y).unwrap_or(transform.translation.y));
            place_marker(commands, &self.marker_assets, self.markers.iter(), end);
        }
    }

    /// Asks the server to follow another player, and starts predicting it.
    fn follow(&mut self, target: Entity, id: venture_protocol::PlayerId) {
        let Some(player) = self.player.as_mut() else { return };
        self.to_server.write(ToServer(ClientMessage::Follow { target: id }));
        player.1.following = Some(Following { target, follow: Follow::default() });
    }
}
