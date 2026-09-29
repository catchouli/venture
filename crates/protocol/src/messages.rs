//! The message types.

use serde::{Deserialize, Serialize};
use venture_rpg::{Dice, Stat, Stats};
use venture_simulation::Rules;

/// Identifies something in the game world (a player or an NPC) for as long
/// as it's there. Ids aren't reused while the server runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u32);

impl EntityId {
    /// Not an entity of the server's: things of the world itself (a tree)
    /// that clients select. The server's ids start at 1.
    pub const NONE: EntityId = EntityId(0);
}

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
    /// A monster that can be fought.
    Monster {
        /// Its level.
        level: u32,
        /// How it looks.
        look: MonsterLook,
    },
    /// A fishing trap set in the water.
    Trap {
        /// Whose it is.
        owner: EntityId,
    },
    /// A felled tree, lying where it fell (its entity's position is where it stood).
    FallenTree {
        /// The tree it was.
        tree: TreeId,
        /// Which way it fell, in radians (around the vertical, as props turn).
        toward: f32,
        /// The woodworking level it takes to cut it up.
        level: u32,
    },
}

/// The kind of body a monster has (placeholder models, until real ones).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum MonsterShape {
    /// A rabbit: small, with long ears.
    Rabbit,
    /// A goblin: a short, green-skinned person with big ears and a club.
    Goblin,
}

/// What a field names (for the editor, which offers the names that exist,
/// and follows renames). Marked on fields with `#[reflect(@Refers::Item)]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Refers {
    /// An appearance's id.
    Appearance,
    /// A monster's id.
    Monster,
    /// An item's id.
    Item,
    /// An ability's id.
    Ability,
    /// A picture in `icons/`.
    Icon,
    /// A sound in `sounds/`.
    Sound,
    /// A script in `scripts/`.
    Script,
    /// An outfit part from `characters/characters.ron`.
    Part,
    /// A hairstyle (not a beard) from `characters/characters.ron`.
    Hairstyle,
    /// A beard from `characters/characters.ron`.
    Beard,
    /// A bone of the characters' skeleton (see `venture_content::characters::BONES`).
    Bone,
    /// A monster model from `characters/characters.ron`.
    MonsterModel,
}

/// How a monster looks: its shape and colours (sRGB, each 0 to 1).
///
/// ```ron
/// (shape: Rabbit, main: (0.6, 0.5, 0.4), trim: (0.9, 0.88, 0.85))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct MonsterLook {
    /// Its shape.
    pub shape: MonsterShape,
    /// Its main colour (fur, or skin).
    pub main: (f32, f32, f32),
    /// Its second colour (a rabbit's tail and belly, a goblin's clothes).
    pub trim: (f32, f32, f32),
    /// Its model, from the content's `characters/characters.ron` (drawn
    /// instead of the shape, once the models have loaded).
    #[serde(default)]
    #[cfg_attr(feature = "reflect", reflect(@Refers::MonsterModel))]
    pub model: Option<String>,
}

/// How an attack went, in a [`ServerMessage::Attacked`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackResult {
    /// It missed.
    Miss,
    /// It hit, for this much damage.
    Hit(u32),
    /// A critical hit, for this much damage.
    Critical(u32),
}

/// How many slots the hotbar has (keys 1 to 9, then 0).
pub const HOTBAR_SLOTS: usize = 10;

/// What's in a hotbar slot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotbarEntry {
    /// An ability (its id). (Called a skill in older saved hotbars.)
    #[serde(alias = "Skill")]
    Ability(String),
    /// An action.
    Action(HotbarAction),
    /// A kind of item (its id), such as bread: using it uses the last one
    /// in the bag.
    Item(String),
}

