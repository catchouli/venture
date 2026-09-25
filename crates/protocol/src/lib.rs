//! The messages exchanged between the client and the server.
//!
//! The protocol is a small set of **commands** from the client ("I'd like to
//! move there", "say this") and **events** from the server ("this entity
//! appeared", "here is where everyone is"). Players and NPCs are both
//! *entities*, identified by an [`EntityId`]. The server never trusts a command:
//! it validates each one and decides what actually happens. The client's
//! world is only a view of the events it receives. See `docs/client-server.md`.
//!
//! Messages are encoded with [postcard] (a compact binary format for
//! [serde]) and sent as binary WebSocket messages.

mod messages;
mod rules;

pub use messages::{
    ClientMessage, DialogueLine, EntityId, EntityKind, EntityState, ItemStack, ServerMessage, ShopStock,
};
pub use rules::{MAX_CHAT_CHARS, MAX_NAME_CHARS, MIN_NAME_CHARS, clean_chat_text, clean_name, is_valid_token};

/// The protocol version. Increase it whenever messages change; the server
/// turns away clients speaking a different version.
pub const PROTOCOL_VERSION: u32 = 6;

/// The largest message a client may send, in bytes. Anything bigger is
/// refused before decoding.
pub const MAX_CLIENT_MESSAGE_BYTES: usize = 2048;

/// A message that couldn't be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The message is bigger than allowed.
    TooLarge,
    /// The bytes aren't a valid message.
    Malformed,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::TooLarge => write!(f, "message too large"),
            DecodeError::Malformed => write!(f, "malformed message"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl ClientMessage {
    /// Encodes the message for sending.
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("client messages always serialise")
    }

    /// Decodes a message received from a client, refusing oversized ones.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_CLIENT_MESSAGE_BYTES {
            return Err(DecodeError::TooLarge);
        }
        postcard::from_bytes(bytes).map_err(|_| DecodeError::Malformed)
    }
}

impl ServerMessage {
    /// Encodes the message for sending.
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("server messages always serialise")
    }

    /// Decodes a message received from the server.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        postcard::from_bytes(bytes).map_err(|_| DecodeError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_survive_a_round_trip() {
        let command = ClientMessage::MoveTo { x: 1.5, y: -2.0 };
        assert_eq!(ClientMessage::decode(&command.encode()).unwrap(), command);

        let event = ServerMessage::Snapshot {
            tick: 42,
            entities: vec![EntityState { id: EntityId(7), x: 1.0, y: 2.0, moving: true }],
        };
        assert_eq!(ServerMessage::decode(&event.encode()).unwrap(), event);
    }

    #[test]
    fn oversized_and_malformed_messages_are_refused() {
        assert_eq!(ClientMessage::decode(&vec![0; MAX_CLIENT_MESSAGE_BYTES + 1]), Err(DecodeError::TooLarge));
        assert_eq!(ClientMessage::decode(&[250, 250, 250]), Err(DecodeError::Malformed));
    }
}
