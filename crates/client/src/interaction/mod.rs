//! Interacting with the world: selecting characters, clicking the ground to
//! run there (and the marker left where you clicked), giving the character
//! orders, and the actions a player can take.

pub mod actions;
pub mod admin_commands;
pub mod clicks;
pub mod move_marker;
pub mod orders;
pub mod targeting;

use bevy::prelude::*;

/// Handles clicks on the world.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            actions::ActionsPlugin,
            targeting::TargetingPlugin,
            clicks::ClickPlugin,
            move_marker::MoveMarkerPlugin,
        ));
    }
}
