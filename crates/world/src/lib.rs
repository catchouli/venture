//! Core world data types shared by every part of Venture.
//!
//! This crate is plain data with no engine or rendering dependencies, so the
//! client, the world generator and (later) the server can all use it.
//! See `docs/world-model.md` for the concepts behind these types.
//!
//! Units: one unit is one metre. The terrain is sampled on a grid of
//! [`CELL_SIZE`]-metre cells, but nothing in the game is locked to that grid:
//! characters and objects have continuous positions.

mod baked;
mod biome;
mod chunk;
mod coords;
mod ground;
mod prop;

pub use baked::{
    BakedWorldError, CHUNK_FORMAT_VERSION, MANIFEST_FILE, SettlementInfo, WorldManifest, chunk_file_path, decode_chunk,
    encode_chunk,
};
pub use biome::Biome;
pub use chunk::Chunk;
pub use coords::{CellPos, ChunkPos};
pub use ground::{Fluid, FluidKind, Ground, TerrainCell};
pub use prop::{Prop, PropKind};

/// Width of one terrain cell, in metres.
pub const CELL_SIZE: f32 = 1.0;

/// Width and depth of a chunk, in cells.
pub const CHUNK_SIZE: i32 = 64;

/// Number of height corners along one edge of a chunk (one more than cells).
pub const CHUNK_CORNERS: i32 = CHUNK_SIZE + 1;
