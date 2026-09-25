//! Biomes: the broad regions the world is divided into.

use serde::{Deserialize, Serialize};

/// A kind of region. Each has its own terrain, materials and (later) wildlife,
/// resources and music, and is home to one of the peoples of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Biome {
    /// Open grassy plains and wide river valleys. Home of the humans.
    Grassland,
    /// Rolling, enchanted meadows full of flowers and ponds. Home of the elves.
    Meadow,
    /// Rocky mountains, gorges and caves. Home of the dwarves.
    Highlands,
    /// Ash fields, black rock and lava around volcanoes.
    Volcanic,
}

impl Biome {
    /// Every biome, in a fixed order.
    pub const ALL: [Biome; 4] = [Biome::Grassland, Biome::Meadow, Biome::Highlands, Biome::Volcanic];

    /// The number of biomes.
    pub const COUNT: usize = Self::ALL.len();

    /// This biome's position in [`Biome::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}
