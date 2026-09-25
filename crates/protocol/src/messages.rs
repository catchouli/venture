//! The message types.

use serde::{Deserialize, Serialize};
use venture_simulation::Rules;

/// Identifies something in the game world (a player or an NPC) for as long
/// as it's there. Ids aren't reused while the server runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u32);

/// What kind of thing an entity is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    /// Another player.
    Player,
    /// A non-player character.
    Npc {
        /// Its appearance (an id from the content pack's appearances).
        appearance: String,
    },
}

/// Commands from a client. Each is a *request*; the server decides what happens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// The first message on a connection.
    Hello {
        /// The client's [`crate::PROTOCOL_VERSION`].
        protocol_version: u32,
        /// The client's login token (see `docs/client-server.md`).
        token: String,
    },
    /// The name for a new character, after the server asks with [`ServerMessage::NameNeeded`].
    ChooseName {
        /// The requested name.
        name: String,
    },
    /// Run to a point, finding a way around obstacles.
    MoveTo {
        /// Destination, in metres.
        x: f32,
        /// Destination, in metres.
        y: f32,
    },
    /// Keep running in a direction until told otherwise.
    Steer {
        /// Direction in world `(x, y)`. Its length doesn't matter.
        x: f32,
        /// See `x`.
        y: f32,
    },
    /// Stop moving (and stop following anyone).
    Stop,
    /// Follow another player or an NPC, staying close behind them.
    Follow {
        /// Who to follow.
        target: EntityId,
    },
    /// Talk to an NPC, walking over to them first if needed.
    Interact {
        /// Who to talk to.
        target: EntityId,
    },
    /// Say something in chat.
    Chat {
        /// What to say.
        text: String,
    },
}

/// Events from the server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// This token has no character yet: the client should ask the player for
    /// a name and reply with [`ClientMessage::ChooseName`].
    NameNeeded {
        /// A random name to offer.
        suggestion: String,
    },
    /// The chosen name isn't allowed; the client should ask again.
    NameRejected {
        /// Why, in words to show the player.
        reason: String,
    },
    /// The player is in the game.
    Welcome {
        /// The player's own entity id.
        you: EntityId,
        /// The character's name.
        name: String,
        /// Where the character is, in metres.
        position: (f32, f32),
        /// The server's game rules, which the client's prediction must follow too.
        rules: Rules,
    },
    /// The server won't accept this connection (for example, a different
    /// protocol version). The connection is closed after this.
    Rejected {
        /// Why, in words to show the player.
        reason: String,
    },
    /// Something is in the game: sent for everything already there when you
    /// join, then for each newcomer.
    EntityAppeared {
        /// Its id.
        id: EntityId,
        /// Its name.
        name: String,
        /// What it is.
        kind: EntityKind,
        /// Where it is, in metres.
        position: (f32, f32),
    },
    /// Something has left the game.
    EntityLeft {
        /// Its id.
        id: EntityId,
    },
    /// Where everything is, sent every server tick. Includes the receiving player.
    Snapshot {
        /// The server tick this describes.
        tick: u64,
        /// Every entity's state.
        entities: Vec<EntityState>,
    },
    /// Something a player or NPC said.
    Chat {
        /// Who said it.
        from: EntityId,
        /// Their name, as it was when they said it.
        name: String,
        /// What they said (already cleaned by the server).
        text: String,
    },
    /// A message from the server itself, shown in chat.
    Notice {
        /// The message.
        text: String,
    },
    /// The content pack has changed: clients should fetch the new content manifest.
    ContentChanged {
        /// The new content version.
        version: String,
    },
}

/// One entity's state in a [`ServerMessage::Snapshot`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityState {
    /// Which entity.
    pub id: EntityId,
    /// Position in metres.
    pub x: f32,
    /// Position in metres.
    pub y: f32,
    /// Whether it is moving.
    pub moving: bool,
}
