//! A chunk's navigation mesh tile (see `docs/plan-collision.md`): where a
//! character can stand, as convex polygons, baked from the world's solid
//! shapes (by `venture_navigation`, with Recast). Routes are found over the
//! tiles of neighbouring chunks, joined where their edges meet.

use serde::{Deserialize, Serialize};

/// What a navigation mesh polygon is: how it's walked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NavKind {
    /// Dry ground (or a floor).
    #[default]
    Ground,
    /// Shallow water, waded through (slower, so routes keep out of it when they can).
    Shallows,
}

/// One way down: a drop from the edge of somewhere to the ground below it (a
/// bank too steep to climb, say), which can be jumped or fallen down but not
/// climbed back up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavDrop {
    /// The edge it's taken from: two points (`x`, `y`, height), in metres.
    pub from: [[f32; 3]; 2],
    /// Where it lands: two points under them, in the same order.
    pub to: [[f32; 3]; 2],
}

/// A chunk's navigation mesh tile. Points are `[x, y, height]` in the world, in metres.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NavTile {
    /// The polygons' corners.
    pub vertices: Vec<[f32; 3]>,
    /// The polygons: convex, anticlockwise seen from above, as indices into `vertices`.
    pub polygons: Vec<Vec<u16>>,
    /// Each polygon's kind.
    pub kinds: Vec<NavKind>,
    /// The ground's height in detail, polygon by polygon (a polygon's corners
    /// alone can't follow the ground's bumps): each polygon's triangles, with
    /// their own corners.
    pub detail: Vec<NavDetail>,
    /// The ways down from this tile.
    pub drops: Vec<NavDrop>,
}

/// One polygon's detail: triangles over it, following the ground.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NavDetail {
    /// The triangles' corners.
    pub vertices: Vec<[f32; 3]>,
    /// The triangles, as indices into `vertices`.
    pub triangles: Vec<[u8; 3]>,
}

impl NavTile {
    /// Whether it has nowhere to stand.
    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }
}
