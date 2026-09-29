//! How characters move: along a planned route (click to move), or steered in
//! a direction (keyboard), through the world's solid shapes (see
//! `venture_collision`), falling when there's nothing under them.

use std::collections::VecDeque;

use venture_collision::Collision;
use venture_navigation::{NavWorld, Point, Point3};

use crate::rules::MovementRules;

/// How fast characters run, in metres per second.
pub const RUN_SPEED: f32 = 5.0;

/// How many times per second the server simulates the world.
pub const TICK_RATE: f32 = 20.0;

/// A route's waypoint counts as reached this close to it, in metres.
const REACHED: f32 = 0.05;
/// A character on a route that gets less than this share of the way it
/// should each tick is held up by something.
const HELD_UP: f32 = 0.25;
/// A character held up this long (in seconds) gives up on its route.
const GIVE_UP_AFTER: f32 = 1.0;
/// The most waypoints passed in one step.
const MOST_WAYPOINTS_AT_ONCE: usize = 4;

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

/// A character's body: how high its feet are, and how it's rising or falling.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Body {
    /// The height of its feet, in metres.
    pub height: f32,
    /// How fast it's going up (down, if negative), in metres per second.
    pub rising: f32,
    /// Whether it's standing on something.
    pub grounded: bool,
    /// How long it's been held up on its route, in seconds.
    pub held_up: f32,
}

impl Body {
    /// A body standing at `height`.
    pub fn standing(height: f32) -> Self {
        Self { height, grounded: true, ..Self::default() }
    }

    /// Jumps, if it's standing on something. Returns whether it did.
    pub fn jump(&mut self, rules: &MovementRules) -> bool {
        if !self.grounded {
            return false;
        }
        self.rising = rules.jump_speed();
        self.grounded = false;
        true
    }
}

