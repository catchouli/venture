//! The game itself: acting on players' (validated) requests, simulating the
//! world each tick, and telling clients what happened. The only code that
//! changes players while they play.

use bevy::prelude::*;
use venture_protocol::{EntityId, EntityState, ServerMessage};
use venture_simulation::{Follow, FollowOutcome, INTERACT_DISTANCE, Movement, clamp_move_goal, plan_route, step};

use crate::entities::{DisplayName, Following, Mover, NetworkId, PendingInteraction, Player, Position};
use crate::link::Link;
use crate::npcs::{Npc, NpcInteraction};
use crate::requests::{PlayerAction, PlayerRequest};
use crate::world::ServerWorld;
use crate::{GameRules, ServerSystems};

/// Runs the game.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tick>().add_systems(
            FixedUpdate,
            (act_on_requests, update_followers, simulate, send_snapshot).chain().in_set(ServerSystems::Gameplay),
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
    entities: Query<(Entity, &NetworkId, &Position)>,
    npcs: Query<(), With<Npc>>,
    players: Query<(&Player, &NetworkId, &DisplayName)>,
) {
    let find = |id: EntityId| entities.iter().find(|(_, network_id, _)| network_id.0 == id);
    for request in requests.read() {
        let player = request.player;
        // Any movement command replaces following and walking over to talk.
        if !matches!(request.action, PlayerAction::Chat(_)) {
            commands.entity(player).remove::<(Following, PendingInteraction)>();
        }
        let Ok((&Position(position), mut mover)) = movers.get_mut(player) else { continue };
        match &request.action {
            PlayerAction::MoveTo(goal) => {
                let goal = clamp_move_goal(position, *goal, rules.0.max_move_distance);
                mover.0 = plan_route(position, goal, |x, y| world.is_blocked(x, y));
            }
            PlayerAction::Steer(direction) => mover.0 = Movement::Steer(*direction),
            PlayerAction::Stop => mover.0 = Movement::Idle,
            PlayerAction::Follow(target_id) => {
                let Some((target, _, &Position(target_position))) = find(*target_id).filter(|(e, ..)| *e != player)
                else {
                    continue;
                };
                if distance(position, target_position) <= rules.0.max_move_distance {
                    commands.entity(player).insert(Following { target, follow: Follow::default() });
                }
            }
            PlayerAction::Interact(target_id) => {
                let Some((target, _, &Position(target_position))) =
                    find(*target_id).filter(|(e, ..)| npcs.contains(*e))
                else {
                    continue;
                };
                let gap = distance(position, target_position);
                if gap <= INTERACT_DISTANCE {
                    mover.0 = Movement::Idle;
                    interactions.write(NpcInteraction { npc: target, player });
                } else if gap <= rules.0.max_move_distance {
                    // Walk over, then talk (see `update_followers`).
                    commands
                        .entity(player)
                        .insert((Following { target, follow: Follow::default() }, PendingInteraction { target }));
                }
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
    entities: Query<(&NetworkId, &Position, &Mover)>,
) {
    tick.0 += 1;
    if players.is_empty() {
        return;
    }
    let states: Vec<EntityState> = entities
        .iter()
        .map(|(&NetworkId(id), &Position((x, y)), mover)| EntityState { id, x, y, moving: mover.0.is_moving() })
        .collect();
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    link.broadcast(everyone, &ServerMessage::Snapshot { tick: tick.0, entities: states });
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
