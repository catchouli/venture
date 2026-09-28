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
//! (for [`crate::equipment`]), an [`ItemUseRequest`] (for
//! [`crate::consumables`]), a [`AbilityRequest`] (for [`crate::abilities`]), a
//! [`HotbarRequest`] (for [`crate::hotbar`]), a [`PartyRequest`] (for
//! [`crate::parties`]), a [`FishingRequest`] (for [`crate::fishing`]) or an [`AdminRequest`] (for [`crate::admin`]). Too many strikes and the connection is closed.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{
    AdminCommand, ClientMessage, EntityId, FishingCommand, HOTBAR_SLOTS, HotbarEntry, PROTOCOL_VERSION, PartyCommand,
    ServerMessage, clean_chat_text, clean_name, clean_party_name, is_valid_token,
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
/// The longest a content id (a class or ability) may be, in bytes.
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
            .add_message::<ItemUseRequest>()
            .add_message::<AbilityRequest>()
            .add_message::<HotbarRequest>()
            .add_message::<PartyRequest>()
            .add_message::<FishingRequest>()
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
        /// The chosen looks, sliders in range. Not yet checked against the content.
        features: venture_protocol::Features,
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
    /// Attack this entity (if it's a monster), running into reach first.
    Attack(EntityId),
    /// Do an emote.
    Emote(venture_protocol::Emote),
    /// Jump.
    Jump,
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
    /// A stat point on a stat.
    SpendStatPoint(venture_rpg::Stat),
    /// Ability points on an ability (its id, at most [`MAX_ID_BYTES`] long).
    LearnAbility(String),
}

/// A player changing what they wear, for [`crate::equipment`].
#[derive(Message)]
pub struct EquipmentRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: EquipmentAction,
}

/// A player using the item in an inventory slot, for [`crate::consumables`].
#[derive(Message)]
pub struct ItemUseRequest {
    /// The player entity.
    pub player: Entity,
    /// The inventory slot (not yet checked against their bag).
    pub slot: u32,
}

/// What a player wants to put on or take off. Checked against their bag and equipment.
pub enum EquipmentAction {
    /// Equip the item in this inventory slot.
    Equip(u32),
    /// Take off what's in this slot.
    Unequip(venture_protocol::EquipSlot),
}

/// A player using an ability, for [`crate::abilities`].
#[derive(Message)]
pub struct AbilityRequest {
    /// The player entity.
    pub player: Entity,
    /// Which ability (an id at most [`MAX_ID_BYTES`] long). Not yet checked
    /// against the content or what they know.
    pub ability: String,
    /// What to use it on, if anything.
    pub target: Option<EntityId>,
}

/// A player rearranging their hotbar, for [`crate::hotbar`].
#[derive(Message)]
pub struct HotbarRequest {
    /// The player entity.
    pub player: Entity,
    /// What's in each slot: at most [`HOTBAR_SLOTS`], with ability ids at most
    /// [`MAX_ID_BYTES`] long. Not yet checked against what they know.
    pub slots: Vec<Option<HotbarEntry>>,
}

/// A player's request about parties, for [`crate::parties`].
#[derive(Message)]
pub struct PartyRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: PartyAction,
}

/// What a player asked about parties. Names are already cleaned; whether
/// the request makes sense (are they in a party?) is for
/// [`crate::parties`] to decide.
pub enum PartyAction {
    /// Start a party with this (valid) name.
    Create(String),
    /// Invite this entity (if it's a player).
    Invite(EntityId),
    /// Invite the player with this (validly shaped) name.
    InviteNamed(String),
    /// Accept the invitation they have.
    Accept,
    /// Decline the invitation they have.
    Decline,
    /// Leave their party.
    Leave,
}

/// A player's request about fishing, for [`crate::fishing`].
#[derive(Message)]
pub struct FishingRequest {
    /// The player entity.
    pub player: Entity,
    /// What they asked for.
    pub action: FishingAction,
}