/// An action that can go on the hotbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotbarAction {
    /// Attack the selected monster.
    Attack,
    /// Follow the selected player.
    Follow,
    /// Talk to the selected NPC.
    Talk,
    /// Wave.
    Wave,
    /// Go back to town.
    Town,
    /// Select the nearest monster within reach (see `/target`).
    Target,
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
        /// The chosen looks.
        features: Features,
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
    /// Where the player's character has got to. The client moves its own
    /// character (as it's told to: [`Self::MoveTo`], [`Self::Steer`], following
    /// and so on) and says where it is about twenty times a second while it
    /// moves. The server checks each move (speed, walls, height) and puts the
    /// character back ([`ServerMessage::Placed`]) if it isn't allowed.
    Moved {
        /// Where, in metres.
        x: f32,
        /// Where, in metres.
        y: f32,
        /// The height of its feet, in metres.
        height: f32,
        /// The placement it's from (the last [`ServerMessage::Placed`]'s):
        /// moves from before the server last placed the character are ignored.
        placement: u32,
    },
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
    /// Jump (the client jumps its character at once; the server tells
    /// everyone nearby).
    Jump,
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
    /// Attack a monster, running into reach first and then attacking until
    /// it dies or the player does something else.
    Attack {
        /// What to attack.
        target: EntityId,
    },
    /// An admin command (refused unless the player is an admin).
    Admin(AdminCommand),
    /// Do an emote (such as waving), for everyone nearby to see.
    Emote(Emote),
    /// Spend a stat point on a stat.
    SpendStatPoint {
        /// Which stat.
        stat: Stat,
    },
    /// Learn one of the class's abilities with ability points.
    LearnAbility {
        /// Which ability (its id).
        ability: String,
    },
    /// Equip the item in an inventory slot (swapping out whatever it replaces).
    Equip {
        /// The inventory slot, from 0.
        slot: u32,
    },
    /// Use the item in an inventory slot (eat it, drink it), which uses one up.
    UseItem {
        /// The inventory slot, from 0.
        slot: u32,
    },
    /// Take off what's in an equipment slot, putting it in the bag.
    Unequip {
        /// Which slot.
        slot: EquipSlot,
    },
    /// Use an ability, on a target if it needs one (running into range first).
    UseAbility {
        /// Which ability (its id).
        ability: String,
        /// What to use it on, if anything.
        target: Option<EntityId>,
    },
    /// Rearrange the hotbar (at most [`HOTBAR_SLOTS`] slots), saved with the character.
    SetHotbar {
        /// What's in each slot, from key 1.
        slots: Vec<Option<HotbarEntry>>,
    },
    /// Something to do with parties.
    Party(PartyCommand),
    /// Something to do with fishing.
    Fishing(FishingCommand),
    /// Something to do with felling trees and cutting them up.
    Woodcutting(WoodcuttingCommand),
}

/// A tree in the world: the chunk it's in, and its place among the chunk's
/// props (the same on every machine for a bake).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TreeId {
    /// The chunk's x.
    pub chunk_x: i32,
    /// The chunk's y.
    pub chunk_y: i32,
    /// Its place among the chunk's props.
    pub index: u16,
}

/// Something to do with woodcutting (part of the Woodworking skill; see
/// `docs/plan-trees.md`), in a [`ClientMessage::Woodcutting`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum WoodcuttingCommand {
    /// Start chopping at a standing tree (the player must be beside it,
    /// with an axe in hand), to fell it.
    Fell {
        /// Which tree.
        tree: TreeId,
    },
    /// Start chopping a fallen tree up for logs.
    Cut {
        /// The fallen tree.
        fallen: EntityId,
    },
    /// Stop chopping.
    Stop,
}

/// Something to do with fishing, in a [`ClientMessage::Fishing`] (see
/// `docs/plan-fishing.md`). The server answers with [`ServerMessage::Fishing`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FishingCommand {
    /// Cast the rod in hand at a point in the water, with bait or a lure
    /// from the bag.
    Cast {
        /// Where, in metres.
        x: f32,
        /// Where, in metres.
        y: f32,
        /// The bait or lure (an item id).
        tackle: String,
    },
    /// Strike: when a fish bites (with a ledger or float) it must be struck
    /// in time to hook it.
    Strike,
    /// Start or stop reeling in a fight, from a tick of the fight (the
    /// client runs the fight ahead: see `venture_simulation::fishing::FightTimeline`).
    Reel {
        /// Whether reeling.
        reeling: bool,
        /// The fight's tick it's from (the next one the client stepped).
        tick: u32,
    },
    /// Reel in and stop fishing.
    Stop,
    /// Set a trap from the bag in the water, with bait from the bag if any.
    SetTrap {
        /// Where, in metres.
        x: f32,
        /// Where, in metres.
        y: f32,
        /// The trap (an item id).
        trap: String,
        /// The bait (an item id), if any.
        bait: Option<String>,
    },
    /// Empty your trap and take it back (if it's caught anything).
    CollectTrap,
}

