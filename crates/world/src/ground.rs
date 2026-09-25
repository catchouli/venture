//! What the terrain is made of and covered by.

use crate::Biome;

/// The material on the surface of a terrain cell. Decides how it looks and,
/// later, gameplay properties such as footstep sounds and what can be gathered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ground {
    /// Ordinary grass.
    Grass,
    /// Lush grass full of flowers, found in meadows.
    MeadowGrass,
    /// Bare earth.
    Dirt,
    /// Beaches, river banks and lake beds.
    Sand,
    /// Exposed stone on steep slopes and high mountains.
    Rock,
    /// Snow on the highest peaks.
    Snow,
    /// Packed-earth roads and paths.
    Path,
    /// Grey volcanic ash.
    Ash,
    /// Black volcanic rock.
    Basalt,
}

/// The kind of liquid covering a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FluidKind {
    /// Rivers, lakes and the sea.
    Water,
    /// Molten rock, in volcanic regions.
    Lava,
}

/// A liquid surface over a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fluid {
    /// What the liquid is.
    pub kind: FluidKind,
    /// Height of the surface, in metres.
    pub level: f32,
}

/// Everything stored about one terrain cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainCell {
    /// The surface material.
    pub ground: Ground,
    /// Liquid covering the cell, if any.
    pub fluid: Option<Fluid>,
    /// The biome the cell belongs to.
    pub biome: Biome,
}

impl Default for TerrainCell {
    fn default() -> Self {
        Self { ground: Ground::Grass, fluid: None, biome: Biome::Grassland }
    }
}
