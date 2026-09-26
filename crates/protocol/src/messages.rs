//! The message types.

use serde::{Deserialize, Serialize};
use venture_rpg::{Abilities, Ability, Dice};
use venture_simulation::Rules;

/// Identifies something in the game world (a player or an NPC) for as long
/// as it's there. Ids aren't reused while the server runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u32);

/// What kind of thing an entity is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    /// Another player.
    Player {
        /// Their level (their combat level).
        level: u32,
        /// How they look.
        look: PlayerLook,
    },
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
    /// Make a new character, after the server asks with [`ServerMessage::CharacterNeeded`].
    CreateCharacter {
        /// The requested name.
        name: String,
        /// The chosen class (an id from [`ServerMessage::CharacterNeeded`]'s classes).
        class: String,
        /// The chosen body type.
        body: BodyType,
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
    /// Follow another player, staying close behind them.
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
    /// Go back to the starting town's square.
    ReturnToTown,
    /// Pick one of the options in the open conversation ([`ServerMessage::Dialogue`]).
    ChooseDialogueOption {
        /// The option's position in the list, from 0.
        index: u32,
    },
    /// Close the open conversation.
    EndDialogue,
    /// Buy from the open shop ([`ServerMessage::ShopOpened`]).
    Buy {
        /// The item's position in the shop's stock, from 0.
        index: u32,
        /// How many to buy.
        count: u32,
    },
    /// Sell to the open shop.
    Sell {
        /// The inventory slot holding the item, from 0.
        slot: u32,
        /// How many to sell.
        count: u32,
    },
    /// Close the open shop.
    CloseShop,
    /// An admin command (refused unless the player is an admin).
    Admin(AdminCommand),
    /// Do an emote (such as waving), for everyone nearby to see.
    Emote(Emote),
    /// Spend a stat point on an ability.
    SpendStatPoint {
        /// Which ability.
        ability: Ability,
    },
    /// Learn one of the class's skills with skill points.
    LearnSkill {
        /// Which skill (its id).
        skill: String,
    },
    /// Equip the item in an inventory slot (swapping out whatever it replaces).
    Equip {
        /// The inventory slot, from 0.
        slot: u32,
    },
    /// Take off what's in an equipment slot, putting it in the bag.
    Unequip {
        /// Which slot.
        slot: EquipSlot,
    },
}

/// A gesture a character can make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Emote {
    /// Wave hello.
    Wave,
}

/// A command only admins may use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AdminCommand {
    /// Set your own level (`/setlevel 10`), with the points it would have given.
    SetLevel {
        /// The new level.
        level: u32,
    },
    /// Start your character over at level 1 (`/reset`).
    Reset,
    /// Change your class (`/setclass mage`), refunding your points.
    SetClass {
        /// The class's id or name.
        class: String,
    },
    /// Change your body type (`/setgender feminine`).
    SetBody {
        /// The new body type.
        body: BodyType,
    },
}

/// Events from the server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// This token has no character yet: the client should ask the player for
    /// a name and a class, and reply with [`ClientMessage::CreateCharacter`].
    CharacterNeeded {
        /// A random name to offer.
        suggestion: String,
        /// The classes to choose from (the first is the default).
        classes: Vec<ClassChoice>,
    },
    /// The new character can't be made as asked (such as a name that's
    /// taken); the client should ask again.
    CharacterRejected {
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
        /// Whether the player is an admin (so the client lists admin commands).
        admin: bool,
        /// How the player's character looks.
        look: PlayerLook,
    },
    /// Someone nearby did an emote (sent to them too).
    Emoted {
        /// Who.
        id: EntityId,
        /// Which emote.
        emote: Emote,
    },
    /// The player's own character sheet, sent whenever it changes.
    Character(CharacterSheet),
    /// A player's level or look has changed (sent to everyone, the player too).
    PlayerChanged {
        /// Whose.
        id: EntityId,
        /// Their level.
        level: u32,
        /// How they look (their class shows in their outfit).
        look: PlayerLook,
        /// Whether they went up a level (and everyone should see a level-up effect).
        celebrate: bool,
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
    /// The next part of a conversation with an NPC, for this player only. The
    /// client shows it in a dialogue window, replacing what was there.
    Dialogue {
        /// Who the player is talking to.
        npc: EntityId,
        /// What is said, in order.
        lines: Vec<DialogueLine>,
        /// The replies the player can choose from. Empty when the conversation is over.
        options: Vec<String>,
    },
    /// The conversation has ended (for example, the player walked away).
    DialogueEnded,
    /// The player's inventory, slot by slot, sent whenever it changes.
    Inventory {
        /// Each slot: empty, or holding some of one item.
        slots: Vec<Option<ItemStack>>,
        /// Their money.
        miasma: u64,
    },
    /// The player's quest journal, sent whenever it changes: every quest
    /// they've started, in the order they started them.
    Quests {
        /// Each quest.
        quests: Vec<QuestEntry>,
    },
    /// A shop has opened for this player: the client shows its window.
    ShopOpened {
        /// Who runs it.
        npc: EntityId,
        /// The shop's name.
        name: String,
        /// What it sells, in order.
        stock: Vec<ShopStock>,
    },
    /// The open shop has closed (for example, the player walked away).
    ShopClosed,
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

/// One line of a conversation in a [`ServerMessage::Dialogue`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DialogueLine {
    /// Who says it, if the line names someone.
    pub speaker: Option<String>,
    /// What they say.
    pub text: String,
}

