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
    /// How characters move: gravity, jumps and slopes.
    #[serde(default)]
    pub movement: MovementRules,
}

impl Default for Rules {
    fn default() -> Self {
        Self { max_move_distance: 150.0, day: DayCycle::default(), movement: MovementRules::default() }
    }
}

/// How characters move through the world (see `docs/plan-collision.md`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementRules {
    /// How fast characters fall faster, in metres per second per second.
    pub gravity: f32,
    /// How high a jump lifts a character's feet, in metres.
    pub jump_height: f32,
    /// The steepest slope a character can walk up, in degrees. Steeper ones
    /// slide them down.
    pub max_climb_degrees: f32,
    /// The highest ledge a character steps up without jumping, in metres.
    pub step_height: f32,
}

impl Default for MovementRules {
    fn default() -> Self {
        Self { gravity: 20.0, jump_height: 1.4, max_climb_degrees: 50.0, step_height: 0.4 }
    }
}

impl MovementRules {
    /// The speed a jump leaves the ground at, in metres per second.
    pub fn jump_speed(&self) -> f32 {
        (2.0 * self.gravity.max(0.0) * self.jump_height.max(0.0)).sqrt()
    }

    /// How the character controller moves characters by these rules.
    pub fn character(&self) -> venture_collision::CharacterSettings {
        venture_collision::CharacterSettings {
            max_climb_degrees: self.max_climb_degrees,
            min_slide_degrees: self.max_climb_degrees,
            step_height: self.step_height,
            ..Default::default()
        }
    }
}