/// How a player's fishing is going, in a [`ServerMessage::Fishing`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FishingState {
    /// The line is in the water, waiting for a bite.
    Waiting {
        /// Where the bait or lure is, in metres.
        at: (f32, f32),
        /// How they're fishing.
        method: venture_rpg::fishing::Method,
    },
    /// Something nibbles (don't strike yet).
    Nibble,
    /// A fish bites: strike now.
    Bite,
    /// A fish is hooked and fighting (sent every tick while it lasts).
    Fight(venture_simulation::fishing::Fight),
    /// A fish is landed (an item id).
    Caught {
        /// What was caught.
        item: String,
    },
    /// Fishing has ended (reeled in, or the fish got away: a
    /// [`ServerMessage::Notice`] says why).
    Ended,
}

/// Something to do with parties (groups of players), in a
/// [`ClientMessage::Party`]. The server answers in chat either way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PartyCommand {
    /// Start a party with this name, with yourself in it (`/party NAME`).
    Create {
        /// The party's name.
        name: String,
    },
    /// Invite a player to your party (`/invite`, with them selected).
    Invite {
        /// Who.
        target: EntityId,
    },
    /// Invite a player to your party by name (`/invite NAME`).
    InviteNamed {
        /// Their name.
        name: String,
    },
    /// Join the party you've been invited to (`/accept`).
    Accept,
    /// Turn down the party invitation (`/decline`).
    Decline,
    /// Leave your party (`/leave`).
    Leave,
}

/// The party a player is in, in a [`ServerMessage::Party`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartyInfo {
    /// Its name.
    pub name: String,
    /// Everyone in it (the player too), in the order they joined.
    pub members: Vec<PartyMember>,
}

/// Someone in a party, in a [`PartyInfo`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartyMember {
    /// Their entity.
    pub id: EntityId,
    /// Their name.
    pub name: String,
    /// Their class's name.
    pub class: String,
    /// Their level.
    pub level: u32,
    /// Health left.
    pub health: u32,
    /// Most health.
    pub max_health: u32,
    /// Mana left.
    pub mana: u32,
    /// Most mana.
    pub max_mana: u32,
}

/// An invitation to join a party, in a [`ServerMessage::PartyInvite`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartyInvite {
    /// Who sent it.
    pub from: String,
    /// The party's name.
    pub party: String,
    /// Seconds left to answer it.
    pub seconds: f32,
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
    /// Set the time of day for everyone (`/time 19.5`), in hours.
    SetTime {
        /// The time, from 0 to 24.
        hour: f32,
    },
    /// Set how fast the days go for everyone (`/timescale 10`): 1 is the
    /// usual pace, 0 stops the clock.
    SetTimescale {
        /// How many times the usual pace.
        scale: f32,
    },
    /// Put items in your bag (`/give worm 20`), for trying things out.
    Give {
        /// The item (an id, or its name).
        item: String,
        /// How many.
        count: u32,
    },
    /// Set your level in a skill (`/setskill fishing 20`).
    SetSkill {
        /// The skill (an id, or its name).
        skill: String,
        /// The level.
        level: u32,
    },
    /// Move a player somewhere (`/tp NAME`, `/tp NAME OTHER`, `/tp X Y`,
    /// `/tp NAME X Y`).
    Teleport {
        /// Who to move (a player's name), or `None` for yourself.
        who: Option<String>,
        /// Where to.
        to: TeleportTo,
    },
    /// Set how fast you run (`/setspeed 5`): 1 is the usual pace.
    SetSpeed {
        /// How many times the usual pace.
        scale: f32,
    },
}

