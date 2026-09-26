//! The kinds of things a content pack defines.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use venture_protocol::{BodyType, CreatureLook, EquipSlot, FEATURE_RANGE, Features, ItemLook};
use venture_rpg::{ClassRules, Dice, Stat, Stats};

/// The cut of an NPC's clothes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum OutfitStyle {
    /// A plain tunic and trousers.
    #[default]
    Tunic,
    /// A tunic with wooden plates strapped over it.
    Armour,
    /// A long robe.
    Robe,
}

/// A colour as sRGB red, green and blue, each from 0 to 1.
pub type Colour = (f32, f32, f32);

fn default_bolt_speed() -> f32 {
    12.0
}

/// How a character looks.
///
/// ```ron
/// (id: "villager_green", skin: (0.86, 0.66, 0.53), hair: (0.3, 0.19, 0.11),
///  tunic: (0.25, 0.35, 0.22), trousers: (0.3, 0.26, 0.22))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct AppearanceDef {
    /// Unique name, used by NPCs to refer to it.
    pub id: String,
    /// Skin colour.
    pub skin: Colour,
    /// Hair colour.
    pub hair: Colour,
    /// Tunic colour.
    pub tunic: Colour,
    /// Trousers colour.
    pub trousers: Colour,
    /// Their build (androgynous if not given).
    #[serde(default)]
    pub body: BodyType,
    /// The cut of their clothes (a tunic if not given).
    #[serde(default)]
    pub outfit: OutfitStyle,
    /// Their hairstyle (from `characters/characters.ron`), if not their body's usual one.
    #[serde(default)]
    pub hairstyle: Option<String>,
    /// The outfit parts they wear (from `characters/characters.ron`), if not
    /// the ones for their outfit style.
    #[serde(default)]
    pub parts: Vec<String>,
    /// Their beard (from `characters/characters.ron`), if they have one.
    #[serde(default)]
    pub beard: Option<String>,
    /// Their height, build and head size (from -10 to 10, as in the character
    /// creator). Any left out are picked from the appearance's id, so each
    /// person looks a little different, but always the same.
    #[serde(default)]
    pub height: Option<i8>,
    /// Their build (see `height`).
    #[serde(default)]
    pub build: Option<i8>,
    /// Their head size (see `height`).
    #[serde(default)]
    pub head: Option<i8>,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// How far picked sliders go either way (less than players can choose, so
/// NPCs look ordinary unless a designer says otherwise).
pub const PICKED_RANGE: i8 = 6;

impl AppearanceDef {
    /// Their features: the ones given, and the rest picked from the id.
    /// (Colours are the appearance's own, so the palette choices are left
    /// at the first.)
    pub fn features(&self) -> Features {
        let pick = |given: Option<i8>, which: u8| {
            given.unwrap_or_else(|| {
                let span = (2 * PICKED_RANGE + 1) as u64;
                (seeded(&self.id, which) % span) as i8 - PICKED_RANGE
            })
        };
        Features {
            height: pick(self.height, 0),
            build: pick(self.build, 1),
            head: pick(self.head, 2),
            hair: self.hairstyle.clone(),
            beard: self.beard.clone(),
            hair_colour: 0,
            skin: 0,
        }
        .clamped()
    }
}

/// A number picked from a name (the same on every computer): FNV-1a.
fn seeded(name: &str, which: u8) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes().chain([which]) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

const _: () = assert!(PICKED_RANGE <= FEATURE_RANGE);

/// A non-player character.
///
/// ```ron
/// (id: "millbrook_guard", name: "Guard Aldren", appearance: "guard",
///  at: StartingSettlement(offset: (6.0, 3.0)), script: Some("guard.rhai"), dialogue: Some("Guard"))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct NpcDef {
    /// Unique name, used to recognise the NPC across content reloads.
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// Which appearance to use (an [`AppearanceDef`] id).
    pub appearance: String,
    /// Where the NPC lives.
    pub at: Placement,
    /// The script that gives the NPC its behaviour (a file in `scripts/`), if any.
    #[serde(default)]
    pub script: Option<String>,
    /// The conversation that opens when a player talks to the NPC: the title
    /// of a node in the `dialogue/` files, if any.
    #[serde(default)]
    pub dialogue: Option<String>,
    /// How far from home (in metres) the NPC may wander. Zero keeps it in place.
    #[serde(default)]
    pub wander_radius: f32,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// Where something is placed in the world. Placing things relative to
