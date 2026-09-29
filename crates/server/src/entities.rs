//! The components that make up things in the game world: players and NPCs
//! alike are *entities* with a network id, a name, a position and movement.
//!
//! Who may change them:
//! - [`crate::sessions`] creates and removes players,
//! - [`crate::npcs`] creates, removes and moves NPCs (including on behalf of scripts),
//! - [`crate::gameplay`] acts on players' requests and runs the simulation.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{EntityId, EntityKind, ServerMessage};
use venture_simulation::{Body, Follow, Movement};

use crate::link::ConnectionId;
use crate::profiles::ProfileKey;

/// Hands out entity ids. Ids aren't reused while the server runs.
#[derive(Resource, Default)]
pub struct EntityIds(u32);

impl EntityIds {
    /// A fresh id.
    pub fn allocate(&mut self) -> EntityId {
        self.0 += 1;
        EntityId(self.0)
    }
}

/// The id clients know an entity by.
#[derive(Component, Clone, Copy)]
pub struct NetworkId(pub EntityId);

/// The name shown for an entity.
#[derive(Component, Clone)]
pub struct DisplayName(pub String);

/// Where an entity is, in metres.
#[derive(Component, Clone, Copy)]
pub struct Position(pub Point);

/// How an entity is moving.
#[derive(Component, Default)]
#[require(Feet)]
pub struct Mover(pub Movement);

/// How high an entity's feet are, and how it's rising or falling (see
/// [`Body`]), and where it was when it last moved: an entity found somewhere
/// else (just arrived, or moved by `/town` or an admin) is stood on the
/// ground there first.
#[derive(Component, Default, Clone, Copy)]
pub struct Feet {
    /// Its body.
    pub body: Body,
    /// Where it was when it last moved (`None`: it hasn't yet).
    pub at: Option<Point>,
}

/// How fast an entity runs, in metres per second, if not the usual
/// [`venture_simulation::RUN_SPEED`] (set by admins with `/setspeed`; not saved).
#[derive(Component, Clone, Copy)]
pub struct RunSpeed(pub f32);

/// A player in the game.
#[derive(Component)]
pub struct Player {
    /// The connection they play through.
    pub connection: ConnectionId,
    /// Which profile they're saved to.
    pub profile: ProfileKey,
}

/// An entity following another.
#[derive(Component)]
pub struct Following {
    /// The entity being followed.
    pub target: Entity,
    /// The state of the chase.
    pub follow: Follow,
}

/// A player walking over to an NPC to talk to them, once close enough.
#[derive(Component)]
pub struct PendingInteraction {
    /// The NPC.
    pub target: Entity,
}

/// The message announcing an entity to clients.
pub fn appeared(id: EntityId, name: &DisplayName, kind: EntityKind, Position(position): Position) -> ServerMessage {
    ServerMessage::EntityAppeared { id, name: name.0.clone(), kind, position }
}
