//! Development options read from environment variables, for checking the game
//! without a person at the keyboard (see `docs/building-and-running.md`).
//!
//! - `VENTURE_CAMERA=x,y,yaw,pitch,distance`: start the player at `(x, y)`
//!   metres, with the camera at that rotation, tilt (radians) and distance (metres).
//! - `VENTURE_MOVE_TO=x,y`: once loaded, run to `(x, y)` as if it were clicked.
//! - `VENTURE_SCREENSHOT=path`: see [`crate::screenshot`].
//!
//! Environment variables don't exist on the web, so there these are all unset.

use bevy::prelude::*;
use venture_navigation::Point;

/// Development options.
#[derive(Resource, Default, Debug)]
pub struct DevSettings {
    /// Where the player starts, instead of the world's spawn point.
    pub start_position: Option<Point>,
    /// The camera's starting `(yaw, pitch, distance)`.
    pub camera_view: Option<(f32, f32, f32)>,
    /// Where the player runs once the world has loaded.
    pub move_to: Option<Point>,
}

impl DevSettings {
    /// Reads the settings from the environment.
    pub fn from_environment() -> Self {
        let camera = numbers("VENTURE_CAMERA").filter(|n| n.len() == 5);
        let move_to = numbers("VENTURE_MOVE_TO").filter(|n| n.len() == 2);
        Self {
            start_position: camera.as_ref().map(|n| (n[0], n[1])),
            camera_view: camera.as_ref().map(|n| (n[2], n[3], n[4])),
            move_to: move_to.map(|n| (n[0], n[1])),
        }
    }
}

/// A comma-separated list of numbers from an environment variable.
fn numbers(variable: &str) -> Option<Vec<f32>> {
    let value = std::env::var(variable).ok()?;
    value.split(',').map(|part| part.trim().parse().ok()).collect()
}