/// settlements keeps content in the right spot even when the terrain
/// generator is tuned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Placement {
    /// Relative to the settlement new characters start in, in metres (east, south).
    StartingSettlement {
        /// Offset from the settlement's centre.
        offset: (f32, f32),
    },
    /// Relative to a settlement, by its number in the baked world (0 is the best site).
    Settlement {
        /// The settlement's number.
        index: usize,
        /// Offset from the settlement's centre.
        offset: (f32, f32),
    },
    /// At a position in the world, in metres.
    Position((f32, f32)),
}

/// Something a player can carry.
///
/// ```ron
/// (id: "bread", name: "Bread", description: "A crusty loaf, still warm.",
///  icon: "bread.png", value: 4)
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    /// Unique name, used everywhere else to refer to it (and saved in profiles).
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// A line or two about it, shown when the player points at it.
    pub description: String,
    /// Its picture: a PNG file in `icons/`.
    pub icon: String,
    /// Whether any number of them share one inventory slot (like arrows).
    #[serde(default)]
    pub stackable: bool,
    /// What it's worth, in Miasma. Shops buy it for half that; zero means
    /// shops won't buy it at all (quest items, for example).
    #[serde(default)]
    pub value: u32,
    /// If it can be worn or wielded: where, what it does, and how it looks.
    #[serde(default)]
    pub equip: Option<Equippable>,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// What an equippable item does when worn.
///
/// ```ron
/// equip: Some((slots: [Weapon], weapon: Some((damage: "1d6", speed: 1.0, stat: Strength)),
///              look: (style: Sword, main: (0.7, 0.7, 0.72), trim: (0.35, 0.22, 0.12))))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct Equippable {
    /// The slots it fills (a robe fills both `Body` and `Legs`).
    pub slots: Vec<EquipSlot>,
    /// Armour class it adds.
    #[serde(default)]
    pub armour: i32,
    /// What it does as a weapon (for items in the `Weapon` slot).
    #[serde(default)]
    pub weapon: Option<WeaponDef>,
    /// How it shows on the character (the simple shapes drawn when the
    /// character models aren't available).
    pub look: ItemLook,
    /// The outfit parts it puts on the character (from
    /// `characters/characters.ron`): clothes, armour, or a weapon in hand.
    #[serde(default)]
    pub parts: Vec<String>,
}

/// A weapon's part in the rules.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
    /// Its damage dice, before the stat's bonus (such as `"1d6"`).
    pub damage: Dice,
    /// Attacks per second, before Dexterity.
    pub speed: f32,
    /// The stat it attacks and does damage with.
    pub stat: Stat,
}

/// An ability: something a character can do, learned from their class.
///
/// ```ron
/// (id: "whack", name: "Whack", description: "A heavy blow.", icon: "whack.png",
///  mana: 3, cooldown: 6.0, range: 2.5, effect: Strike(damage: "1d8"))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct AbilityDef {
    /// Unique name (saved in profiles, so don't rename lightly).
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// What it does, in a sentence or two.
    pub description: String,
    /// Its picture: a PNG file in `icons/`.
    pub icon: String,
    /// Mana it costs to use.
    #[serde(default)]
    pub mana: u32,
    /// Seconds before it can be used again.
    #[serde(default)]
    pub cooldown: f32,
    /// How far away the target may be, in metres (0 for abilities on yourself).
    #[serde(default)]
    pub range: f32,
    /// What it does.
    pub effect: AbilityEffect,
    /// The sound it makes when used: a file in `sounds/`, if any.
    #[serde(default)]
    pub sound: Option<String>,
    /// How it looks: the colour and size of its flash, bolt or glow.
    #[serde(default)]
    pub visual: AbilityVisual,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl AbilityDef {
    /// How far away its target may be, in metres: melee reach for weapon
    /// blows (whatever `range` says), `range` for everything else.
    pub fn reach(&self) -> f32 {
        match self.effect {
            AbilityEffect::Strike { .. } => venture_simulation::MELEE_RANGE,
            _ => self.range,
        }
    }

    /// Whether it's used on a target (rather than on yourself).
    pub fn needs_target(&self) -> bool {
        matches!(self.effect, AbilityEffect::Strike { .. } | AbilityEffect::Bolt { .. })
    }
}

