//! The game itself: acting on players' (validated) requests, simulating the
//! world each tick, and telling clients what happened. The only code that
//! changes players while they play.

use bevy::prelude::*;
use venture_protocol::{EntityId, EntityState, ServerMessage};
use venture_simulation::{
    Follow, FollowOutcome, INTERACT_DISTANCE, MELEE_RANGE, Movement, clamp_move_goal, plan_route, step,
};

use crate::abilities::PendingAbility;
use crate::combat::{Attacking, Health, Resting};
use crate::creatures::{Creature, Dead};
use crate::entities::{DisplayName, Following, Mover, NetworkId, PendingInteraction, Player, Position};
use crate::link::Link;
use crate::npcs::{Npc, NpcInteraction};
use crate::requests::{PlayerAction, PlayerRequest};
use crate::world::ServerWorld;
use crate::{GameRules, ServerSystems};

/// Players within this distance (in metres) see an emote.
const EMOTE_RANGE: f32 = 40.0;

/// Runs the game.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tick>().add_systems(
            FixedUpdate,
            (act_on_requests, sit_down, update_followers, update_attackers, update_casters, simulate, send_snapshot)
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
    mut movers: Query<(&Position, &mut Mover)>,
    entities: Query<(Entity, &NetworkId, &Position, &DisplayName)>,
    npcs: Query<(), With<Npc>>,
    creatures: Query<(), With<Creature>>,
    dead: Query<(), With<Dead>>,
    players: Query<(&Player, &NetworkId, &DisplayName)>,
) {
    let find = |id: EntityId| entities.iter().find(|(_, network_id, ..)| network_id.0 == id);
    for request in requests.read() {
        let player = request.player;
        let Ok((&Position(position), mut mover)) = movers.get_mut(player) else { continue };
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
                mover.0 = plan_route(position, goal, |x, y| world.is_blocked(x, y));
            }
            PlayerAction::Steer(direction) => {
                stop_following();
                mover.0 = Movement::Steer(*direction);
            }
            PlayerAction::Stop => {
                stop_following();
                mover.0 = Movement::Idle;
            }
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
                } else if creatures.contains(target) {
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
                } else if !creatures.contains(target) {
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
    mut followers: Query<(Entity, &mut Following, &Position, &mut Mover, Option<&PendingInteraction>)>,
) {
    for (entity, mut following, &Position(position), mut mover, pending) in &mut followers {
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
        let outcome = following
            .follow
            .update(position, target, &mut mover.0, rules.0.max_move_distance, |x, y| world.is_blocked(x, y));
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
    mut attackers: Query<(Entity, &mut Attacking, &Position, &mut Mover)>,
) {
    for (entity, mut attacking, &Position(position), mut mover) in &mut attackers {
        let Ok(&Position(target)) = targets.get(attacking.target) else {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<Attacking>();
            continue;
        };
        if distance(position, target) <= MELEE_RANGE {
            mover.0 = Movement::Idle;
            continue;
        }
        let outcome = attacking
            .follow
            .update(position, target, &mut mover.0, rules.0.max_move_distance, |x, y| world.is_blocked(x, y));
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
    mut casters: Query<(Entity, &mut PendingAbility, &Position, &mut Mover), Without<Attacking>>,
) {
    for (entity, mut pending, &Position(position), mut mover) in &mut casters {
        let Ok(&Position(target)) = targets.get(pending.target) else {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<PendingAbility>();
            continue;
        };
        if distance(position, target) <= pending.reach {
            mover.0 = Movement::Idle;
            continue;
        }
        let outcome = pending
            .follow
            .update(position, target, &mut mover.0, rules.0.max_move_distance, |x, y| world.is_blocked(x, y));
        if outcome == FollowOutcome::Stop {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<PendingAbility>();
        }
    }
}

/// Moves everything for one tick.
fn simulate(time: Res<Time>, world: Res<ServerWorld>, mut movers: Query<(&mut Position, &mut Mover)>) {
    let dt = time.delta_secs();
    for (mut position, mut mover) in &mut movers {
        position.0 = step(position.0, &mut mover.0, dt, |x, y| world.is_blocked(x, y));
    }
}

/// Tells every client where everything is.
fn send_snapshot(
    mut tick: ResMut<Tick>,
    mut link: ResMut<Link>,
    players: Query<&Player>,
    entities: Query<(&NetworkId, &Position, &Mover, Option<&Health>, Has<Creature>, Has<Resting>)>,
) {
    tick.0 += 1;
    if players.is_empty() {
        return;
    }
    let states: Vec<EntityState> = entities
        .iter()
        .map(|(&NetworkId(id), &Position((x, y)), mover, health, creature, resting)| {
            // Only creatures' health is shown to everyone; players' is private.
            let health = health.filter(|_| creature).map(Health::fraction);
            EntityState { id, x, y, moving: mover.0.is_moving(), health, resting }
        })
        .collect();
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    link.broadcast(everyone, &ServerMessage::Snapshot { tick: tick.0, entities: states });
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
