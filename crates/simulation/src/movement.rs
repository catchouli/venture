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

/// How far past the capsule's own sweep a reported move may end (across,
/// in metres): the client moves in many small steps, sliding round corners,
/// where the check sweeps once.
const MOVE_TOLERANCE: f32 = 0.35;
/// How much faster than they run a character's reported moves may add up
/// to (for messages bunched up by the network).
const SPEED_SLACK: f32 = 1.25;
/// How many seconds of running a character may save up (for messages held
/// up by the network, then arriving together).
const MOST_SAVED_UP: f32 = 1.0;
/// How far above the ground (beyond a jump) a character may report being, in metres.
const HEIGHT_SLACK: f32 = 0.5;

/// How far a player's character may still move, by what they've been
/// allowed so far: it fills at their running speed (and a little more),
/// and moves spend it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveBudget(f32);

impl MoveBudget {
    /// Time passes (`dt` seconds, running at `speed`).
    pub fn fill(&mut self, speed: f32, dt: f32) {
        self.0 = (self.0 + speed.max(0.0) * SPEED_SLACK * dt.max(0.0)).min(speed.max(0.0) * MOST_SAVED_UP);
    }

    /// Spends `distance` metres, if there's enough.
    pub fn spend(&mut self, distance: f32) -> bool {
        let enough = distance <= self.0 + 1e-4;
        if enough {
            self.0 = (self.0 - distance).max(0.0);
        }
        enough
    }
}

/// Why a reported move wasn't allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadMove {
    /// Further than the character could have run.
    TooFar,
    /// Through something solid, or into it (or into deep water or lava).
    Blocked,
    /// Higher than a jump reaches.
    TooHigh,
}