/// A skill: something characters get better at by doing it (gathering and
/// making things), with its own level and experience.
///
/// ```ron
/// (id: "fishing", name: "Fishing", description: "Catching fish.", icon: "skill_fishing.png")
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct SkillDef {
    /// Unique name (saved in profiles, so don't rename lightly).
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// What it's about, in a sentence or two.
    pub description: String,
    /// Its picture: a PNG file in `icons/`.
    pub icon: String,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// The interface's look: the image for each piece of it (window frames,
/// buttons, slots, bars and so on) and the toolbar's icons. Read from
/// `ui/theme.ron`; the images are PNGs in `ui/`. Every piece is optional: a
/// piece without an image keeps the plain flat look. See
/// `docs/content-authoring.md` for what each piece is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ThemeDef {
    /// Each piece's image, by piece name (such as `"window"`).
    #[serde(default)]
    pub pieces: BTreeMap<String, ThemePiece>,
    /// The toolbar's icon for each tab of the character window, by tab name
    /// (such as `"stats"`): PNGs in `ui/`.
    #[serde(default)]
    pub tab_icons: BTreeMap<String, String>,
    /// A font for all the interface's text (a TTF or OTF file in `ui/`), if
    /// not the built-in one.
    #[serde(default)]
    pub font: Option<String>,
}

/// The piece names a theme can give images to, with what each is.
pub const THEME_PIECES: [(&str, &str); 21] = [
    ("window", "the frame of a window (character window, shop, dialogue)"),
    ("header", "the strip across the top of a window, behind its title"),
    ("panel", "a smaller frame (player and target frames, chat, hotbar, toolbar)"),
    ("chat", "the chat log's backing (usually see-through)"),
    ("tooltip", "a tooltip's frame"),
    ("button", "a button with words on it"),
    ("button_hover", "a button under the mouse"),
    ("button_pressed", "a button being pressed"),
    ("button_chosen", "the button for the chosen option (a class or body type when making a character)"),
    ("slot", "a box holding an icon (bag, gear, abilities, hotbar)"),
    ("slot_hover", "a slot under the mouse"),
    ("bar_back", "the empty part of a bar: drawn light, and tinted a darker shade of each bar's colour"),
    ("bar_fill", "the full part of a bar: drawn light, and tinted each bar's colour"),
    ("bar_frame", "drawn over a whole bar, not tinted: its rim, or a shine"),
    ("close", "a window's close button"),
    ("close_hover", "a close button under the mouse"),
    ("divider", "a line under headings"),
    ("text_field", "a box to type in (chat, names)"),
    ("toolbar_button", "a toolbar button, behind its icon"),
    ("toolbar_button_hover", "a toolbar button under the mouse"),
    ("toolbar_button_open", "the toolbar button for the tab that's open"),
];

/// One piece's image, drawn as a nine-slice: the corners stay as they are,
/// the edges stretch (or repeat) along, and the middle fills the rest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ThemePiece {
    /// The PNG in `ui/`.
    pub image: String,
    /// How wide the corners and edges are, in the image's pixels: one number
    /// for all four sides, or `(left, right, top, bottom)`.
    #[serde(default)]
    pub border: ThemeBorder,
    /// Whether the edges and middle repeat (rather than stretch).
    #[serde(default)]
    pub tile: bool,
}

/// How wide a piece's border is, in its image's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(untagged)]
pub enum ThemeBorder {
    /// The same on every side.
    All(f32),
    /// Left, right, top and bottom.
    Sides(f32, f32, f32, f32),
}

impl Default for ThemeBorder {
    fn default() -> Self {
        ThemeBorder::All(0.0)
    }
}

impl ThemeBorder {
    /// Left, right, top and bottom.
    pub fn sides(self) -> [f32; 4] {
        match self {
            ThemeBorder::All(width) => [width; 4],
            ThemeBorder::Sides(left, right, top, bottom) => [left, right, top, bottom],
        }
    }
}

/// How an ability's effect looks (placeholder effects drawn by the client).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct AbilityVisual {
    /// Its colour.
    pub colour: Colour,
    /// How big it is, with 1 as normal.
    pub size: f32,
}

impl Default for AbilityVisual {
    fn default() -> Self {
        Self { colour: (1.0, 0.9, 0.7), size: 1.0 }
    }
}

/// The game's own sounds (as opposed to abilities' sounds): files in `sounds/`,
/// each optional. Read from `sounds.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct GameSounds {
    /// An attack that hits.
    #[serde(default)]
    pub hit: Option<String>,
    /// A critical hit.
    #[serde(default)]
    pub critical: Option<String>,
    /// An attack that misses.
    #[serde(default)]
    pub miss: Option<String>,
    /// Something dying.
    #[serde(default)]
    pub death: Option<String>,
    /// The player reaching a new level.
    #[serde(default)]
    pub level_up: Option<String>,
}

impl GameSounds {
    /// Every sound file named.
    pub fn files(&self) -> impl Iterator<Item = &String> {
        [&self.hit, &self.critical, &self.miss, &self.death, &self.level_up].into_iter().flatten()
    }
}

