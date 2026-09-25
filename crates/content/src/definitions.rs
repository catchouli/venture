//! The kinds of things a content pack defines.

use serde::{Deserialize, Serialize};
use venture_protocol::{BodyType, Outfit, OutfitStyle};
use venture_rpg::{Abilities, ClassRules};

/// A colour as sRGB red, green and blue, each from 0 to 1.
pub type Colour = (f32, f32, f32);

/// How a character looks.
///
/// ```ron
/// (id: "villager_green", skin: (0.86, 0.66, 0.53), hair: (0.3, 0.19, 0.11),
///  tunic: (0.25, 0.35, 0.22), trousers: (0.3, 0.26, 0.22))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
}

/// A non-player character.
///
/// ```ron
/// (id: "millbrook_guard", name: "Guard Aldren", appearance: "guard",
///  at: StartingSettlement(offset: (6.0, 3.0)), script: Some("guard.rhai"), dialogue: Some("Guard"))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
}

/// Where something is placed in the world. Placing things relative to
/// settlements keeps content in the right spot even when the terrain
/// generator is tuned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[serde(deny_unknown_fields)]
pub struct ShopDef {
    /// Unique name, used by dialogue to open it.
    pub id: String,
    /// The name shown at the top of the shop window.
    pub name: String,
    /// What it sells, in the order shown.
    pub stock: Vec<ShopItem>,
}

/// Something a shop sells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[serde(deny_unknown_fields)]
pub struct QuestDef {
    /// Unique name, used by dialogue (and saved in profiles, so don't rename lightly).
    pub id: String,
    /// The name shown to players.
    pub name: String,
    /// The stages, in order. The last one completes the quest.
    pub stages: Vec<QuestStage>,
}

/// One stage of a quest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[serde(deny_unknown_fields)]
pub struct NewCharacterDef {
    /// Their money.
    #[serde(default)]
    pub miasma: u64,
    /// The items in their bag.
    #[serde(default)]
    pub items: Vec<ItemCount>,
}

/// A class, which players choose when they make a character.
///
/// ```ron
/// (id: "fighter", name: "Fighter", description: "...",
///  rules: (hit_die: 10, mana_per_level: 3, casting_ability: Wisdom),
///  stats: (strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// The ability scores a new character of this class starts with.
    pub stats: Abilities,
    /// What its characters wear (until they wear equipment).
    ///
    /// ```ron
    /// outfit: (style: Robe, main: (0.3, 0.3, 0.55), trim: (0.8, 0.7, 0.4))
    /// ```
    pub outfit: Outfit,
}

/// A number of one kind of item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// The most characters in an NPC's name.
pub(crate) const MAX_NPC_NAME_CHARS: usize = 32;
/// The furthest an NPC may be allowed to wander from home, in metres.
pub(crate) const MAX_WANDER_RADIUS: f32 = 50.0;
