//! The game itself: acting on players' (validated) requests, simulating the
//! world each tick, and telling clients what happened. The only code that
//! changes players while they play.

use bevy::prelude::*;
use venture_protocol::{EntityId, EntityState, ServerMessage};
use venture_simulation::{
    Body, Follow, FollowOutcome, INTERACT_DISTANCE, MELEE_RANGE, MoveBudget, Movement, RUN_SPEED, check_move,
    clamp_move_goal, plan_route, step,
};

use crate::abilities::PendingAbility;
use crate::collision::Solid;
use crate::combat::{Attacking, Health, Resting};
use crate::entities::{
    ClientMoves, DisplayName, Feet, Following, Mover, NetworkId, PendingInteraction, Player, Position, RunSpeed,
};
use crate::link::Link;
use crate::monsters::{Dead, Monster};
use crate::npcs::{Npc, NpcInteraction};
use crate::requests::{PlayerAction, PlayerRequest};
use crate::world::ServerWorld;
use crate::{GameRules, ServerSystems};

/// How high a character put at `at` stands: on the lowest place there that
/// can be stood on (the ground, not a roof or a wall's top), or failing that
/// on the highest solid surface.
fn standing_height(world: &ServerWorld, solid: &Solid, at: (f32, f32)) -> Option<f32> {
    let highest = solid.0.surface_below(at, HIGHEST, HIGHEST - LOWEST)?;
    let lowest = world.nav.standing_near(at, 0.5).map(|[_, _, height]| height);
    // (The navigation mesh sits a little above what it's on: the solid surface under it.)
    Some(lowest.and_then(|height| solid.0.surface_below(at, height + 0.5, 2.0)).unwrap_or(highest))
}

/// A player whose client hasn't reported a move for this long (seconds) has stopped.
const STOPPED_AFTER: f32 = 0.3;

/// The highest and lowest anything in the world is, in metres (for finding the ground under a point).
const HIGHEST: f32 = 2000.0;
const LOWEST: f32 = -500.0;

/// Players within this distance (in metres) see an emote.
const EMOTE_RANGE: f32 = 40.0;

/// Runs the game.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tick>().add_systems(
            FixedUpdate,
            (
                act_on_requests,
                accept_moves,
                sit_down,
                update_followers,
                update_attackers,
                update_casters,
                simulate,
                place_players,
                send_snapshot,
            )
                .chain()
                .in_set(ServerSystems::Gameplay),
        );
    }
}

/// How many ticks the server has run.
#[derive(Resource, Default)]
struct Tick(u64);