/// What an ability does when used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum AbilityEffect {
    /// Sit down and recover health and mana faster, until you move.
    Rest,
    /// Hit the target with the weapon, for this damage instead of the weapon's.
    Strike {
        /// Its damage dice, before the weapon stat's bonus.
        damage: Dice,
    },
    /// Throw a spell at the target, which flies to it.
    Bolt {
        /// Its damage dice, before the casting stat's bonus.
        damage: Dice,
        /// How fast it flies, in metres per second.
        #[serde(default = "default_bolt_speed")]
        speed: f32,
    },
    /// Heal yourself.
    Heal {
        /// How much health comes back.
        amount: Dice,
    },
}

/// An ability a class can learn: from which level, and for how many ability points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ClassAbility {
    /// Which ability (a [`AbilityDef`] id).
    pub ability: String,
    /// The level it can be learned from.
    pub level: u32,
    /// What it costs, in ability points.
    pub cost: u32,
}

impl ItemDef {
    /// What shops pay for one, if they'll buy it at all: half its value
    /// (rounded down, but at least 1 coin).
    pub fn sell_price(&self) -> Option<u32> {
        (self.value > 0).then(|| (self.value / 2).max(1))
    }
}

/// A shop: what it sells, and for how much. A shop is opened from an NPC's
/// dialogue (`<<open_shop elsies_bakery>>`), and it buys anything with a value.
///
/// ```ron
/// (id: "elsies_bakery", name: "Elsie's Bakery", stock: [(item: "bread", price: 5)])
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ShopDef {
    /// Unique name, used by dialogue to open it.
    pub id: String,
    /// The name shown at the top of the shop window.
    pub name: String,
    /// What it sells, in the order shown.
    pub stock: Vec<ShopItem>,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// Something a shop sells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ShopItem {
    /// Which item (an [`ItemDef`] id).
    pub item: String,
    /// What one costs, in Miasma.
    pub price: u32,
}

/// A quest: a story the player follows through named stages. Dialogue moves
/// it on (`<<set_quest tomas_pipe found>>`); reaching the last stage
/// completes it. Stages only move forward.
///
/// ```ron
/// (id: "tomas_pipe", name: "Old Tomas's Pipe", stages: [
///     (id: "asked", journal: "Old Tomas has lost his pipe."),
///     (id: "done", journal: "I found it for him."),
/// ])
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct QuestDef {
    /// Unique name, used by dialogue (and saved in profiles, so don't rename lightly).
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// The stages, in order. The last one completes the quest.
    pub stages: Vec<QuestStage>,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// One stage of a quest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct QuestStage {
    /// Its name, unique within the quest (and saved in profiles).
    pub id: String,
    /// What the player's journal says while the quest is at this stage.
    pub journal: String,
}

impl QuestDef {
    /// Where a stage comes in the quest, if it's one of its stages.
    pub fn stage_index(&self, stage: &str) -> Option<usize> {
        self.stages.iter().position(|candidate| candidate.id == stage)
    }

    /// Whether a stage is the last one, which completes the quest.
    pub fn is_final(&self, stage: &str) -> bool {
        self.stages.last().is_some_and(|last| last.id == stage)
    }
}

/// What new characters start with (whatever their class).
///
/// ```ron
/// (miasma: 25, items: [(item: "bread", count: 1)])
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct NewCharacterDef {
    /// Their money.
    #[serde(default)]
    pub miasma: u64,
    /// The items in their bag.
    #[serde(default)]
    pub items: Vec<ItemCount>,
    /// The abilities every character knows from the start (such as resting).
    #[serde(default)]
    pub abilities: Vec<String>,
}

/// A class, which players choose when they make a character.
///
/// ```ron
/// (id: "fighter", name: "Fighter", description: "...",
///  rules: (hit_die: 10, mana_per_level: 3, casting_stat: Wisdom),
///  stats: (strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10),
///  starting_equipment: ["training_sword"], starting_abilities: ["whack"],
///  abilities: [(ability: "shield_bash", level: 3, cost: 1)])
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ClassDef {
    /// Unique name (saved in profiles, so don't rename lightly).
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// A sentence or two, shown when choosing a class.
    pub description: String,
    /// Its part in the RPG rules: health and mana per level.
    pub rules: ClassRules,
    /// The stats a new character of this class starts with.
    pub stats: Stats,
    /// The items a new character of this class has equipped.
    #[serde(default)]
    pub starting_equipment: Vec<String>,
    /// The abilities a new character of this class knows (on top of everyone's).
    #[serde(default)]
    pub starting_abilities: Vec<String>,
    /// The abilities the class can learn with ability points.
    #[serde(default)]
    pub abilities: Vec<ClassAbility>,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// A number of one kind of item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct ItemCount {
    /// Which item (an [`ItemDef`] id).
    pub item: String,
    /// How many.
    pub count: u32,
}

