//! Game rules shared by the server and the client.
//!
//! The server runs these rules to decide what happens. The client runs the
//! same rules to *predict* the player's own movement, so it feels instant
//! (see `docs/client-server.md`). Keeping one copy of the rules means the
//! prediction and the server agree.

mod day;
pub mod fishing;
mod following;
mod movement;
mod rules;

pub use following::{FOLLOW_DISTANCE, Follow, FollowOutcome, INTERACT_DISTANCE, MELEE_RANGE, TALK_RANGE};

pub use day::{DayCycle, SkyPosition};
pub use movement::{
    BadMove, Body, MoveBudget, Movement, RUN_SPEED, TICK_RATE, check_move, clamp_move_goal, plan_route,
    steering_direction, step,
};
pub use rules::{MovementRules, Rules};