/// Where `/tp` moves a player to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TeleportTo {
    /// To another player (by name).
    Player(String),
    /// To a place in the world, in metres.
    Place {
        /// The first coordinate (as the map shows it).
        x: f32,
        /// The second coordinate.
        y: f32,
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
    /// How fast the player's character runs now, in metres per second (sent
    /// when an admin changes it, and on joining if it isn't the usual).
    RunSpeed {
        /// Metres per second.
        speed: f32,
    },
    /// Someone attacked something (sent to everyone nearby).
    Attacked {
        /// Who attacked.
        attacker: EntityId,
        /// What they attacked.
        target: EntityId,
        /// How it went.
        result: AttackResult,
        /// The ability it was done with, if any (for its look and sound).
        ability: Option<String>,
    },
    /// Someone used an ability (sent to everyone nearby, and them): the client
    /// plays it (a swing, a bolt flying off, a glow, sitting down). Any damage
    /// follows as [`ServerMessage::Attacked`], when it lands.
    AbilityUsed {
        /// Who used it.
        user: EntityId,
        /// Which ability (its id).
        ability: String,
        /// What they used it on, if anything.
        target: Option<EntityId>,
    },
    /// Something was healed (sent to everyone nearby).
    Healed {
        /// What was healed.
        id: EntityId,
        /// By how much.
        amount: u32,
    },
    /// The player's hotbar, sent when they join.
    Hotbar {
        /// What's in each slot, from key 1.
        slots: Vec<Option<HotbarEntry>>,
    },
    /// Something died (sent to everyone nearby). A monster is removed a
    /// moment later; a player wakes up at a respawn point.
    Died {
        /// What died.
        id: EntityId,
    },
    /// The player's own health, sent whenever it changes.
    Vitals {
        /// Health left.
        health: u32,
        /// Most health.
        max_health: u32,
        /// Mana left.
        mana: u32,
        /// Most mana.
        max_mana: u32,
    },
    /// The server has put the player's character here (joining, a
    /// teleport, respawning, or a move it didn't allow): the client moves it
    /// there, and says where it goes from there with this placement.
    Placed {
        /// Where, in metres.
        x: f32,
        /// Where, in metres.
        y: f32,
        /// The height of its feet, in metres.
        height: f32,
        /// Which placement this is (counting up).
        placement: u32,
    },
    /// Someone nearby jumped (sent to everyone nearby but them).
    Jumped {
        /// Who.
        id: EntityId,
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
    /// The player's party, sent when they join or leave one and whenever
    /// anything shown about it changes (such as a member's health). `None`:
    /// they aren't in one.
    Party(Option<PartyInfo>),
    /// The time of day, in hours (0 to 24): sent when the player joins, and
    /// when an admin sets it (or how fast it goes). The client runs the
    /// clock on from there, at the pace of the day cycle in the rules times
    /// `scale`.
    Time {
        /// The time.
        hour: f32,
        /// How many times the usual pace the clock runs (0: stopped).
        scale: f32,
    },
    /// The player has been invited to a party (`Some`), or the invitation
    /// has been answered or has run out (`None`).
    PartyInvite(Option<PartyInvite>),
    /// How the player's fishing is going.
    Fishing(FishingState),
    /// Another player has cast a line (to this point), or stopped fishing (`None`).
    Angler {
        /// Who.
        id: EntityId,
        /// Where their line is.
        at: Option<(f32, f32)>,
    },
    /// Every tree that's been felled (and not grown back yet), sent on joining.
    TreesFelled {
        /// The trees.
        trees: Vec<TreeId>,
    },
    /// A tree has been felled (its fallen trunk comes as an entity).
    TreeFelled {
        /// Which.
        tree: TreeId,
    },
    /// A felled tree has grown back.
    TreeGrewBack {
        /// Which.
        tree: TreeId,
    },
    /// The world has been baked again (and the server has loaded it): the
    /// manifest may have changed, and these chunks have. Clients read the
    /// manifest and map again, and load these chunks again.
    WorldChanged {
        /// The chunks that changed, as (x, y).
        chunks: Vec<(i32, i32)>,
    },
    /// Someone nearby swung an axe (sent to everyone nearby, them too).
    Chopped {
        /// Who.
        id: EntityId,
        /// Whether it bit.
        bit: bool,
        /// Where they swung at (the trunk, or the nearest point of a fallen
        /// one), in metres, so they can be turned to face it.
        at: (f32, f32),
    },
    /// The player has started chopping (at this tree or fallen tree), or stopped (`None`).
    Chopping {
        /// What they're chopping.
        at: Option<ChopTarget>,
    },
}

/// What a player chops.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChopTarget {
    /// A standing tree, to fell it.
    Standing(TreeId),
    /// A fallen tree, for logs.
    Fallen(EntityId),
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
    /// How high its feet are, in metres (on the ground, or in the air when jumping or falling).
    pub height: f32,
    /// Whether it is moving (or in the air).
    pub moving: bool,
    /// How healthy it is, from 0 to 1 (monsters only; players' health is private).
    pub health: Option<f32>,
    /// Whether it's sitting down to rest.
    pub resting: bool,
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
    /// The outfit parts its starting equipment puts on (for the preview).
    pub outfit: Vec<String>,
}

