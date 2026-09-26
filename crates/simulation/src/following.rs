//! Following another character: keep running towards them until close, and
//! re-plan the route when they move away.

use venture_navigation::Point;

use crate::movement::{Movement, plan_route};

/// Followers stop this close to whoever they follow, in metres.
pub const FOLLOW_DISTANCE: f32 = 2.0;

/// How close (in metres) a player must be to an NPC to talk to them.
pub const INTERACT_DISTANCE: f32 = 3.0;
/// How close (in metres) a character must be to hit something with a weapon.
/// A little more than [`FOLLOW_DISTANCE`], so chasing something keeps it in reach.
pub const MELEE_RANGE: f32 = 2.6;

/// Once talking or trading with an NPC, the player may walk this far from
/// them (in metres) before the conversation or shop closes.
pub const TALK_RANGE: f32 = 6.0;

/// The route is re-planned once the target has moved this far (in metres)
/// from where the current route was planned to.
const REPLAN_DISTANCE: f32 = 1.0;

/// The state of following someone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Follow {
    /// Where the target was when the current route was planned.
    planned_for: Option<Point>,
}

/// Whether to keep following.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowOutcome {
    /// Keep following.
    Continue,
    /// The target is out of reach; stop following.
    Stop,
}

impl Follow {
    /// Updates `movement` for a follower at `position` whose target is at
    /// `target`: runs towards the target (re-planning when it moves), and
    /// stands still once close. Gives up if the target is further than
    /// `max_distance`.
    pub fn update(
        &mut self,
        position: Point,
        target: Point,
        movement: &mut Movement,
        max_distance: f32,
        is_blocked: impl Fn(i32, i32) -> bool,
    ) -> FollowOutcome {
        let distance = (target.0 - position.0).hypot(target.1 - position.1);
        if distance > max_distance {
            *movement = Movement::Idle;
            return FollowOutcome::Stop;
        }
        if distance <= FOLLOW_DISTANCE {
            *movement = Movement::Idle;
            self.planned_for = None;
            return FollowOutcome::Continue;
        }
        let target_moved =
            self.planned_for.is_none_or(|planned| (target.0 - planned.0).hypot(target.1 - planned.1) > REPLAN_DISTANCE);
        if target_moved || !movement.is_moving() {
            *movement = plan_route(position, target, is_blocked);
            self.planned_for = Some(target);
        }
        FollowOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: fn(i32, i32) -> bool = |_, _| false;

    #[test]
    fn followers_run_towards_their_target_and_stop_close_by() {
        let mut follow = Follow::default();
        let mut movement = Movement::Idle;
        assert_eq!(follow.update((0.0, 0.0), (10.0, 0.0), &mut movement, 150.0, OPEN), FollowOutcome::Continue);
        assert!(movement.is_moving());

        follow.update((9.0, 0.0), (10.0, 0.0), &mut movement, 150.0, OPEN);
        assert_eq!(movement, Movement::Idle);
    }

    #[test]
    fn following_stops_when_the_target_is_out_of_reach() {
        let mut follow = Follow::default();
        let mut movement = Movement::Idle;
        assert_eq!(follow.update((0.0, 0.0), (500.0, 0.0), &mut movement, 150.0, OPEN), FollowOutcome::Stop);
        assert_eq!(movement, Movement::Idle);
    }
}
