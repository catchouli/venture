//! The kinds of things a content pack defines.

use serde::{Deserialize, Serialize};

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
}

/// A non-player character.
///
/// ```ron
/// (id: "millbrook_guard", name: "Guard Aldren", appearance: "guard",
///  at: StartingSettlement(offset: (6.0, 3.0)), script: Some("guard.rhai"))
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

/// The most characters in an NPC's name.
pub(crate) const MAX_NPC_NAME_CHARS: usize = 32;
/// The furthest an NPC may be allowed to wander from home, in metres.
pub(crate) const MAX_WANDER_RADIUS: f32 = 50.0;
