//! Giving the player's character orders: running somewhere, following
//! someone, or walking over to talk to them. Each order is asked of the
//! server ([`ToServer`]) and predicted locally at once, so it feels instant.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{ClientMessage, EntityId};
use venture_simulation::{Follow, INTERACT_DISTANCE, Movement, clamp_move_goal, plan_route};

use crate::characters::player::{Following, GameRules, LocalPlayer};
use crate::common::coords::{from_bevy, to_bevy};
use crate::interaction::move_marker::{MarkerAssets, MoveMarker, place_marker};
use crate::network::ToServer;
use crate::world::query::{ground_height, is_blocked};
use crate::world::streaming::{ChunkStore, ObstacleStore};

/// Everything needed to give the player's character orders.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Orders<'w, 's> {
    /// The loaded terrain.
    pub store: Res<'w, ChunkStore>,
    obstacles: Res<'w, ObstacleStore>,
    rules: Res<'w, GameRules>,
    marker_assets: Res<'w, MarkerAssets>,
    markers: Query<'w, 's, Entity, With<MoveMarker>>,
    to_server: MessageWriter<'w, ToServer>,
    player: Option<Single<'w, 's, (&'static Transform, &'static mut LocalPlayer)>>,
    positions: Query<'w, 's, &'static Transform, Without<LocalPlayer>>,
}

impl Orders<'_, '_> {
    /// Asks the server to move towards `goal` (at most the allowed distance
    /// away), and predicts the route locally.
    pub fn move_to(&mut self, commands: &mut Commands, goal: Point) {
        let Some(player) = self.player.as_mut() else { return };
        let (transform, player) = &mut **player;
        let start = from_bevy(transform.translation);
        let goal = clamp_move_goal(start, goal, self.rules.0.max_move_distance);
        self.to_server.write(ToServer(ClientMessage::MoveTo { x: goal.0, y: goal.1 }));

        player.following = None;
        player.movement = plan_route(start, goal, |x, y| is_blocked(&self.obstacles, x, y));
        if let Movement::Route(route) = &player.movement
            && let Some(&(end_x, end_y)) = route.back()
        {
            let end =
                to_bevy(end_x, end_y, ground_height(&self.store, end_x, end_y).unwrap_or(transform.translation.y));
            place_marker(commands, &self.marker_assets, self.markers.iter(), end);
        }
    }

    /// Asks the server to follow another player, and starts predicting it.
    pub fn follow(&mut self, target: Entity, id: EntityId) {
        let Some(player) = self.player.as_mut() else { return };
        self.to_server.write(ToServer(ClientMessage::Follow { target: id }));
        player.1.following = Some(Following { target, follow: Follow::default(), stop_within: None });
    }

    /// Asks the server to talk to an NPC (walking over first), and predicts the walk.
    pub fn talk_to(&mut self, target: Entity, id: EntityId) {
        let Some(player) = self.player.as_mut() else { return };
        self.to_server.write(ToServer(ClientMessage::Interact { target: id }));
        player.1.following =
            Some(Following { target, follow: Follow::default(), stop_within: Some(INTERACT_DISTANCE) });
    }

    /// Asks the server to attack a creature (running into reach first), and
    /// predicts the run.
    pub fn attack(&mut self, target: Entity, id: EntityId) {
        let Some(player) = self.player.as_mut() else { return };
        self.to_server.write(ToServer(ClientMessage::Attack { target: id }));
        player.1.following = Some(Following { target, follow: Follow::default(), stop_within: None });
    }

    /// Asks the server to use an ability on a creature, and predicts running up
    /// to it: all the way for a weapon ability (which goes on attacking), or
    /// until within `reach` for a spell.
    pub fn use_ability_on(&mut self, target: Entity, id: EntityId, ability: &str, reach: f32, melee: bool) {
        let Some(player) = self.player.as_mut() else { return };
        self.to_server.write(ToServer(ClientMessage::UseAbility { ability: ability.to_string(), target: Some(id) }));
        let (transform, player) = &mut **player;
        let stop_within = (!melee).then_some(reach);
        let already_close = self.positions.get(target).is_ok_and(|target| {
            let (from, to) = (from_bevy(transform.translation), from_bevy(target.translation));
            (to.0 - from.0).hypot(to.1 - from.1) <= reach
        });
        if melee || !already_close {
            player.following = Some(Following { target, follow: Follow::default(), stop_within });
        }
    }

    /// Asks the server to use an ability on the player themself (such as
    /// resting, which stops them where they are).
    pub fn use_ability_on_self(&mut self, ability: &str, stops: bool) {
        self.to_server.write(ToServer(ClientMessage::UseAbility { ability: ability.to_string(), target: None }));
        if stops && let Some(player) = self.player.as_mut() {
            player.1.following = None;
            player.1.movement = Movement::Idle;
        }
    }

    /// Whether the player is an admin.
    pub fn is_admin(&self) -> bool {
        self.player.as_ref().is_some_and(|player| player.1.admin)
    }

    /// Asks the server for something without predicting anything (for
    /// requests the server will probably turn down, and explain why).
    pub fn ask(&mut self, message: ClientMessage) {
        self.to_server.write(ToServer(message));
    }
}
