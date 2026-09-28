//! Props: trees (and bushes, which are young trees), rocks and dead trees standing on the terrain.

use serde::{Deserialize, Serialize};

/// What kind of prop something is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropKind {
    /// A tree, grown by the tree generator (see `docs/plan-trees.md`).
    Tree {
        /// Its species: an index into the world's [`crate::WorldManifest::tree_species`].
        species: u16,
        /// Which of its species' variants it is: 0 young, 1 mature, 2 old.
        variant: u8,
    },
    /// A bare, dead tree.
    DeadTree,
    /// A large rock.
    Boulder,
}

/// How many variants each tree species has (young, mature, old).
pub const TREE_VARIANTS: u8 = 3;

/// Trees scaled smaller than this are bushes (small young trees, in the undergrowth).
pub const BUSH_SCALE: f32 = 0.6;

/// One prop placed in the world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    /// What it is.
    pub kind: PropKind,
    /// Position along the east axis, in metres.
    pub x: f32,
    /// Position along the south axis, in metres.
    pub y: f32,
    /// Height of the ground at its base, in metres.
    pub ground_height: f32,
    /// Size relative to the kind's normal size.
    pub scale: f32,
    /// Rotation around the vertical axis, in radians.
    pub rotation: f32,
}
