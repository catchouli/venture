//! The game itself: acting on players' (validated) requests, simulating the
//! world each tick, and telling clients what happened. The only code that
//! changes players while they play.

use bevy::prelude::*;
use venture_protocol::{PlayerState, ServerMessage};
use venture_simulation::{Follow, FollowOutcome, Movement, clamp_move_goal, plan_route, step};

use crate::link::Link;
use crate::players::{Following, Mover, Player, Position};
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
    mut players: Query<(Entity, &Player, &Position, &mut Mover)>,
) {
    for request in requests.read() {
        // Any movement command replaces following.
        if !matches!(request.action, PlayerAction::Chat(_)) {
            commands.entity(request.player).remove::<Following>();
        }
        match &request.action {
            PlayerAction::MoveTo(goal) => {
                let Ok((_, _, &Position(position), mut mover)) = players.get_mut(request.player) else { continue };
                let goal = clamp_move_goal(position, *goal, rules.0.max_move_distance);
                mover.0 = plan_route(position, goal, |x, y| world.is_blocked(x, y));
            }
            PlayerAction::Steer(direction) => {
                let Ok((.., mut mover)) = players.get_mut(request.player) else { continue };
                mover.0 = Movement::Steer(*direction);
            }
            PlayerAction::Stop => {
                let Ok((.., mut mover)) = players.get_mut(request.player) else { continue };
                mover.0 = Movement::Idle;
            }
            PlayerAction::Follow(target_id) => {
                let Ok((_, _, &Position(position), _)) = players.get(request.player) else { continue };
                let target =
                    players.iter().find(|(entity, player, ..)| player.id == *target_id && *entity != request.player);
                let Some((target, _, &Position(target_position), _)) = target else { continue };
                let distance = (target_position.0 - position.0).hypot(target_position.1 - position.1);
                if distance <= rules.0.max_move_distance {
                    commands.entity(request.player).insert(Following { target, follow: Follow::default() });
                }
            }
            PlayerAction::Chat(text) => {
                let Ok((_, speaker, ..)) = players.get(request.player) else { continue };
                let message = ServerMessage::Chat { from: speaker.id, name: speaker.name.clone(), text: text.clone() };
                let everyone: Vec<_> = players.iter().map(|(_, player, ..)| player.connection).collect();
                link.broadcast(everyone, &message);
            }
        }
    }
}

/// Steers followers towards whoever they follow, and stops those whose target
/// has left or got out of reach.
fn update_followers(
    mut commands: Commands,
    world: Res<ServerWorld>,
    rules: Res<GameRules>,
    positions: Query<&Position>,
    mut followers: Query<(Entity, &mut Following, &Position, &mut Mover)>,
) {
    for (entity, mut following, &Position(position), mut mover) in &mut followers {
        let outcome = match positions.get(following.target) {
            Ok(&Position(target)) => {
                following
                    .follow
                    .update(position, target, &mut mover.0, rules.0.max_move_distance, |x, y| world.is_blocked(x, y))
            }
            Err(_) => FollowOutcome::Stop,
        };
        if outcome == FollowOutcome::Stop {
            mover.0 = Movement::Idle;
            commands.entity(entity).remove::<Following>();
        }
    }
}

/// Moves every player for one tick.
fn simulate(time: Res<Time>, world: Res<ServerWorld>, mut players: Query<(&mut Position, &mut Mover)>) {
    let dt = time.delta_secs();
    for (mut position, mut mover) in &mut players {
        position.0 = step(position.0, &mut mover.0, dt, |x, y| world.is_blocked(x, y));
    }
}

/// Tells every client where everyone is.
fn send_snapshot(mut tick: ResMut<Tick>, mut link: ResMut<Link>, players: Query<(&Player, &Position, &Mover)>) {
    tick.0 += 1;
    let states: Vec<PlayerState> = players
        .iter()
        .map(|(player, &Position((x, y)), mover)| PlayerState { id: player.id, x, y, moving: mover.0.is_moving() })
        .collect();
    if states.is_empty() {
        return;
    }
    let everyone: Vec<_> = players.iter().map(|(player, ..)| player.connection).collect();
    link.broadcast(everyone, &ServerMessage::Snapshot { tick: tick.0, players: states });
}
