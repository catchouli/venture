//! The content pack: what exists in the world on top of the generated land,
//! and how it behaves. See `docs/content-authoring.md`.
//!
//! A content pack is a folder:
//!
//! ```text
//! content/
//!   appearances/*.ron   how characters look (sent to clients)
//!   items/*.ron         things players can carry (sent to clients)
//!   icons/*.png         item pictures (sent to clients)
//!   new_characters.ron  what new characters start with (server only)
//!   classes/*.ron       the classes players choose from (server only)
//!   creatures/*.ron     creatures that can be fought (server only)
//!   spawners/*.ron      where creatures live (server only)
//!   respawn_points.ron  where players wake up after dying (server only)
//!   skills/*.ron        what skills do (sent to clients)
//!   npcs/*.ron          who lives where (server only)
//!   shops/*.ron         what shops sell (server only)
//!   quests/*.ron        quests and their stages (server only)
//!   scripts/*.rhai      how NPCs behave (server only)
//!   dialogue/*.yarn     conversations with NPCs (server only)
//! ```
//!
//! [`ContentPack::load`] reads and checks everything, and reports every
//! problem it finds (with its file) rather than stopping at the first.
//! [`ClientContent`] is the part clients download, described by a
//! [`ContentManifest`] of hashes so they only fetch what changed.

mod client;
mod definitions;
#[cfg(feature = "scripts")]
pub mod dialogue;
#[cfg(feature = "scripts")]
pub mod dialogue_commands;
mod load;
#[cfg(feature = "scripts")]
pub mod script_hooks;

pub use client::{
    CLIENT_MANIFEST_FILE, ClientContent, ContentManifest, ManifestFile, parse_appearances, parse_items, parse_skills,
};
pub use definitions::{
    AppearanceDef, ClassDef, ClassSkill, Colour, CreatureAttack, CreatureDef, Equippable, ItemCount, ItemDef,
    NewCharacterDef, NpcDef, OutfitStyle, Placement, QuestDef, QuestStage, RespawnPoint, ShopDef, ShopItem, SkillDef,
    SkillEffect, SpawnerDef, WeaponDef,
};
pub use load::{ContentError, ContentPack};
