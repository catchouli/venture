//! Development tools: the debug overlay, automatic screenshots, and settings
//! read from environment variables.

pub mod debug_hud;
pub mod screenshot;
pub mod settings;

use bevy::prelude::*;

/// Adds the development tools.
pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((debug_hud::DebugHudPlugin, screenshot::ScreenshotPlugin));
    }
}
