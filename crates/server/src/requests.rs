//! **The gate**: every message from a client passes through here, and only here.
//!
//! For each message, in order:
//! 1. its size is checked and it is decoded (malformed messages count as strikes),
//! 2. the connection's rate limit is applied,
//! 3. it must be allowed at this point in the connection's life (for example,
//!    no moving before logging in),
//! 4. its contents are validated and cleaned (tokens, names, positions,
//!    directions, chat text),
//!
//! and only then does it become a [`SessionRequest`] (for [`crate::sessions`]),
//! a [`PlayerRequest`] (for [`crate::gameplay`]) or a [`DialogueRequest`] (for
//! [`crate::dialogue`]). Too many strikes and the connection is closed.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{
    ClientMessage, EntityId, PROTOCOL_VERSION, ServerMessage, clean_chat_text, clean_name, is_valid_token,
};
use venture_simulation::steering_direction;

use crate::ServerSystems;
use crate::link::{ConnectionId, Link, LinkEvent};
use crate::profiles::ProfileKey;
use crate::world::ServerWorld;

/// Bad messages a connection may send before it is closed.
const MAX_STRIKES: u32 = 10;
/// Messages of any kind a connection may send in a burst...
const MESSAGE_BURST: f32 = 40.0;
/// ...and per second after that.
const MESSAGES_PER_SECOND: f32 = 20.0;
/// Chat messages a player may send in a burst...
const CHAT_BURST: f32 = 5.0;
/// ...and per second after that.
const CHATS_PER_SECOND: f32 = 0.5;

/// Reads and validates client messages.
pub struct RequestsPlugin;

impl Plugin for RequestsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Connections>()
            .add_message::<SessionRequest>()
            .add_message::<PlayerRequest>()
            .add_message::<DialogueRequest>()
            .add_systems(FixedUpdate, receive_messages.in_set(ServerSystems::Requests));
    }
}

/// Every open connection and where it is in its life.
#[derive(Resource, Default)]
pub struct Connections(pub(crate) HashMap<ConnectionId, Connection>);

/// One open connection.
pub(crate) struct Connection {
    /// Where the connection is in its life. Changed by [`crate::sessions`].
    pub stage: Stage,
    messages: RateLimit,
    chat: RateLimit,
    strikes: u32,
}

/// Where a connection is in its life.
pub(crate) enum Stage {
    /// Waiting for [`ClientMessage::Hello`].
    AwaitingHello,
    /// Waiting for the server to finish logging it in. Messages are ignored.
    Joining,
    /// A new player: waiting for [`ClientMessage::ChooseName`].
    AwaitingName { profile: ProfileKey },
    /// In the game, as this player entity.
    Playing { player: Entity },
}

/// A request to join or leave, for [`crate::sessions`].
#[derive(Message)]
pub enum SessionRequest {
    /// Log in with the profile belonging to a (validated) token.
    Hello {
        /// Who is logging in.
        connection: ConnectionId,
        /// The profile their token belongs to.
        profile: ProfileKey,
    },
    /// Create a new character.
    ChooseName {
        /// Who is creating it.
        connection: ConnectionId,
        /// The profile to save it to.
        profile: ProfileKey,
        /// The character's (cleaned) name.
        name: String,
    },
    /// The connection has closed.
    Leave {
        /// Which connection.
        connection: ConnectionId,
    },
}

/// A request from a player in the game, for [`crate::gameplay`].
#[derive(Message)]
pub struct PlayerRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: PlayerAction,
}

/// What a player asked for. Already validated.
pub enum PlayerAction {
    /// Run to this point, which is inside the world.
    MoveTo(Point),
    /// Run in this direction (a unit vector).
    Steer(Point),
    /// Stop moving (and following).
    Stop,
    /// Follow this entity (if it's a player).
    Follow(EntityId),
    /// Talk to this entity (if it's an NPC), walking over first if needed.
    Interact(EntityId),
    /// Say this (already cleaned) in chat.
    Chat(String),
    /// Go back to the starting town's square.
    ReturnToTown,
}

/// A player's answer in a conversation with an NPC, for [`crate::dialogue`].
#[derive(Message)]
pub struct DialogueRequest {
    /// The player entity.
    pub player: Entity,
    /// What they chose.
    pub choice: DialogueChoice,
}

/// What a player chose in a conversation. Not yet checked against the
/// options on offer.
pub enum DialogueChoice {
    /// The option at this position in the list.
    Option(u32),
    /// Close the conversation.
    End,
}

fn receive_messages(
    mut link: ResMut<Link>,
    mut connections: ResMut<Connections>,
    world: Res<ServerWorld>,
    time: Res<Time>,
    mut sessions: MessageWriter<SessionRequest>,
    mut players: MessageWriter<PlayerRequest>,
    mut dialogue: MessageWriter<DialogueRequest>,
) {
    let now = time.elapsed_secs();
    while let Some(event) = link.next_event() {
        match event {
            LinkEvent::Connected { id, .. } => {
                let connection = Connection {
                    stage: Stage::AwaitingHello,
                    messages: RateLimit::new(MESSAGE_BURST, MESSAGES_PER_SECOND, now),
                    chat: RateLimit::new(CHAT_BURST, CHATS_PER_SECOND, now),
                    strikes: 0,
                };
                connections.0.insert(id, connection);
            }
            LinkEvent::Disconnected { id } => {
                sessions.write(SessionRequest::Leave { connection: id });
            }
            LinkEvent::Received { id, bytes } => {
                let Some(connection) = connections.0.get_mut(&id) else { continue };
                let verdict = judge(id, connection, &bytes, &world, now);
                match verdict {
                    Verdict::Session(request) => {
                        sessions.write(request);
                    }
                    Verdict::Player(request) => {
                        players.write(request);
                    }
                    Verdict::Dialogue(request) => {
                        dialogue.write(request);
                    }
                    Verdict::Reply(message) => link.send(id, &message),
                    Verdict::Ignore => {}
                    Verdict::Strike => {
                        connection.strikes += 1;
                        if connection.strikes >= MAX_STRIKES {
                            warn!("Closing connection {id:?}: too many bad messages.");
                            link.close(id);
                        }
                    }
                    Verdict::Reject(reason) => {
                        link.send(id, &ServerMessage::Rejected { reason });
                        link.close(id);
                    }
                }
            }
        }
    }
}