/// Some of one item, in an inventory slot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    /// Which item (an id from the content pack's items).
    pub item: String,
    /// How many. Always at least 1.
    pub count: u32,
}

/// Something a shop sells, in a [`ServerMessage::ShopOpened`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopStock {
    /// Which item.
    pub item: String,
    /// What one costs, in coins.
    pub price: u32,
}

/// One quest in the player's journal ([`ServerMessage::Quests`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestEntry {
    /// The quest's name.
    pub name: String,
    /// What the journal says about where the player has got to.
    pub journal: String,
    /// Whether it's finished.
    pub complete: bool,
}

/// A class a new character can be, in [`ServerMessage::CharacterNeeded`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassChoice {
    /// Its id, to send back.
    pub id: String,
    /// Its name.
    pub name: String,
    /// What it's like.
    pub description: String,
    /// How its starting equipment looks (for the preview).
    pub worn: Vec<ItemLook>,
}

/// A character's build. It only changes how they look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyType {
    /// Broader shoulders, narrower hips.
    Masculine,
    /// Narrower shoulders and waist, wider hips.
    Feminine,
    /// In between.
    #[default]
    Androgynous,
}

impl BodyType {
    /// Every body type, in the order they're offered.
    pub const ALL: [BodyType; 3] = [BodyType::Masculine, BodyType::Feminine, BodyType::Androgynous];

    /// Its name, as shown to players.
    pub fn name(self) -> &'static str {
        match self {
            BodyType::Masculine => "Masculine",
            BodyType::Feminine => "Feminine",
            BodyType::Androgynous => "Androgynous",
        }
    }
}

/// A place on a character that equipment goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EquipSlot {
    /// What they fight with.
    Weapon,
    /// Their chest and arms.
    Body,
    /// Their legs.
    Legs,
}

impl EquipSlot {
    /// Every slot, in the order they're shown.
    pub const ALL: [EquipSlot; 3] = [EquipSlot::Weapon, EquipSlot::Body, EquipSlot::Legs];

    /// Its name, as shown to players.
    pub fn name(self) -> &'static str {
        match self {
            EquipSlot::Weapon => "Weapon",
            EquipSlot::Body => "Body",
            EquipSlot::Legs => "Legs",
        }
    }
}

/// How a worn item shows on a character (placeholder shapes, until real models).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LookStyle {
    /// Plates strapped over the chest and back, with shoulder guards.
    Plates,
    /// Plates strapped over the thighs and shins.
    LegPlates,
    /// A long robe, from the shoulders to the ankles.
    Robe,
    /// A sword, held in the right hand.
    Sword,
    /// A staff, held in the right hand.
    Staff,
}

/// How a worn item looks: its shape and colours (sRGB, each 0 to 1).
///
/// ```ron
/// (style: Plates, main: (0.62, 0.46, 0.28), trim: (0.3, 0.2, 0.12))
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemLook {
    /// Its shape.
    pub style: LookStyle,
    /// Its main colour.
    pub main: (f32, f32, f32),
    /// Its second colour (straps, trim, a hilt).
    pub trim: (f32, f32, f32),
}

/// How a player's character looks: their build, and what they wear over
/// their plain clothes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerLook {
    /// Their build.
    pub body: BodyType,
    /// How each item they wear looks.
    pub worn: Vec<ItemLook>,
}

/// Everything about the player's own character, for the stats tab
/// ([`ServerMessage::Character`]). The server works out the secondary stats,
/// so the client never has to repeat the rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterSheet {
    /// Their class's name.
    pub class: String,
    /// Their level.
    pub level: u32,
    /// Experience earned towards the next level.
    pub experience: u64,
    /// Experience the next level takes in all (0 at the top level).
    pub experience_to_next: u64,
    /// Their ability scores.
    pub stats: Abilities,
    /// Stat points to spend.
    pub stat_points: u32,
    /// Skill points to spend.
    pub skill_points: u32,
    /// Most health.
    pub max_health: u32,
    /// Most mana.
    pub max_mana: u32,
    /// Armour class.
    pub armour_class: i32,
    /// Added to attack rolls.
    pub attack_bonus: i32,
    /// Added to damage.
    pub damage_bonus: i32,
    /// Attacks per second.
    pub attacks_per_second: f32,
    /// Weapon damage, bonus included.
    pub damage: Dice,
    /// The skills they know (skill ids from the content pack), in the order learned.
    pub skills: Vec<String>,
    /// Their class's skills, with the level each unlocks at and its cost.
    pub class_skills: Vec<SkillOffer>,
    /// What they have equipped: the item (an id) in each filled slot. An item
    /// filling several slots is listed for each.
    pub equipment: Vec<(EquipSlot, String)>,
}

/// A skill a class can learn, in a [`CharacterSheet`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillOffer {
    /// The skill (an id from the content pack's skills).
    pub skill: String,
    /// The level it can be learned from.
    pub level: u32,
    /// What it costs to learn, in skill points.
    pub cost: u32,
}
