//! The content pack: what exists in the world on top of the generated land,
//! and how it behaves. See `docs/content-authoring.md`.
//!
//! A content pack is a folder:
//!
//! ```text
//! content/
//!   appearances/*.ron   how characters look (sent to clients)
//!   items/*.ron         things players can carry (sent to clients)
//!   icons/*.png         item and ability pictures (sent to clients)
//!   sounds/*.wav        sounds (sent to clients)
//!   sounds.ron          the game's own sounds: hits, misses, levelling up (sent to clients)
//!   new_characters.ron  what new characters start with (server only)
//!   classes/*.ron       the classes players choose from (server only)
//!   creatures/*.ron     creatures that can be fought (server only)
//!   spawners/*.ron      where creatures live (server only)
//!   respawn_points.ron  where players wake up after dying (server only)
//!   abilities/*.ron        what abilities do (sent to clients)
//!   skills/*.ron           gathering and making skills (sent to clients)
//!   ui/theme.ron, ui/*.png the interface's look (sent to clients)
//!   characters/            character models, outfits and animations (sent to clients)
//!   ui/layouts/*.ron       where each window's parts go (sent to clients)
//!   ui/styles.ron          the interface's text styles and colours (sent to clients)
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

pub mod characters;
mod client;
mod definitions;
#[cfg(feature = "scripts")]
pub mod dialogue;
#[cfg(feature = "scripts")]
pub mod dialogue_commands;
pub mod layout;
mod load;
#[cfg(feature = "scripts")]
pub mod script_hooks;

pub use client::{
    CLIENT_MANIFEST_FILE, ClientContent, ContentManifest, ManifestFile, parse_abilities, parse_appearances,
    parse_game_sounds, parse_items, parse_skills, parse_theme,
};
pub use definitions::{
    AbilityDef, AbilityEffect, AbilityVisual, AppearanceDef, ClassAbility, ClassDef, Colour, CreatureAttack,
    CreatureDef, Equippable, GameSounds, ItemCount, ItemDef, NewCharacterDef, NpcDef, OutfitStyle, QuestDef,
    QuestStage, RespawnPoint, ShopDef, ShopItem, SkillDef, SpawnerDef, THEME_PIECES, ThemeBorder, ThemeDef, ThemePiece,
    WeaponDef,
};
pub use load::{ContentError, ContentPack};
pub use venture_protocol::Refers;