/// What to do with one message.
enum Verdict {
    Session(SessionRequest),
    Player(PlayerRequest),
    Dialogue(DialogueRequest),
    /// Answer the client directly, without changing anything.
    Reply(ServerMessage),
    Ignore,
    /// A bad message.
    Strike,
    /// Refuse the connection.
    Reject(String),
}

/// Decides what to do with one message from a connection.
fn judge(id: ConnectionId, connection: &mut Connection, bytes: &[u8], world: &ServerWorld, now: f32) -> Verdict {
    let Ok(message) = ClientMessage::decode(bytes) else { return Verdict::Strike };
    if !connection.messages.allow(now) {
        return Verdict::Ignore;
    }

    match (&connection.stage, message) {
        (Stage::AwaitingHello, ClientMessage::Hello { protocol_version, token }) => {
            if protocol_version != PROTOCOL_VERSION {
                return Verdict::Reject(format!(
                    "This game client is out of date (protocol {protocol_version}, server {PROTOCOL_VERSION}). Please reload the page."
                ));
            }
            if !is_valid_token(&token) {
                return Verdict::Reject("Invalid login token.".to_string());
            }
            connection.stage = Stage::Joining;
            Verdict::Session(SessionRequest::Hello { connection: id, profile: ProfileKey::from_token(&token) })
        }
        (Stage::AwaitingName { profile }, ClientMessage::ChooseName { name }) => match clean_name(&name) {
            Ok(name) => {
                let request = SessionRequest::ChooseName { connection: id, profile: profile.clone(), name };
                connection.stage = Stage::Joining;
                Verdict::Session(request)
            }
            Err(reason) => Verdict::Reply(ServerMessage::NameRejected { reason }),
        },
        (Stage::Joining, _) => Verdict::Ignore,
        (&Stage::Playing { player }, message) => judge_player_message(player, connection, message, world, now),
        _ => Verdict::Strike,
    }
}

/// Validates a message from a player in the game.
fn judge_player_message(
    player: Entity,
    connection: &mut Connection,
    message: ClientMessage,
    world: &ServerWorld,
    now: f32,
) -> Verdict {
    let action = match message {
        ClientMessage::MoveTo { x, y } => {
            if !world.contains((x, y)) {
                return Verdict::Strike;
            }
            PlayerAction::MoveTo((x, y))
        }
        ClientMessage::Steer { x, y } => match steering_direction((x, y)) {
            Some(direction) => PlayerAction::Steer(direction),
            None => PlayerAction::Stop,
        },
        ClientMessage::Stop => PlayerAction::Stop,
        ClientMessage::Follow { target } => PlayerAction::Follow(target),
        ClientMessage::Interact { target } => PlayerAction::Interact(target),
        ClientMessage::Chat { text } => {
            if !connection.chat.allow(now) {
                return Verdict::Reply(ServerMessage::Notice {
                    text: "You're sending messages too quickly.".to_string(),
                });
            }
            let Some(text) = clean_chat_text(&text) else { return Verdict::Ignore };
            PlayerAction::Chat(text)
        }
        ClientMessage::ReturnToTown => PlayerAction::ReturnToTown,
        ClientMessage::ChooseDialogueOption { index } => {
            return Verdict::Dialogue(DialogueRequest { player, choice: DialogueChoice::Option(index) });
        }
        ClientMessage::EndDialogue => {
            return Verdict::Dialogue(DialogueRequest { player, choice: DialogueChoice::End });
        }
        ClientMessage::Hello { .. } | ClientMessage::ChooseName { .. } => return Verdict::Strike,
    };
    Verdict::Player(PlayerRequest { player, action })
}

/// A token bucket: allows bursts of up to `capacity` events, refilling at
/// `per_second`.
struct RateLimit {
    tokens: f32,
    capacity: f32,
    per_second: f32,
    last: f32,
}

impl RateLimit {
    fn new(capacity: f32, per_second: f32, now: f32) -> Self {
        Self { tokens: capacity, capacity, per_second, last: now }
    }

    /// Whether one more event is allowed now (using it up if so).
    fn allow(&mut self, now: f32) -> bool {
        self.tokens = (self.tokens + (now - self.last) * self.per_second).min(self.capacity);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_allow_bursts_then_refill() {
        let mut limit = RateLimit::new(2.0, 1.0, 0.0);
        assert!(limit.allow(0.0));
        assert!(limit.allow(0.0));
        assert!(!limit.allow(0.0));
        assert!(limit.allow(1.0));
    }
}
