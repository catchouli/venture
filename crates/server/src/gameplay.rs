//! The game itself: acting on players' (validated) requests, simulating the
//! world each tick, and telling clients what happened. The only code that
//! changes players while they play.

use bevy::prelude::*;
use venture_protocol::{PlayerState, ServerMessage};
use venture_simulation::{Movement, plan_route, step};

use crate::ServerSystems;
use crate::link::Link;
use crate::players::{Mover, Player, Position};
use crate::requests::{PlayerAction, PlayerRequest};
use crate::world::ServerWorld;

/// Runs the game.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tick>().add_systems(
            FixedUpdate,
            (act_on_requests, simulate, send_snapshot).chain().in_set(ServerSystems::Gameplay),
        );
    }
}

/// How many ticks the server has run.
#[derive(Resource, Default)]
struct Tick(u64);

fn act_on_requests(
    mut requests: MessageReader<PlayerRequest>,
    mut link: ResMut<Link>,
    world: Res<ServerWorld>,
    mut players: Query<(&Player, &Position, &mut Mover)>,
) {
    for request in requests.read() {
        match &request.action {
            PlayerAction::MoveTo(goal) => {
                let Ok((_, &Position(position), mut mover)) = players.get_mut(request.player) else { continue };
                mover.0 = plan_route(position, *goal, |x, y| world.is_blocked(x, y));
            }
            PlayerAction::Steer(direction) => {
                let Ok((.., mut mover)) = players.get_mut(request.player) else { continue };
                mover.0 = Movement::Steer(*direction);
            }
            PlayerAction::Stop => {
                let Ok((.., mut mover)) = players.get_mut(request.player) else { continue };
                mover.0 = Movement::Idle;
            }
            PlayerAction::Chat(text) => {
                let Ok((speaker, ..)) = players.get(request.player) else { continue };
                let message = ServerMessage::Chat { from: speaker.id, name: speaker.name.clone(), text: text.clone() };
                let everyone: Vec<_> = players.iter().map(|(player, ..)| player.connection).collect();
                link.broadcast(everyone, &message);
            }
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
