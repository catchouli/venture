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
//! a [`PlayerRequest`] (for [`crate::gameplay`]), a [`DialogueRequest`] (for
//! [`crate::dialogue`]), a [`ShopRequest`] (for [`crate::shops`]), a
//! [`CharacterRequest`] (for [`crate::character`]), an [`EquipmentRequest`]
//! (for [`crate::equipment`]), a [`SkillRequest`] (for [`crate::skills`]), a
//! [`HotbarRequest`] (for [`crate::hotbar`]) or an [`AdminRequest`] (for
//! [`crate::admin`]). Too many strikes and the connection is closed.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{
    AdminCommand, ClientMessage, EntityId, HOTBAR_SLOTS, HotbarEntry, PROTOCOL_VERSION, ServerMessage, clean_chat_text,
    clean_name, is_valid_token,
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
/// The longest a content id (a class or skill) may be, in bytes.
const MAX_ID_BYTES: usize = 64;
/// The most items one purchase or sale may involve.
const MAX_TRADE_COUNT: u32 = 100;
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
            .add_message::<ShopRequest>()
            .add_message::<AdminRequest>()
            .add_message::<CharacterRequest>()
            .add_message::<EquipmentRequest>()
            .add_message::<SkillRequest>()
            .add_message::<HotbarRequest>()
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
    /// A new player: waiting for [`ClientMessage::CreateCharacter`].
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
    CreateCharacter {
        /// Who is creating it.
        connection: ConnectionId,
        /// The profile to save it to.
        profile: ProfileKey,
        /// The character's (cleaned) name. Not yet checked for being taken.
        name: String,
        /// The chosen class's id. Not yet checked against the content.
        class: String,
        /// The chosen body type.
        body: venture_protocol::BodyType,
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
    /// Attack this entity (if it's a creature), running into reach first.
    Attack(EntityId),
    /// Do an emote.
    Emote(venture_protocol::Emote),
}

/// A player's answer in a conversation with an NPC, for [`crate::dialogue`].
#[derive(Message)]
pub struct DialogueRequest {
    /// The player entity.
    pub player: Entity,
    /// What they chose.
    pub choice: DialogueChoice,
}

/// A player spending points, for [`crate::character`].
#[derive(Message)]
pub struct CharacterRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: CharacterAction,
}

/// How a player wants to spend points. Checked against their character.
pub enum CharacterAction {
    /// A stat point on an ability.
    SpendStatPoint(venture_rpg::Ability),
    /// Skill points on a skill (its id, at most [`MAX_ID_BYTES`] long).
    LearnSkill(String),
}

/// A player changing what they wear, for [`crate::equipment`].
#[derive(Message)]
pub struct EquipmentRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: EquipmentAction,
}

/// What a player wants to put on or take off. Checked against their bag and equipment.
pub enum EquipmentAction {
    /// Equip the item in this inventory slot.
    Equip(u32),
    /// Take off what's in this slot.
    Unequip(venture_protocol::EquipSlot),
}

/// A player using a skill, for [`crate::skills`].
#[derive(Message)]
pub struct SkillRequest {
    /// The player entity.
    pub player: Entity,
    /// Which skill (an id at most [`MAX_ID_BYTES`] long). Not yet checked
    /// against the content or what they know.
    pub skill: String,
    /// What to use it on, if anything.
    pub target: Option<EntityId>,
}

/// A player rearranging their hotbar, for [`crate::hotbar`].
#[derive(Message)]
pub struct HotbarRequest {
    /// The player entity.
    pub player: Entity,
    /// What's in each slot: at most [`HOTBAR_SLOTS`], with skill ids at most
    /// [`MAX_ID_BYTES`] long. Not yet checked against what they know.
    pub slots: Vec<Option<HotbarEntry>>,
}

/// An admin command, for [`crate::admin`] (which checks the player is an admin).
#[derive(Message)]
pub struct AdminRequest {
    /// The player entity.
    pub player: Entity,
    /// The command.
    pub command: AdminCommand,
}

/// A player's request to the shop they have open, for [`crate::shops`].
#[derive(Message)]
pub struct ShopRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: ShopAction,
}

