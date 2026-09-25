//! Development options read from environment variables, for checking the game
//! without a person at the keyboard (see `docs/building-and-running.md`).
//!
//! - `VENTURE_CAMERA=x,y,yaw,pitch,distance`: start new characters at `(x, y)`
//!   metres (in `--local` mode), with the camera at that rotation, tilt
//!   (radians) and distance (metres).
//! - `VENTURE_MOVE_TO=x,y`: once loaded, run to `(x, y)` as if it were clicked.
//! - `VENTURE_SCREENSHOT=path`: see [`crate::screenshot`].
//! - `VENTURE_PROFILE=name`: use a separate login token (so a separate
//!   character), for running several clients on one computer.
//! - `VENTURE_AUTOMATED=1`: nobody is at the keyboard, so take the suggested
//!   name without asking (implied by `VENTURE_SCREENSHOT`).
//! - `VENTURE_SAY=text`: say this in chat once in the game.
//!
//! Environment variables don't exist on the web, so there these are all unset.

use bevy::prelude::*;
use venture_navigation::Point;

/// Development options.
#[derive(Resource, Default, Debug)]
pub struct DevSettings {
    /// Where new characters start, instead of the world's spawn point (`--local` mode only).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub start_position: Option<Point>,
    /// The camera's starting `(yaw, pitch, distance)`.
    pub camera_view: Option<(f32, f32, f32)>,
    /// Where the player runs once the world has loaded.
    pub move_to: Option<Point>,
    /// Which login token to use, if not the usual one.
    pub profile: Option<String>,
    /// Whether nobody is at the keyboard (a screenshot run), so prompts should
    /// take their defaults.
    pub automated: bool,
    /// Something to say in chat once in the game.
    pub say: Option<String>,
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
            profile: std::env::var("VENTURE_PROFILE").ok().filter(|name| name.chars().all(char::is_alphanumeric)),
            automated: std::env::var_os("VENTURE_SCREENSHOT").is_some()
                || std::env::var_os("VENTURE_AUTOMATED").is_some(),
            say: std::env::var("VENTURE_SAY").ok(),
        }
    }
}

/// A comma-separated list of numbers from an environment variable.
fn numbers(variable: &str) -> Option<Vec<f32>> {
    let value = std::env::var(variable).ok()?;
    value.split(',').map(|part| part.trim().parse().ok()).collect()
}
