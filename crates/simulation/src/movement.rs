//! How characters move: along a planned route (click to move), or steered in
//! a direction (keyboard).

use std::collections::VecDeque;

use venture_navigation::{Point, find_path};

/// How fast characters run, in metres per second.
pub const RUN_SPEED: f32 = 5.0;

/// How many times per second the server simulates the world.
pub const TICK_RATE: f32 = 20.0;

/// The furthest a steered character moves before checking what's in the
/// way again, in metres (less than a cell, so nothing is hopped over).
const MOST_STEERED_AT_ONCE: f32 = 0.5;

/// The most terrain cells the pathfinder may explore when planning one route.
const MAX_ROUTE_SEARCH_CELLS: usize = 40_000;

/// What a character is doing with its legs.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Movement {
    /// Standing still.
    #[default]
    Idle,
    /// Running through these waypoints, in order.
    Route(VecDeque<Point>),
    /// Running in a direction (a unit vector in world `(x, y)`) until told otherwise.
    Steer(Point),
}

impl Movement {
    /// Whether the character is moving.
    pub fn is_moving(&self) -> bool {
        !matches!(self, Movement::Idle)
    }
}

/// Plans a route from `start` towards `goal`. See [`find_path`] for what
/// happens when the goal can't be reached.
pub fn plan_route(start: Point, goal: Point, is_blocked: impl Fn(i32, i32) -> bool) -> Movement {
    let route: VecDeque<Point> = find_path(start, goal, is_blocked, MAX_ROUTE_SEARCH_CELLS).into();
    if route.is_empty() { Movement::Idle } else { Movement::Route(route) }
}

/// Where a move from `start` towards `goal` should aim, at most `max_distance`
/// metres away (in a straight line). Closer goals are unchanged.
pub fn clamp_move_goal(start: Point, goal: Point, max_distance: f32) -> Point {
    let (dx, dy) = (goal.0 - start.0, goal.1 - start.1);
    let distance = dx.hypot(dy);
    if distance <= max_distance || distance == 0.0 {
        return goal;
    }
    let scale = max_distance / distance;
    (start.0 + dx * scale, start.1 + dy * scale)
}

/// Turns a requested steering direction into a unit vector, or `None` for
/// "stop". Anything that isn't a sensible direction (zero, infinite or NaN)
/// means stop, so a bad request can never make a character move faster.
pub fn steering_direction((x, y): Point) -> Option<Point> {
    let length = x.hypot(y);
    (length.is_finite() && length > 1e-6).then(|| (x / length, y / length))
}

/// Moves a character running at `speed` (metres per second; usually
/// [`RUN_SPEED`]) for `dt` seconds from `position`, and returns where it ends
/// up. Routes shorten as waypoints are reached, and become `Idle` at the end.
pub fn step(
    position: Point,
    movement: &mut Movement,
    speed: f32,
    dt: f32,
    is_blocked: impl Fn(i32, i32) -> bool,
) -> Point {
    let distance = speed.max(0.0) * dt.max(0.0);
    match movement {
        Movement::Idle => position,
        Movement::Steer(direction) => {
            // In short hops, so fast characters can't hop over a thin wall.
            let hops = (distance / MOST_STEERED_AT_ONCE).ceil().max(1.0);
            let hop = (direction.0 * distance / hops, direction.1 * distance / hops);
            (0..hops as u32).fold(position, |at, _| steer(at, hop, &is_blocked))
        }
        Movement::Route(route) => {
            let end = follow_route(position, distance, route);
            if route.is_empty() {
                *movement = Movement::Idle;
            }
            end
        }
    }
}

/// Moves up to `distance` metres along `route` from `(x, y)`, dropping
/// waypoints as they are reached.
fn follow_route((mut x, mut y): Point, distance: f32, route: &mut VecDeque<Point>) -> Point {
    let mut remaining = distance;
    while remaining > 0.0 {
        let Some(&(target_x, target_y)) = route.front() else { break };
        let to_target = (target_x - x).hypot(target_y - y);
        if to_target <= remaining {
            (x, y) = (target_x, target_y);
            remaining -= to_target;
            route.pop_front();
        } else {
            x += (target_x - x) / to_target * remaining;
            y += (target_y - y) / to_target * remaining;
            remaining = 0.0;
        }
    }
    (x, y)
}

/// Moves from `(x, y)` by `offset` without entering blocked cells. If the full
/// move is blocked, slides along whichever axis is free, so characters glide
/// along walls and shores instead of sticking to them.
fn steer((x, y): Point, (dx, dy): Point, is_blocked: impl Fn(i32, i32) -> bool) -> Point {
    let open = |(px, py): Point| !is_blocked(px.floor() as i32, py.floor() as i32);
    [(x + dx, y + dy), (x + dx, y), (x, y + dy)].into_iter().find(|&candidate| open(candidate)).unwrap_or((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: fn(i32, i32) -> bool = |_, _| false;

    #[test]
    fn routes_are_followed_at_running_speed() {
        let mut movement = Movement::Route(VecDeque::from([(10.0, 0.0)]));
        let position = step((0.0, 0.0), &mut movement, RUN_SPEED, 1.0, OPEN);
        assert_eq!(position, (RUN_SPEED, 0.0));
        assert!(movement.is_moving());
    }

    #[test]
    fn routes_end_idle_at_the_destination() {
        let mut movement = Movement::Route(VecDeque::from([(1.0, 0.0), (1.0, 1.0)]));
        let position = step((0.0, 0.0), &mut movement, RUN_SPEED, 1.0, OPEN);
        assert_eq!(position, (1.0, 1.0));
        assert_eq!(movement, Movement::Idle);
    }

    #[test]
    fn steering_slides_along_walls() {
        // A wall along x = 5: moving diagonally into it slides along y.
        let wall = |x: i32, _: i32| x == 5;
        let mut movement = Movement::Steer(steering_direction((1.0, 1.0)).unwrap());
        let position = step((4.9, 0.5), &mut movement, RUN_SPEED, 0.1, wall);
        assert!(position.0 < 5.0 && position.1 > 0.5);
    }

    #[test]
    fn fast_steering_does_not_cross_walls() {
        let wall = |x: i32, _: i32| x == 5;
        let mut movement = Movement::Steer((1.0, 0.0));
        let position = step((4.5, 0.5), &mut movement, RUN_SPEED * 20.0, 0.05, wall);
        assert!(position.0 < 5.0, "stopped at the wall, not past it: {position:?}");
    }

    #[test]
    fn far_goals_are_brought_within_reach() {
        assert_eq!(clamp_move_goal((0.0, 0.0), (300.0, 400.0), 100.0), (60.0, 80.0));
        assert_eq!(clamp_move_goal((0.0, 0.0), (3.0, 4.0), 100.0), (3.0, 4.0));
    }

    #[test]
    fn bad_steering_requests_mean_stop() {
        assert_eq!(steering_direction((0.0, 0.0)), None);
        assert_eq!(steering_direction((f32::NAN, 1.0)), None);
        assert_eq!(steering_direction((f32::INFINITY, 0.0)), None);
        let (x, y) = steering_direction((300.0, 400.0)).unwrap();
        assert!((x.hypot(y) - 1.0).abs() < 1e-6, "huge vectors are normalised, never faster");
    }
}
