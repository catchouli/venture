//! Tunable game rules. The server decides them (see its command-line options)
//! and tells each client when it joins, so both sides play by the same rules.

use serde::{Deserialize, Serialize};

use crate::DayCycle;

/// Game rules that can be tuned per server.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    /// The furthest (in a straight line, in metres) a single click can send a
    /// character, and the furthest a character keeps following someone.
    /// Further clicks go this far in that direction, so crossing the world
    /// takes active play rather than one click.
    pub max_move_distance: f32,
    /// How the days go (the clock, and the sun's path).
    pub day: DayCycle,
}

impl Default for Rules {
    fn default() -> Self {
        Self { max_move_distance: 150.0, day: DayCycle::default() }
    }
}