fn act_on_requests(
    mut commands: Commands,
    mut requests: MessageReader<PlayerRequest>,
    mut link: ResMut<Link>,
    world: Res<ServerWorld>,
    rules: Res<GameRules>,
    mut interactions: MessageWriter<NpcInteraction>,
    mut movers: Query<(&Position, &mut Mover, &mut Feet)>,
    entities: Query<(Entity, &NetworkId, &Position, &DisplayName)>,
    npcs: Query<(), With<Npc>>,
    monsters: Query<(), With<Monster>>,
    dead: Query<(), With<Dead>>,
    players: Query<(&Player, &NetworkId, &DisplayName)>,
) {
    let find = |id: EntityId| entities.iter().find(|(_, network_id, ..)| network_id.0 == id);
    for request in requests.read() {
        let player = request.player;
        let Ok((&Position(position), mut mover, mut feet)) = movers.get_mut(player) else { continue };
        // Tells the player something (such as why what they asked for didn't happen).
        let mut tell = |text: String| {
            if let Ok((me, ..)) = players.get(player) {
                link.send(me.connection, &ServerMessage::Notice { text });
            }
        };
        // Any new order replaces following, walking over to talk, fighting and resting.
        let mut stop_following = || {
            commands.entity(player).remove::<(Following, PendingInteraction, Attacking, PendingAbility, Resting)>();
        };
        match &request.action {
            PlayerAction::MoveTo(goal) => {
                stop_following();
                let goal = clamp_move_goal(position, *goal, rules.0.max_move_distance);
                mover.0 = plan_route(&world.nav, [position.0, position.1, feet.body.height], goal);
            }
            PlayerAction::Steer(direction) => {
                stop_following();
                mover.0 = Movement::Steer(*direction);
            }
            PlayerAction::Stop => {
                stop_following();
                mover.0 = Movement::Idle;
            }
            // (See `accept_moves`.)
            PlayerAction::Moved { .. } => {}
            PlayerAction::Follow(target_id) => {
                // An unknown id is someone who has just left: nothing to say.
                let Some((target, _, &Position(target_position), name)) = find(*target_id) else { continue };
                if target == player {
                    tell("You can't follow yourself.".to_string());
                } else if !players.contains(target) {
                    tell(format!("You can't follow {}. Only other players can be followed.", name.0));
                } else if distance(position, target_position) > rules.0.max_move_distance {
                    tell(format!("{} is too far away to follow.", name.0));
                } else {
                    stop_following();
                    commands.entity(player).insert(Following { target, follow: Follow::default() });
                }
            }
            PlayerAction::Interact(target_id) => {
                let Some((target, _, &Position(target_position), name)) = find(*target_id) else { continue };
                if target == player {
                    tell("Talking to yourself won't get you far.".to_string());
                } else if monsters.contains(target) {
                    tell(format!("The {} doesn't have much to say.", name.0));
                } else if !npcs.contains(target) {
                    tell(format!("{} is another player. To talk to them, use the chat box.", name.0));
                } else if distance(position, target_position) > rules.0.max_move_distance {
                    tell(format!("{} is too far away to talk to.", name.0));
                } else if distance(position, target_position) <= INTERACT_DISTANCE {
                    stop_following();
                    mover.0 = Movement::Idle;
                    interactions.write(NpcInteraction { npc: target, player });
                } else {
                    // Walk over, then talk (see `update_followers`).
                    stop_following();
                    commands
                        .entity(player)
                        .insert((Following { target, follow: Follow::default() }, PendingInteraction { target }));
                }
            }
            PlayerAction::Attack(target_id) => {
                let Some((target, _, &Position(target_position), name)) = find(*target_id) else { continue };
                if dead.contains(target) {
                    continue;
                } else if players.contains(target) {
                    tell(format!("You can't attack {}. Players can't fight each other.", name.0));
                } else if !monsters.contains(target) {
                    tell(format!("You can't attack {}.", name.0));
                } else if distance(position, target_position) > rules.0.max_move_distance {
                    tell(format!("The {} is too far away to attack.", name.0));
                } else {
                    // Run into reach (see `update_attackers`), then combat takes over.
                    stop_following();
                    commands.entity(player).insert(Attacking { target, cooldown: 0.0, follow: Follow::default() });
                }
            }
            PlayerAction::ReturnToTown => {
                stop_following();
                // The town square, whatever development overrides say about where new characters start.
                commands.entity(player).insert(Position(world.manifest.spawn_point));
                mover.0 = Movement::Idle;
                tell("You return to town.".to_string());
            }
            PlayerAction::Emote(emote) => {
                let Ok((_, &NetworkId(id), _)) = players.get(player) else { continue };
                let nearby: Vec<_> = players
                    .iter()
                    .filter(|(_, other, _)| {
                        find(other.0).is_some_and(|(_, _, &Position(at), _)| distance(at, position) <= EMOTE_RANGE)
                    })
                    .map(|(viewer, ..)| viewer.connection)
                    .collect();
                link.broadcast(nearby, &ServerMessage::Emoted { id, emote: *emote });
            }
            PlayerAction::Jump => {
                // Only from the ground (so one at a time).
                if !feet.body.jump(&rules.0.movement) {
                    continue;
                }
                let Ok((_, &NetworkId(id), _)) = players.get(player) else { continue };
                // Jumping stands a resting player up, but doesn't stop them moving.
                commands.entity(player).remove::<Resting>();
                let nearby: Vec<_> = players
                    .iter()
                    .filter(|(_, other, _)| {
                        other.0 != id
                            && find(other.0)
                                .is_some_and(|(_, _, &Position(at), _)| distance(at, position) <= EMOTE_RANGE)
                    })
                    .map(|(viewer, ..)| viewer.connection)
                    .collect();
                link.broadcast(nearby, &ServerMessage::Jumped { id });
            }
            PlayerAction::Chat(text) => {
                let Ok((_, &NetworkId(from), name)) = players.get(player) else { continue };
                let message = ServerMessage::Chat { from, name: name.0.clone(), text: text.clone() };
                let everyone: Vec<_> = players.iter().map(|(player, ..)| player.connection).collect();
                link.broadcast(everyone, &message);
            }
        }
    }
}