/// Checks a move a player's client reports (it moves the player's own
/// character, predicting with [`step`], and says where it got to): from
/// `from` to `to` (each `[x, y, height of the feet]`), with `budget` of
/// running left. Allowed if it's no further than the budget, it doesn't go
/// into or through anything solid (a capsule swept from `from` must get
/// near `to`), and it's no higher off the ground than a jump.
pub fn check_move(
    world: &Collision,
    from: Point3,
    to: Point3,
    budget: &mut MoveBudget,
    rules: &MovementRules,
) -> Result<(), BadMove> {
    let across = (to[0] - from[0]).hypot(to[1] - from[1]);
    if !budget.spend(across) {
        return Err(BadMove::TooFar);
    }
    // (Where the world isn't loaded, only the distance can be checked.)
    if !world.is_loaded_at((from[0], from[1])) || !world.is_loaded_at((to[0], to[1])) {
        return Ok(());
    }
    if !world.fits((to[0], to[1], to[2])) {
        return Err(BadMove::Blocked);
    }
    let settings = rules.character();
    let desired = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
    let swept = world.move_character((from[0], from[1], from[2]), desired, 1.0 / TICK_RATE, &settings).feet;
    if (swept.0 - to[0]).hypot(swept.1 - to[1]) > MOVE_TOLERANCE {
        return Err(BadMove::Blocked);
    }
    let ground = world.surface_below((to[0], to[1]), to[2] + 0.1, 1000.0);
    let rising = to[2] > from[2] + 0.05;
    if rising && ground.is_some_and(|ground| to[2] - ground > rules.jump_height + HEIGHT_SLACK) {
        return Err(BadMove::TooHigh);
    }
    Ok(())
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

    #[test]
    fn honest_moves_are_allowed_and_cheats_are_not() {
        use venture_world::{BakedWall, WallKind, WallSample};
        let mut world = flat();
        let samples = (0..=100).map(|y| WallSample { x: 20.0, y: y as f32, ground: 0.0, open: false }).collect();
        let wall = BakedWall { id: "wall".into(), kind: WallKind::DryStone, height: 1.3, seed: 0, samples };
        world.set_structures(&[wall], &[], &[]);
        let rules = MovementRules::default();
        let mut budget = MoveBudget::default();
        let tick = 1.0 / TICK_RATE;

        // Running along, a tick at a time.
        budget.fill(RUN_SPEED, tick);
        assert_eq!(check_move(&world, [10.0, 10.0, 0.0], [10.25, 10.0, 0.0], &mut budget, &rules), Ok(()));
        // Too far for the time.
        budget.fill(RUN_SPEED, tick);
        assert_eq!(
            check_move(&world, [10.25, 10.0, 0.0], [12.0, 10.0, 0.0], &mut budget, &rules),
            Err(BadMove::TooFar)
        );
        // Through the wall at x = 20, with plenty of time saved up.
        let mut saved = MoveBudget::default();
        saved.fill(RUN_SPEED, 1.0);
        assert_eq!(check_move(&world, [19.3, 50.0, 0.0], [20.7, 50.0, 0.0], &mut saved, &rules), Err(BadMove::Blocked));
        // Into it.
        let mut saved = MoveBudget::default();
        saved.fill(RUN_SPEED, 1.0);
        assert_eq!(check_move(&world, [19.3, 50.0, 0.0], [19.9, 50.0, 0.0], &mut saved, &rules), Err(BadMove::Blocked));
        // Up in the air, higher than a jump.
        let mut saved = MoveBudget::default();
        saved.fill(RUN_SPEED, 1.0);
        assert_eq!(check_move(&world, [10.0, 10.0, 0.0], [10.0, 10.0, 3.0], &mut saved, &rules), Err(BadMove::TooHigh));
        // A jump's height is fine.
        assert_eq!(check_move(&world, [10.0, 10.0, 0.0], [10.1, 10.0, 1.2], &mut saved, &rules), Ok(()));
    }

    #[test]
    fn a_predicted_run_round_a_corner_is_allowed_all_the_way() {
        // The client's own prediction (at its frame rate) sliding along a wall
        // at an angle, reported every tick: every report is allowed.
        use venture_world::{BakedWall, WallKind, WallSample};
        let mut world = flat();
        let samples = (0..=40)
            .map(|i| WallSample { x: 20.0 + i as f32 * 0.7, y: 30.0 + i as f32 * 0.7, ground: 0.0, open: false })
            .collect();
        let wall = BakedWall { id: "wall".into(), kind: WallKind::Hedge, height: 1.7, seed: 0, samples };
        world.set_structures(&[wall], &[], &[]);
        let rules = MovementRules::default();
        let (mut body, mut movement, mut at) = (Body::standing(0.0), Movement::Steer((1.0, 0.0)), (15.0, 35.0));
        let mut budget = MoveBudget::default();
        let mut reported = [at.0, at.1, 0.0];
        let frame = 1.0 / 144.0;
        for i in 1..=(3.0 / frame) as usize {
            at = step(at, &mut body, &mut movement, RUN_SPEED, frame, &world, &rules);
            budget.fill(RUN_SPEED, frame);
            if i % 7 == 0 {
                let now = [at.0, at.1, body.height];
                assert_eq!(check_move(&world, reported, now, &mut budget, &rules), Ok(()), "at {now:?}");
                reported = now;
            }
        }
        assert!(at.0 > 20.0, "slid along it: {at:?}");
    }

    #[test]
    fn predicted_jumps_into_a_hedge_are_allowed_all_the_way() {
        // Running at a hedge (too high to clear) and jumping into it, again
        // and again, as the client predicts it: every report is allowed.
        use venture_world::{BakedWall, WallKind, WallSample};
        let mut world = flat();
        let samples = (0..=100).map(|y| WallSample { x: 20.0, y: y as f32, ground: 0.0, open: false }).collect();
        let wall = BakedWall { id: "hedge".into(), kind: WallKind::Hedge, height: 1.7, seed: 0, samples };
        world.set_structures(&[wall], &[], &[]);
        let rules = MovementRules::default();
        let direction = steering_direction((1.0, 0.3)).unwrap();
        let (mut body, mut movement, mut at) = (Body::standing(0.0), Movement::Steer(direction), (15.0, 40.0));
        let mut budget = MoveBudget::default();
        let mut reported = [at.0, at.1, 0.0];
        let frame = 1.0 / 144.0;
        for i in 1..=(4.0 / frame) as usize {
            if body.grounded && i % 60 == 0 {
                body.jump(&rules);
            }
            at = step(at, &mut body, &mut movement, RUN_SPEED, frame, &world, &rules);
            budget.fill(RUN_SPEED, frame);
            if i % 7 == 0 {
                let now = [at.0, at.1, body.height];
                assert_eq!(check_move(&world, reported, now, &mut budget, &rules), Ok(()), "at {now:?}");
                reported = now;
            }
        }
        assert!(at.0 < 20.0, "never got over: {at:?}");
    }

    #[test]
    fn bridges_are_run_across_over_deep_water() {
        use venture_world::{BridgeKind, BridgePlan, Fluid, FluidBody, FluidKind};
        for kind in BridgeKind::ALL {
            let mut world = Collision::new();
            for y in 0..2 {
                for x in 0..2 {
                    let mut chunk = Chunk::new(ChunkPos::new(x, y));
                    if x == 0 {
                        // A deep river from x = 30 to 40.
                        for cy in 0..=64 {
                            for cx in 31..=39 {
                                chunk.set_corner_height(cx, cy, -3.0);
                            }
                        }
                        for cy in 0..64 {
                            for cx in 30..40 {
                                chunk.cell_mut(cx, cy).fluid =
                                    Some(Fluid { kind: FluidKind::Water, body: FluidBody::River, level: -1.5 });
                            }
                        }
                    }
                    world.add_chunk(&chunk);
                }
            }
            let plan = BridgePlan {
                id: "bridge".into(),
                kind,
                points: vec![(26.0, 20.0), (44.0, 20.0)],
                width: kind.usual_width(),
                curve: kind.usual_curve(),
                seed: 1,
            };
            world.set_bridges(&[venture_structures::bridges::bake_bridge(&plan, |_, _| 0.0)]);
            let mut movement = Movement::Steer((1.0, 0.0));
            let (x, _) = run(&world, (20.0, 20.0), &mut Body::standing(0.0), &mut movement, 6.0);
            assert!(x > 46.0, "{kind:?}: across, not stopped at the water: {x}");
        }
    }
}
