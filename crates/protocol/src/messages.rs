//! The message types.

use serde::{Deserialize, Serialize};

/// Identifies a connected player for the length of their session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

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
    /// Stop moving.
    Stop,
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
        /// The player's id for this session.
        you: PlayerId,
        /// The character's name.
        name: String,
        /// Where the character is, in metres.
        position: (f32, f32),
    },
    /// The server won't accept this connection (for example, a different
    /// protocol version). The connection is closed after this.
    Rejected {
        /// Why, in words to show the player.
        reason: String,
    },
    /// Another player is in the game (sent for everyone already there when you
    /// join, then for each newcomer).
    PlayerJoined {
        /// Their id.
        id: PlayerId,
        /// Their character's name.
        name: String,
        /// Where they are, in metres.
        position: (f32, f32),
    },
    /// A player has left the game.
    PlayerLeft {
        /// Their id.
        id: PlayerId,
    },
    /// Where every player is, sent every server tick. Includes the receiving player.
    Snapshot {
        /// The server tick this describes.
        tick: u64,
        /// Every player's state.
        players: Vec<PlayerState>,
    },
    /// A chat message from a player.
    Chat {
        /// Who said it.
        from: PlayerId,
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
}

/// One player's state in a [`ServerMessage::Snapshot`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    /// Who.
    pub id: PlayerId,
    /// Position in metres.
    pub x: f32,
    /// Position in metres.
    pub y: f32,
    /// Whether they are running.
    pub moving: bool,
}