/// The most characters in an item's name.
pub(crate) const MAX_ITEM_NAME_CHARS: usize = 32;
/// The most characters in an item's description.
pub(crate) const MAX_ITEM_DESCRIPTION_CHARS: usize = 200;

/// The largest a class's hit die may be.
pub(crate) const MAX_HIT_DIE: u32 = 20;

/// A creature that can be fought.
///
/// ```ron
/// (id: "rabbit", name: "Rabbit", level: 1, health: 4, armour_class: 8,
///  attack: (damage: "1d2", bonus: 0, speed: 0.6), experience: 10, wander_radius: 6.0,
///  look: (shape: Rabbit, main: (0.55, 0.45, 0.35), trim: (0.92, 0.9, 0.86)))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct CreatureDef {
    /// Unique name, used by spawners.
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// Its level, shown to players.
    pub level: u32,
    /// Its most health.
    pub health: u32,
    /// How hard it is to hit (SRD 5.1: attacks hit if d20 + bonus reaches it).
    pub armour_class: i32,
    /// How it fights back.
    pub attack: CreatureAttack,
    /// Whether it attacks players who come near, rather than only fighting back.
    #[serde(default)]
    pub aggressive: bool,
    /// How near (in metres) a player must come for an aggressive creature to notice them.
    #[serde(default = "default_aggro_radius")]
    pub aggro_radius: f32,
    /// How far from home (in metres) it wanders when it isn't fighting.
    #[serde(default)]
    pub wander_radius: f32,
    /// Experience for killing it.
    pub experience: u64,
    /// How it looks.
    pub look: CreatureLook,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

fn default_aggro_radius() -> f32 {
    10.0
}

/// How a creature attacks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct CreatureAttack {
    /// Its damage dice (such as `"1d4+1"`).
    pub damage: Dice,
    /// Added to its attack rolls.
    pub bonus: i32,
    /// Attacks per second.
    pub speed: f32,
}

/// A spawner: keeps one creature alive somewhere, bringing a new one a while
/// after it dies.
///
/// ```ron
/// (id: "rabbit_east", creature: "rabbit", at: StartingSettlement(offset: (35.0, 5.0)),
///  radius: 8.0, respawn_seconds: 20.0)
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct SpawnerDef {
    /// Unique name.
    pub id: String,
    /// Which creature (a [`CreatureDef`] id).
    pub creature: String,
    /// Where: creatures appear within `radius` of here.
    pub at: Placement,
    /// How far from `at` (in metres) a creature may appear.
    #[serde(default)]
    pub radius: f32,
    /// How long after one dies the next appears, in seconds.
    pub respawn_seconds: f32,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// A place players wake up after dying. They wake at the nearest one.
///
/// ```ron
/// (name: "the town square", at: StartingSettlement(offset: (0.0, 0.0)))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct RespawnPoint {
    /// Its name, as in "You wake up in {name}."
    pub name: String,
    /// Where it is.
    pub at: Placement,
    /// Notes for authors (the game ignores them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// The most characters in an NPC's name.
pub(crate) const MAX_NPC_NAME_CHARS: usize = 32;
/// The furthest an NPC may be allowed to wander from home, in metres.
pub(crate) const MAX_WANDER_RADIUS: f32 = 50.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn appearance(id: &str) -> AppearanceDef {
        let colour = (0.5, 0.5, 0.5);
        AppearanceDef {
            id: id.to_string(),
            skin: colour,
            hair: colour,
            tunic: colour,
            trousers: colour,
            body: BodyType::default(),
            outfit: OutfitStyle::default(),
            hairstyle: None,
            parts: Vec::new(),
            beard: None,
            height: Some(9),
            build: None,
            head: None,
            notes: String::new(),
        }
    }

    #[test]
    fn picked_features_are_ordinary_and_always_the_same() {
        let features = appearance("guard").features();
        assert_eq!(features, appearance("guard").features());
        assert_eq!(features.height, 9);
        for id in ["guard", "baker", "farmer", "elder", "child", "smith"] {
            let features = appearance(id).features();
            assert!(features.build.abs() <= PICKED_RANGE && features.head.abs() <= PICKED_RANGE);
        }
        let builds: std::collections::BTreeSet<i8> =
            ["a", "b", "c", "d", "e", "f"].map(|id| appearance(id).features().build).into();
        assert!(builds.len() > 1, "different people should differ");
    }
}