/// What a player asked of a shop. The counts are checked (from 1 to
/// [`MAX_TRADE_COUNT`]); the rest is checked against the shop and the
/// player's inventory.
pub enum ShopAction {
    /// Buy some of an item the shop sells.
    Buy {
        /// The item's position in the shop's stock.
        index: u32,
        /// How many.
        count: u32,
    },
    /// Sell some of an item in the player's inventory.
    Sell {
        /// The inventory slot it's in.
        slot: u32,
        /// How many.
        count: u32,
    },
    /// Close the shop.
    Close,
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
    mut shops: MessageWriter<ShopRequest>,
    mut admin: MessageWriter<AdminRequest>,
    mut characters: MessageWriter<CharacterRequest>,
    mut equipment: MessageWriter<EquipmentRequest>,
    mut skills: MessageWriter<SkillRequest>,
    mut hotbars: MessageWriter<HotbarRequest>,
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
                    Verdict::Shop(request) => {
                        shops.write(request);
                    }
                    Verdict::Admin(request) => {
                        admin.write(request);
                    }
                    Verdict::Character(request) => {
                        characters.write(request);
                    }
                    Verdict::Equipment(request) => {
                        equipment.write(request);
                    }
                    Verdict::Skill(request) => {
                        skills.write(request);
                    }
                    Verdict::Hotbar(request) => {
                        hotbars.write(request);
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
    Shop(ShopRequest),
    Admin(AdminRequest),
    Character(CharacterRequest),
    Equipment(EquipmentRequest),
    Skill(SkillRequest),
    Hotbar(HotbarRequest),
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
        (Stage::AwaitingName { .. }, ClientMessage::CreateCharacter { class, .. }) if class.len() > MAX_ID_BYTES => {
            Verdict::Strike
        }
        (Stage::AwaitingName { profile }, ClientMessage::CreateCharacter { name, class, body }) => {
            match clean_name(&name) {
                Ok(name) => {
                    let request =
                        SessionRequest::CreateCharacter { connection: id, profile: profile.clone(), name, class, body };
                    connection.stage = Stage::Joining;
                    Verdict::Session(request)
                }
                Err(reason) => Verdict::Reply(ServerMessage::CharacterRejected { reason }),
            }
        }
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
        ClientMessage::Attack { target } => PlayerAction::Attack(target),
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
        // Emotes share chat's rate limit, so they can't be spammed either.
        ClientMessage::Emote(emote) => {
            if !connection.chat.allow(now) {
                return Verdict::Ignore;
            }
            PlayerAction::Emote(emote)
        }
        ClientMessage::ChooseDialogueOption { index } => {
            return Verdict::Dialogue(DialogueRequest { player, choice: DialogueChoice::Option(index) });
        }
        ClientMessage::EndDialogue => {
            return Verdict::Dialogue(DialogueRequest { player, choice: DialogueChoice::End });
        }
        ClientMessage::Buy { count, .. } | ClientMessage::Sell { count, .. }
            if !(1..=MAX_TRADE_COUNT).contains(&count) =>
        {
            return Verdict::Strike;
        }
        ClientMessage::Buy { index, count } => {
            return Verdict::Shop(ShopRequest { player, action: ShopAction::Buy { index, count } });
        }
        ClientMessage::Sell { slot, count } => {
            return Verdict::Shop(ShopRequest { player, action: ShopAction::Sell { slot, count } });
        }
        ClientMessage::CloseShop => return Verdict::Shop(ShopRequest { player, action: ShopAction::Close }),
        ClientMessage::Admin(AdminCommand::SetClass { class }) if class.len() > MAX_ID_BYTES => {
            return Verdict::Strike;
        }
        ClientMessage::Admin(command) => return Verdict::Admin(AdminRequest { player, command }),
        ClientMessage::SpendStatPoint { ability } => {
            return Verdict::Character(CharacterRequest { player, action: CharacterAction::SpendStatPoint(ability) });
        }
        ClientMessage::LearnSkill { skill } if skill.len() > MAX_ID_BYTES => return Verdict::Strike,
        ClientMessage::LearnSkill { skill } => {
            return Verdict::Character(CharacterRequest { player, action: CharacterAction::LearnSkill(skill) });
        }
        ClientMessage::Equip { slot } => {
            return Verdict::Equipment(EquipmentRequest { player, action: EquipmentAction::Equip(slot) });
        }
        ClientMessage::Unequip { slot } => {
            return Verdict::Equipment(EquipmentRequest { player, action: EquipmentAction::Unequip(slot) });
        }
        ClientMessage::UseSkill { skill, .. } if skill.len() > MAX_ID_BYTES => return Verdict::Strike,
        ClientMessage::UseSkill { skill, target } => return Verdict::Skill(SkillRequest { player, skill, target }),
        ClientMessage::SetHotbar { slots } => {
            let too_long =
                |entry: &Option<HotbarEntry>| matches!(entry, Some(HotbarEntry::Skill(id)) if id.len() > MAX_ID_BYTES);
            if slots.len() > HOTBAR_SLOTS || slots.iter().any(too_long) {
                return Verdict::Strike;
            }
            return Verdict::Hotbar(HotbarRequest { player, slots });
        }
        ClientMessage::Hello { .. } | ClientMessage::CreateCharacter { .. } => return Verdict::Strike,
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
