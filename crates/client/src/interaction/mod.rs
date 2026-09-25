//! Interacting with the world with the mouse: selecting characters,
//! following them, clicking the ground to run there, and the marker left
//! where you clicked.

pub mod clicks;
pub mod move_marker;
pub mod targeting;

use bevy::prelude::*;

/// Handles clicks on the world.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((targeting::TargetingPlugin, clicks::ClickPlugin, move_marker::MoveMarkerPlugin));
    }
}