/// A character's build. It only changes how they look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
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

/// How a character looks, beyond their body type and what they wear:
/// chosen in the character creator. Hairstyles and palettes come from the
/// content's `characters/characters.ron`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct Features {
    /// Height, from `-FEATURE_RANGE` (shortest) to `FEATURE_RANGE` (tallest).
    pub height: i8,
    /// Build: from slight to broad.
    pub build: i8,
    /// The size of the head.
    pub head: i8,
    /// Their hairstyle, if not their body's usual one.
    pub hair: Option<String>,
    /// Their beard, if they have one.
    pub beard: Option<String>,
    /// Their hair colour (from the content's palette, numbered from 0).
    pub hair_colour: u8,
    /// Their skin tone (from the content's palette, numbered from 0).
    pub skin: u8,
}

/// How far each of [`Features`]' sliders goes either way.
pub const FEATURE_RANGE: i8 = 10;

impl Features {
    /// These features with the sliders brought within range.
    pub fn clamped(mut self) -> Self {
        for slider in [&mut self.height, &mut self.build, &mut self.head] {
            *slider = (*slider).clamp(-FEATURE_RANGE, FEATURE_RANGE);
        }
        self
    }
}

/// A place on a character that equipment goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum EquipSlot {
    /// Their head (helmets).
    Head,
    /// The hand they fight with: their weapon. (Called the weapon slot in
    /// older profiles and content.)
    #[serde(alias = "Weapon")]
    DominantHand,
    /// Their other hand: a shield, say.
    NonDominantHand,
    /// Their chest and arms.
    Body,
    /// Their legs.
    Legs,
}

impl EquipSlot {
    /// Every slot.
    pub const ALL: [EquipSlot; 5] =
        [EquipSlot::Head, EquipSlot::DominantHand, EquipSlot::NonDominantHand, EquipSlot::Body, EquipSlot::Legs];

    /// Its name, as shown to players.
    pub fn name(self) -> &'static str {
        match self {
            EquipSlot::Head => "Head",
            EquipSlot::DominantHand => "Dominant hand",
            EquipSlot::NonDominantHand => "Non-dominant hand",
            EquipSlot::Body => "Body",
            EquipSlot::Legs => "Legs",
        }
    }
}

/// How a worn item shows on a character (placeholder shapes, until real models).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
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
    /// A crude club, held in the right hand.
    Club,
    /// A small round shield, strapped to the left forearm.
    Buckler,
    /// A fishing rod, held out over the water in the right hand.
    Rod,
    /// An axe, held in the right hand.
    Axe,
}

/// How a worn item looks: its shape and colours (sRGB, each 0 to 1).
///
/// ```ron
/// (style: Plates, main: (0.62, 0.46, 0.28), trim: (0.3, 0.2, 0.12))
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
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
    /// Their looks.
    pub features: Features,
    /// How each item they wear looks (as simple shapes).
    pub worn: Vec<ItemLook>,
    /// The outfit parts what they wear puts on them (names from the
    /// content's `characters/characters.ron`).
    pub outfit: Vec<String>,
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
    /// Their stats.
    pub stats: Stats,
    /// Stat points to spend.
    pub stat_points: u32,
    /// Ability points to spend.
    pub ability_points: u32,
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
    /// The abilities they know (ability ids from the content pack), in the order learned.
    pub abilities: Vec<String>,
    /// Their class's abilities, with the level each unlocks at and its cost.
    pub class_abilities: Vec<AbilityOffer>,
    /// What they have equipped: the item (an id) in each filled slot. An item
    /// filling several slots is listed for each.
    pub equipment: Vec<(EquipSlot, String)>,
    /// Their skills (gathering and making), in the order they're shown.
    pub skills: Vec<SkillProgress>,
}

/// How far along a skill a character is, in a [`CharacterSheet`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProgress {
    /// Which skill (a skill id from the content pack).
    pub skill: String,
    /// Its level.
    pub level: u32,
    /// Experience earned towards the next level.
    pub experience: u64,
    /// Experience the next level takes in all (0 at the top level).
    pub experience_to_next: u64,
}

/// An ability a class can learn, in a [`CharacterSheet`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityOffer {
    /// The ability (an id from the content pack's abilities).
    pub ability: String,
    /// The level it can be learned from.
    pub level: u32,
    /// What it costs to learn, in ability points.
    pub cost: u32,
}