/// Steers followers towards whoever they follow; stops those whose target has
/// left or got out of reach; and starts conversations once close enough.
fn update_followers(
    mut commands: Commands,
    world: Res<ServerWorld>,
    rules: Res<GameRules>,
    positions: Query<&Position>,
    mut interactions: MessageWriter<NpcInteraction>,
    mut followers: Query<(Entity, &mut Following, &Position, &Feet, &mut Mover, Option<&PendingInteraction>)>,
) {
    for (entity, mut following, &Position(position), feet, mut mover, pending) in &mut followers {
        let Ok(&Position(target)) = positions.get(following.target) else {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<(Following, PendingInteraction)>();
            continue;
        };
        if let Some(pending) = pending
            && distance(position, target) <= INTERACT_DISTANCE
        {
            mover.0 = Movement::Idle;
            interactions.write(NpcInteraction { npc: pending.target, player: entity });
            commands.entity(entity).remove::<(Following, PendingInteraction)>();
            continue;
        }
        let outcome = following.follow.update(position, target, &mut mover.0, rules.0.max_move_distance, |from, to| {
            plan_route(&world.nav, [from.0, from.1, feet.body.height], to)
        });
        if outcome == FollowOutcome::Stop {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<(Following, PendingInteraction)>();
        }
    }
}

/// Keeps players who are attacking something within reach of it (combat
/// does the attacking), and stops them when it's dead or gone.
fn update_attackers(
    mut commands: Commands,
    world: Res<ServerWorld>,
    rules: Res<GameRules>,
    targets: Query<&Position, Without<Dead>>,
    mut attackers: Query<(Entity, &mut Attacking, &Position, &Feet, &mut Mover)>,
) {
    for (entity, mut attacking, &Position(position), feet, mut mover) in &mut attackers {
        let Ok(&Position(target)) = targets.get(attacking.target) else {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<Attacking>();
            continue;
        };
        if distance(position, target) <= MELEE_RANGE {
            mover.0 = Movement::Idle;
            continue;
        }
        let outcome = attacking.follow.update(position, target, &mut mover.0, rules.0.max_move_distance, |from, to| {
            plan_route(&world.nav, [from.0, from.1, feet.body.height], to)
        });
        if outcome == FollowOutcome::Stop {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<Attacking>();
        }
    }
}

/// Players who have just sat down to rest stop whatever they were doing.
fn sit_down(mut commands: Commands, mut resters: Query<(Entity, &mut Mover), Added<Resting>>) {
    for (entity, mut mover) in &mut resters {
        mover.0 = Movement::Idle;
        commands.entity(entity).remove::<(Following, PendingInteraction, Attacking, PendingAbility)>();
    }
}

/// Runs players into range of the target of an ability they're waiting to use
/// (see [`crate::abilities`], which uses it once they're close enough). Players
/// also attacking the target are already being kept in reach.
fn update_casters(
    mut commands: Commands,
    world: Res<ServerWorld>,
    rules: Res<GameRules>,
    targets: Query<&Position, Without<Dead>>,
    mut casters: Query<(Entity, &mut PendingAbility, &Position, &Feet, &mut Mover), Without<Attacking>>,
) {
    for (entity, mut pending, &Position(position), feet, mut mover) in &mut casters {
        let Ok(&Position(target)) = targets.get(pending.target) else {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<PendingAbility>();
            continue;
        };
        if distance(position, target) <= pending.reach {
            mover.0 = Movement::Idle;
            continue;
        }
        let outcome = pending.follow.update(position, target, &mut mover.0, rules.0.max_move_distance, |from, to| {
            plan_route(&world.nav, [from.0, from.1, feet.body.height], to)
        });
        if outcome == FollowOutcome::Stop {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<PendingAbility>();
        }
    }
}

/// Players' clients move their own characters (predicting with the same
/// rules, so it's instant and never rolls back), and say where they've got
/// to. Each move is checked ([`check_move`]: no faster than they run, not
/// into or through anything solid, no higher than a jump); one that isn't
/// allowed puts the character back where it was (a new placement), and
/// moves from before a placement are ignored.
fn accept_moves(
    time: Res<Time>,
    solid: Res<Solid>,
    rules: Res<GameRules>,
    mut requests: MessageReader<PlayerRequest>,
    mut players: Query<(&mut Position, &mut Feet, &mut ClientMoves, Option<&Health>), With<Player>>,
) {
    let settings = rules.0.movement.character();
    for request in requests.read() {
        let PlayerAction::Moved { to, placement } = request.action else { continue };
        let Ok((mut position, mut feet, mut moves, health)) = players.get_mut(request.player) else { continue };
        if placement != moves.placement || health.is_some_and(|health| health.current() == 0) {
            debug!("A move from placement {placement} was ignored (the character's is {}).", moves.placement);
            continue;
        }
        let from = [position.0.0, position.0.1, feet.body.height];
        match check_move(&solid.0, from, to, &mut moves.budget, &rules.0.movement) {
            Ok(()) => {
                position.0 = (to[0], to[1]);
                feet.body.height = to[2];
                feet.body.grounded = solid.0.stands_on_ground((to[0], to[1], to[2]), &settings);
                if feet.body.grounded {
                    // (Landed: a jump the server allowed is over.)
                    feet.body.rising = 0.0;
                }
                feet.at = Some(position.0);
                moves.last_moved = time.elapsed_secs();
                trace!("A move to {to:?} was allowed.");
            }
            Err(problem) => {
                debug!("A move from {from:?} to {to:?} wasn't allowed: {problem:?}.");
                // (Placed again where it was: see `place_players`.)
                feet.at = None;
                moves.put_back = true;
            }
        }
    }
}

