//! What a left click on the world does:
//!
//! - on a character: select it; or, if it's selected already, follow them
//!   (a player) or walk over and talk to them (an NPC);
//! - on a trap: select it; or, if it's selected already and yours, collect it;
//! - on water, with a fishing rod in hand: cast there (see [`crate::fishing`]);
//! - on the ground: run there (at most the server's move distance away),
//!   with a move marker at the destination.
//!
//! While a fish bites or is hooked, clicks strike and reel instead.
//!
//! Following and talking are [actions](crate::interaction::actions), the same
//! as typing `/follow` or `/talk`.

use bevy::prelude::*;

use crate::common::coords::from_bevy;
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::fishing::traps::CollectTrap;
use crate::fishing::{Angling, CastAt, rod_in_hand};
use crate::interaction::actions::{Action, PerformAction};
use crate::interaction::orders::Orders;
use crate::interaction::targeting::{Hovered, Target, TargetKind, Targetable};
use crate::network::ClientState;
use crate::ui::character_window::Sheet;
use crate::ui::text_input::KeyboardFocus;
use crate::world::query::{raycast_ground, raycast_surface, water_level};
use crate::world::streaming::StreamingProgress;

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
    mut actions: MessageWriter<PerformAction>,
    mut orders: Orders,
    angling: Res<Angling>,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    mut casts: MessageWriter<CastAt>,
    mut traps: MessageWriter<CollectTrap>,
) {
    if !buttons.just_pressed(MouseButton::Left) || angling.busy() {
        return;
    }
    // Clicks on the interface (such as the chat box) aren't clicks on the world.
    if ui.iter().any(|interaction| *interaction != Interaction::None) {
        return;
    }
    // Clicking the world stops typing.
    focus.0 = None;

    if let Some(clicked) = hovered.0 {
        let already_selected = target.0 == Some(clicked);
        match targetables.get(clicked).map(|targetable| targetable.kind) {
            Ok(TargetKind::Player) if already_selected => {
                actions.write(PerformAction::new(Action::Follow));
            }
            Ok(TargetKind::Npc) if already_selected => {
                actions.write(PerformAction::new(Action::Talk));
            }
            Ok(TargetKind::Enemy) if already_selected => {
                actions.write(PerformAction::new(Action::Attack));
            }
            Ok(TargetKind::Trap) if already_selected => {
                traps.write(CollectTrap(clicked));
            }
            _ => target.0 = Some(clicked),
        }
        return;
    }

    let Some(cursor) = window.cursor_position() else { return };
    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else { return };
    if rod_in_hand(&sheet, &content).is_some()
        && let Some(point) = raycast_surface(&orders.store, ray)
    {
        let (x, y) = from_bevy(point);
        if water_level(&orders.store, x, y).is_some() {
            casts.write(CastAt((x, y)));
            return;
        }
    }
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