/// What a player asked about fishing. The cast's point is in the world and
/// its tackle id no longer than [`MAX_ID_BYTES`]; the rest (their rod, the
/// water, their bag) is for [`crate::fishing`] to check.
pub enum FishingAction {
    /// Cast at this point with this bait or lure.
    Cast {
        /// Where.
        at: Point,
        /// The bait or lure's item id.
        tackle: String,
    },
    /// Strike.
    Strike,
    /// Reel (or stop reeling).
    Reel(bool),
    /// Stop fishing.
    Stop,
    /// Set a trap at this point.
    SetTrap {
        /// Where.
        at: Point,
        /// The trap's item id.
        trap: String,
        /// The bait's item id, if any.
        bait: Option<String>,
    },
    /// Collect their trap.
    CollectTrap,
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
    mut item_uses: MessageWriter<ItemUseRequest>,
    mut abilities: MessageWriter<AbilityRequest>,
    mut hotbars: MessageWriter<HotbarRequest>,
    mut parties: MessageWriter<PartyRequest>,
    mut fishing: MessageWriter<FishingRequest>,
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
                    Verdict::UseItem(request) => {
                        item_uses.write(request);
                    }
                    Verdict::Ability(request) => {
                        abilities.write(request);
                    }
                    Verdict::Hotbar(request) => {
                        hotbars.write(request);
                    }
                    Verdict::Party(request) => {
                        parties.write(request);
                    }
                    Verdict::Fishing(request) => {
                        fishing.write(request);
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
    UseItem(ItemUseRequest),
    Ability(AbilityRequest),
    Hotbar(HotbarRequest),
    Party(PartyRequest),
    Fishing(FishingRequest),
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
        (Stage::AwaitingName { .. }, ClientMessage::CreateCharacter { class, features, .. })
            if class.len() > MAX_ID_BYTES
                || [&features.hair, &features.beard].into_iter().flatten().any(|hair| hair.len() > MAX_ID_BYTES) =>
        {
            Verdict::Strike
        }
        (Stage::AwaitingName { profile }, ClientMessage::CreateCharacter { name, class, body, features }) => {
            match clean_name(&name) {
                Ok(name) => {
                    let features = features.clamped();
                    let request = SessionRequest::CreateCharacter {
                        connection: id,
                        profile: profile.clone(),
                        name,
                        class,
                        body,
                        features,
                    };
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
        ClientMessage::Jump => PlayerAction::Jump,
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
        ClientMessage::Admin(
            AdminCommand::SetClass { class: id }
            | AdminCommand::Give { item: id, .. }
            | AdminCommand::SetSkill { skill: id, .. },
        ) if id.len() > MAX_ID_BYTES => {
            return Verdict::Strike;
        }
        ClientMessage::Admin(command) => return Verdict::Admin(AdminRequest { player, command }),
        ClientMessage::SpendStatPoint { stat } => {
            return Verdict::Character(CharacterRequest { player, action: CharacterAction::SpendStatPoint(stat) });
        }
        ClientMessage::LearnAbility { ability } if ability.len() > MAX_ID_BYTES => return Verdict::Strike,
        ClientMessage::LearnAbility { ability } => {
            return Verdict::Character(CharacterRequest { player, action: CharacterAction::LearnAbility(ability) });
        }
        ClientMessage::Equip { slot } => {
            return Verdict::Equipment(EquipmentRequest { player, action: EquipmentAction::Equip(slot) });
        }
        ClientMessage::UseItem { slot } => return Verdict::UseItem(ItemUseRequest { player, slot }),
        ClientMessage::Unequip { slot } => {
            return Verdict::Equipment(EquipmentRequest { player, action: EquipmentAction::Unequip(slot) });
        }
        ClientMessage::UseAbility { ability, .. } if ability.len() > MAX_ID_BYTES => return Verdict::Strike,
        ClientMessage::UseAbility { ability, target } => {
            return Verdict::Ability(AbilityRequest { player, ability, target });
        }
        ClientMessage::SetHotbar { slots } => {
            let too_long = |entry: &Option<HotbarEntry>| matches!(entry, Some(HotbarEntry::Ability(id) | HotbarEntry::Item(id)) if id.len() > MAX_ID_BYTES);
            if slots.len() > HOTBAR_SLOTS || slots.iter().any(too_long) {
                return Verdict::Strike;
            }
            return Verdict::Hotbar(HotbarRequest { player, slots });
        }
        ClientMessage::Party(command) => return judge_party_command(player, connection, command, now),
        ClientMessage::Fishing(command) => {
            let action = match command {
                FishingCommand::Cast { x, y, tackle } => {
                    if !world.contains((x, y)) || tackle.len() > MAX_ID_BYTES {
                        return Verdict::Strike;
                    }
                    FishingAction::Cast { at: (x, y), tackle }
                }
                FishingCommand::SetTrap { x, y, trap, bait } => {
                    let long = |id: &str| id.len() > MAX_ID_BYTES;
                    if !world.contains((x, y)) || long(&trap) || bait.as_deref().is_some_and(long) {
                        return Verdict::Strike;
                    }
                    FishingAction::SetTrap { at: (x, y), trap, bait }
                }
                FishingCommand::CollectTrap => FishingAction::CollectTrap,
                FishingCommand::Strike => FishingAction::Strike,
                FishingCommand::Reel(reeling) => FishingAction::Reel(reeling),
                FishingCommand::Stop => FishingAction::Stop,
            };
            return Verdict::Fishing(FishingRequest { player, action });
        }
        ClientMessage::Hello { .. } | ClientMessage::CreateCharacter { .. } => return Verdict::Strike,
    };
    Verdict::Player(PlayerRequest { player, action })
}

/// Validates a party command. Creating parties and inviting share chat's
/// rate limit (so invitations can't be spammed); names are cleaned, and a
/// name that isn't allowed is explained.
fn judge_party_command(player: Entity, connection: &mut Connection, command: PartyCommand, now: f32) -> Verdict {
    let notice = |text: String| Verdict::Reply(ServerMessage::Notice { text });
    let action = match command {
        PartyCommand::Create { name } | PartyCommand::InviteNamed { name } if name.len() > MAX_ID_BYTES => {
            return Verdict::Strike;
        }
        PartyCommand::Create { .. } | PartyCommand::Invite { .. } | PartyCommand::InviteNamed { .. }
            if !connection.chat.allow(now) =>
        {
            return notice("You're doing that too quickly.".to_string());
        }
        PartyCommand::Create { name } => match clean_party_name(&name) {
            Ok(name) => PartyAction::Create(name),
            Err(reason) => return notice(reason),
        },
        PartyCommand::Invite { target } => PartyAction::Invite(target),
        PartyCommand::InviteNamed { name } => match clean_name(&name) {
            Ok(name) => PartyAction::InviteNamed(name),
            Err(_) => {
                return notice(format!("There's no player called {}.", clean_chat_text(&name).unwrap_or_default()));
            }
        },
        PartyCommand::Accept => PartyAction::Accept,
        PartyCommand::Decline => PartyAction::Decline,
        PartyCommand::Leave => PartyAction::Leave,
    };
    Verdict::Party(PartyRequest { player, action })
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
