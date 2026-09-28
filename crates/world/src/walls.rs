//! Walls and fences, as the bake leaves them (see `docs/plan-walls.md`):
//! their kind, size and seed, and their path sampled evenly along its length
//! with the ground's height at each sample. Clients build their meshes from
//! these (with `venture_structures`), without needing the terrain.

use serde::{Deserialize, Serialize};

/// How far apart a baked wall's samples are, along its path, in metres.
pub const WALL_SAMPLE_SPACING: f32 = 0.5;

/// What kind of wall something is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum WallKind {
    /// Flat stones laid without mortar, with upright coping stones on top.
    #[default]
    DryStone,
    /// Posts driven into the ground, with rails between them.
    Fence,
}

impl WallKind {
    /// Its name, for people.
    pub fn name(self) -> &'static str {
        match self {
            WallKind::DryStone => "Dry-stone wall",
            WallKind::Fence => "Fence",
        }
    }

    /// How far either side of its line it blocks movement, in metres.
    pub fn half_width(self) -> f32 {
        match self {
            WallKind::DryStone => 0.4,
            WallKind::Fence => 0.15,
        }
    }
}

/// One sample along a baked wall's path.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WallSample {
    /// Where it is, in metres.
    pub x: f32,
    /// Where it is, in metres.
    pub y: f32,
    /// The ground's height there, in metres.
    pub ground: f32,
    /// Whether the wall has a gap here (such as where a road crosses it).
    pub open: bool,
}

/// A wall as the bake leaves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BakedWall {
    /// Its id (from the authored file).
    pub id: String,
    /// Its kind.
    pub kind: WallKind,
    /// How tall it is, in metres.
    pub height: f32,
    /// Its seed.
    pub seed: u64,
    /// Its path, sampled every [`WALL_SAMPLE_SPACING`] metres.
    pub samples: Vec<WallSample>,
}
