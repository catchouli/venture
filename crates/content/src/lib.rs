//! The content pack: what exists in the world on top of the generated land,
//! and how it behaves. See `docs/content-authoring.md`.
//!
//! A content pack is a folder:
//!
//! ```text
//! content/
//!   appearances/*.ron   how characters look (sent to clients)
//!   npcs/*.ron          who lives where (server only)
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
mod load;
#[cfg(feature = "scripts")]
pub mod script_hooks;

pub use client::{CLIENT_MANIFEST_FILE, ClientContent, ContentManifest, ManifestFile, parse_appearances};
pub use definitions::{AppearanceDef, Colour, NpcDef, Placement};
pub use load::{ContentError, ContentPack};
