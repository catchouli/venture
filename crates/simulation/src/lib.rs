//! Game rules shared by the server and the client.
//!
//! The server runs these rules to decide what happens. The client runs the
//! same rules to *predict* the player's own movement, so it feels instant
//! (see `docs/client-server.md`). Keeping one copy of the rules means the
//! prediction and the server agree.

mod following;
mod movement;
mod rules;

pub use following::{FOLLOW_DISTANCE, Follow, FollowOutcome, INTERACT_DISTANCE, MELEE_RANGE, TALK_RANGE};

/// How long a jump lasts, in seconds (from leaving the ground to landing).
/// For now jumps are for show: movement stays on the ground, so a character
/// can't jump over anything (jumping onto things is planned, but the game
/// will never require it). A new jump can't start until the last has landed.
pub const JUMP_SECONDS: f32 = 0.6;
pub use movement::{Movement, RUN_SPEED, TICK_RATE, clamp_move_goal, plan_route, steering_direction, step};
pub use rules::Rules;