/// Plans a route over the navigation mesh from `start` (with the height of
/// its feet) to `goal` (or the nearest point to it that can be stood on).
/// Standing still if there's no way.
pub fn plan_route(nav: &NavWorld, start: Point3, goal: Point) -> Movement {
    let Some(route) = nav.route(start, [goal.0, goal.1, start[2]]) else { return Movement::Idle };
    let route: VecDeque<Point> = route.into_iter().map(|[x, y, _]| (x, y)).collect();
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

/// Moves a character at `position` with `body` running at `speed` (metres
/// per second; usually [`RUN_SPEED`]) for `dt` seconds through `world`, and
/// returns where it ends up. Routes shorten as waypoints are reached, and
/// become `Idle` at the end, or once the character has been held up too
/// long. Characters fall when there's nothing under them. Nothing moves
/// where the world isn't loaded.
pub fn step(
    position: Point,
    body: &mut Body,
    movement: &mut Movement,
    speed: f32,
    dt: f32,
    world: &Collision,
    rules: &MovementRules,
) -> Point {
    let dt = dt.max(0.0);
    let mut settings = rules.character();
    // (Only feet on the ground step up onto things: a jump that falls short of a wall's top doesn't climb it.)
    if !body.grounded {
        settings.step_height = 0.0;
    }
    if dt == 0.0 || !world.is_loaded_at(position) {
        return position;
    }
    // Standing still on the ground: nothing to do.
    if !movement.is_moving() && body.grounded && body.rising <= 0.0 {
        return position;
    }
    // (Rising at the tick's average speed, so jumps go as high whatever the tick rate.)
    let before = body.rising;
    body.rising -= rules.gravity * dt;
    let mut feet = (position.0, position.1, body.height);
    let mut rise = (before + body.rising) / 2.0 * dt;
    let mut grounded = body.grounded;
    // Moves by `(dx, dy)` across (and the rise, the first time), and returns how far across it got.
    let mut go = |feet: &mut (f32, f32, f32), (dx, dy): Point| {
        let moved = world.move_character(*feet, (dx, dy, rise), dt, &settings);
        rise = 0.0;
        grounded = moved.grounded;
        let across = (moved.feet.0 - feet.0).hypot(moved.feet.1 - feet.1);
        *feet = moved.feet;
        across
    };
    let distance = speed.max(0.0) * dt;
    match movement {
        Movement::Idle => {
            go(&mut feet, (0.0, 0.0));
        }
        Movement::Steer(direction) => {
            go(&mut feet, (direction.0 * distance, direction.1 * distance));
        }
        Movement::Route(route) => {
            let (mut remaining, mut gone) = (distance, 0.0);
            for _ in 0..MOST_WAYPOINTS_AT_ONCE {
                let Some(&(target_x, target_y)) = route.front() else { break };
                let (dx, dy) = (target_x - feet.0, target_y - feet.1);
                let to_target = dx.hypot(dy);
                if to_target <= REACHED {
                    route.pop_front();
                    continue;
                }
                let wanted = to_target.min(remaining);
                let across = go(&mut feet, (dx / to_target * wanted, dy / to_target * wanted));
                gone += across;
                remaining -= wanted;
                if (target_x - feet.0).hypot(target_y - feet.1) <= REACHED {
                    route.pop_front();
                }
                if across < wanted * 0.9 || remaining <= 1e-4 {
                    break;
                }
            }
            body.held_up = if gone < distance * HELD_UP && !route.is_empty() { body.held_up + dt } else { 0.0 };
            if route.is_empty() || body.held_up >= GIVE_UP_AFTER {
                body.held_up = 0.0;
                *movement = Movement::Idle;
            }
        }
    }
    body.height = feet.2;
    // (Still going up, touching a wall's top edge isn't standing on it.)
    body.grounded = grounded && body.rising <= 0.0;
    if grounded && body.rising < 0.0 {
        body.rising = 0.0;
    }
    (feet.0, feet.1)
}

#[cfg(test)]
mod tests {
    use venture_collision::Collision;
    use venture_world::{Chunk, ChunkPos};

    use super::*;

    /// Flat ground from (0, 0) to (128, 128), at height 0.
    fn flat() -> Collision {
        let mut world = Collision::new();
        for y in 0..2 {
            for x in 0..2 {
                world.add_chunk(&Chunk::new(ChunkPos::new(x, y)));
            }
        }
        world
    }

    /// Runs for `seconds` at 20 ticks a second.
    fn run(world: &Collision, from: Point, body: &mut Body, movement: &mut Movement, seconds: f32) -> Point {
        let rules = MovementRules::default();
        let mut at = from;
        for _ in 0..(seconds * TICK_RATE).round() as usize {
            at = step(at, body, movement, RUN_SPEED, 1.0 / TICK_RATE, world, &rules);
        }
        at
    }

    #[test]
    fn routes_are_followed_at_running_speed() {
        let world = flat();
        let mut movement = Movement::Route(VecDeque::from([(30.0, 10.0)]));
        let (x, y) = run(&world, (10.0, 10.0), &mut Body::standing(0.0), &mut movement, 1.0);
        assert!((x - (10.0 + RUN_SPEED)).abs() < 0.05 && (y - 10.0).abs() < 0.01, "{x}, {y}");
        assert!(movement.is_moving());
    }

    #[test]
    fn routes_end_idle_at_the_destination() {
        let world = flat();
        let mut movement = Movement::Route(VecDeque::from([(11.0, 10.0), (11.0, 11.0)]));
        let (x, y) = run(&world, (10.0, 10.0), &mut Body::standing(0.0), &mut movement, 1.0);
        assert!((x - 11.0).abs() < REACHED && (y - 11.0).abs() < REACHED, "{x}, {y}");
        assert_eq!(movement, Movement::Idle);
    }

    #[test]
    fn steering_goes_on_until_told_otherwise() {
        let world = flat();
        let mut movement = Movement::Steer(steering_direction((1.0, 1.0)).unwrap());
        let (x, y) = run(&world, (10.0, 10.0), &mut Body::standing(0.0), &mut movement, 1.0);
        let gone = (x - 10.0).hypot(y - 10.0);
        assert!((gone - RUN_SPEED).abs() < 0.1, "{gone}");
        assert!(movement.is_moving());
    }

    #[test]
    fn jumps_go_up_as_high_as_the_rules_say_and_come_down() {
        let world = flat();
        let rules = MovementRules::default();
        let mut body = Body::standing(0.0);
        let mut movement = Movement::Idle;
        assert!(body.jump(&rules));
        assert!(!body.jump(&rules), "not again in the air");
        let mut highest = 0.0_f32;
        let mut at = (10.0, 10.0);
        for _ in 0..40 {
            at = step(at, &mut body, &mut movement, RUN_SPEED, 1.0 / TICK_RATE, &world, &rules);
            highest = highest.max(body.height);
        }
        assert!((highest - rules.jump_height).abs() < 0.15, "{highest}");
        assert!(body.grounded && body.height.abs() < 0.05, "landed: {body:?}");
        assert_eq!(at, (10.0, 10.0));
    }

    #[test]
    fn characters_held_up_give_up_on_their_routes() {
        use venture_world::{BakedWall, WallKind, WallSample};
        let mut world = flat();
        // A long wall across the way, at x = 20.
        let samples = (0..=100).map(|y| WallSample { x: 20.0, y: y as f32, ground: 0.0, open: false }).collect();
        let wall = BakedWall { id: "wall".into(), kind: WallKind::DryStone, height: 1.3, seed: 0, samples };
        world.set_structures(&[wall], &[], &[]);
        let mut movement = Movement::Route(VecDeque::from([(30.0, 50.0)]));
        let (x, _) = run(&world, (10.0, 50.0), &mut Body::standing(0.0), &mut movement, 4.0);
        assert!(x < 20.0, "{x}");
        assert_eq!(movement, Movement::Idle, "gave up");
    }

    #[test]
    fn running_jumps_clear_fences_but_not_hedges() {
        use venture_world::{BakedWall, WallKind, WallSample};
        let rules = MovementRules::default();
        let across = |kind: WallKind, height: f32| {
            let mut world = flat();
            let samples = (0..=100).map(|y| WallSample { x: 20.0, y: y as f32, ground: 0.0, open: false }).collect();
            let wall = BakedWall { id: "wall".into(), kind, height, seed: 0, samples };
            world.set_structures(&[wall], &[], &[]);
            let (mut body, mut movement, mut at) = (Body::standing(0.0), Movement::Steer((1.0, 0.0)), (17.0, 50.0));
            for tick in 0..60 {
                // Jumping a little before it.
                if tick == 8 {
                    assert!(body.jump(&rules));
                }
                at = step(at, &mut body, &mut movement, RUN_SPEED, 1.0 / TICK_RATE, &world, &rules);
            }
            at.0
        };
        assert!(across(WallKind::Fence, 1.2) > 21.0, "over the fence");
        assert!(across(WallKind::Hedge, 1.7) < 20.0, "not over the hedge");
    }

    #[test]
    fn nothing_moves_where_the_world_is_not_loaded() {
        let world = Collision::new();
        let mut movement = Movement::Steer((1.0, 0.0));
        assert_eq!(run(&world, (10.0, 10.0), &mut Body::standing(0.0), &mut movement, 1.0), (10.0, 10.0));
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
