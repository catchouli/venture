//! The components that make up a player in the game.
//!
//! Only [`crate::sessions`] creates and removes players, and only
//! [`crate::gameplay`] changes them while they play.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::PlayerId;
use venture_simulation::Movement;

use crate::link::ConnectionId;
use crate::profiles::ProfileKey;

/// A player in the game.
#[derive(Component)]
pub struct Player {
    /// Their id for this session.
    pub id: PlayerId,
    /// The connection they play through.
    pub connection: ConnectionId,
    /// Their character's name.
    pub name: String,
    /// Which profile they're saved to.
    pub profile: ProfileKey,
}

/// Where a player is, in metres.
#[derive(Component, Clone, Copy)]
pub struct Position(pub Point);

/// How a player is moving.
#[derive(Component, Default)]
pub struct Mover(pub Movement);