/// Tells players' clients where the server has put their characters: when
/// they arrive, are moved by the server (`/town`, a teleport, respawning), or
/// made a move that wasn't allowed. Those are found somewhere they didn't
/// move to, and are stood on the ground there first. Players who've
/// stopped reporting moves have stopped moving.
fn place_players(
    time: Res<Time>,
    world: Res<ServerWorld>,
    solid: Res<Solid>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &Position, &mut Feet, &mut ClientMoves, &mut Mover, Option<&RunSpeed>)>,
) {
    let (dt, now) = (time.delta_secs(), time.elapsed_secs());
    for (player, position, mut feet, mut moves, mut mover, speed) in &mut players {
        moves.budget.fill(speed.map_or(RUN_SPEED, |speed| speed.0), dt);
        // (A new order counts as moving, until the moves stop coming.)
        if mover.is_changed() {
            moves.last_moved = now;
        }
        if mover.0.is_moving() && now - moves.last_moved > STOPPED_AFTER {
            mover.0 = Movement::Idle;
        }
        if feet.at == Some(position.0) {
            continue;
        }
        // (Put back after a move that wasn't allowed, it keeps its height; anywhere else, on the ground.)
        let height = if moves.put_back { Some(feet.body.height) } else { standing_height(&world, &solid, position.0) };
        let Some(height) = height else { continue };
        moves.put_back = false;
        feet.body = Body::standing(height);
        feet.at = Some(position.0);
        moves.placement += 1;
        moves.budget = MoveBudget::default();
        mover.0 = Movement::Idle;
        let (x, y) = position.0;
        link.send(player.connection, &ServerMessage::Placed { x, y, height, placement: moves.placement });
    }
}

/// Moves everything but players for one tick, through the solid world.
/// Anything found somewhere it didn't move to (just arrived, or moved by a
/// script) is stood on the ground there first.
fn simulate(
    time: Res<Time>,
    world: Res<ServerWorld>,
    solid: Res<Solid>,
    rules: Res<GameRules>,
    mut movers: Query<(&mut Position, &mut Mover, &mut Feet, Option<&RunSpeed>), Without<Player>>,
) {
    let dt = time.delta_secs();
    for (mut position, mut mover, mut feet, speed) in &mut movers {
        if feet.at != Some(position.0) {
            let Some(ground) = standing_height(&world, &solid, position.0) else { continue };
            feet.body = Body::standing(ground);
        }
        let speed = speed.map_or(RUN_SPEED, |speed| speed.0);
        let moved = step(position.0, &mut feet.body, &mut mover.0, speed, dt, &solid.0, &rules.0.movement);
        position.0 = moved;
        feet.at = Some(moved);
    }
}

/// Tells every client where everything is.
fn send_snapshot(
    mut tick: ResMut<Tick>,
    mut link: ResMut<Link>,
    players: Query<&Player>,
    entities: Query<(&NetworkId, &Position, &Mover, &Feet, Option<&Health>, Has<Monster>, Has<Resting>)>,
) {
    tick.0 += 1;
    if players.is_empty() {
        return;
    }
    let states: Vec<EntityState> = entities
        .iter()
        .map(|(&NetworkId(id), &Position((x, y)), mover, feet, health, monster, resting)| {
            // Only monsters' health is shown to everyone; players' is private.
            let health = health.filter(|_| monster).map(Health::fraction);
            let moving = mover.0.is_moving() || !feet.body.grounded;
            EntityState { id, x, y, height: feet.body.height, moving, health, resting }
        })
        .collect();
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    link.broadcast(everyone, &ServerMessage::Snapshot { tick: tick.0, entities: states });
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
