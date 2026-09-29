//! Buildings (see `docs/plan-buildings.md`): what an author draws (a
//! [`BuildingPlan`]: its outline, inner walls, doors, style, floors and
//! seed), and a building as the bake leaves it ([`BakedBuilding`]: the plan
//! and the level of its ground floor). Clients build its meshes from these
//! (with `venture_structures` and the style from the content).

use serde::{Deserialize, Serialize};

/// The shape of a building's roof.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum RoofShape {
    /// Two slopes meeting at a ridge, with upright gable walls at the ends.
    #[default]
    Gable,
    /// Sloping on every side (the ends too).
    Hip,
    /// Flat, behind a parapet (battlemented, if its style says).
    Flat,
}

impl RoofShape {
    /// Every shape, in the order they're offered.
    pub const ALL: [RoofShape; 3] = [RoofShape::Gable, RoofShape::Hip, RoofShape::Flat];

    /// Its name, for people.
    pub fn name(self) -> &'static str {
        match self {
            RoofShape::Gable => "Gable",
            RoofShape::Hip => "Hip",
            RoofShape::Flat => "Flat",
        }
    }
}

/// Which wall a door is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum WallRef {
    /// An outer wall: the outline's edge from its point `n` to the next.
    Outer(u32),
    /// An inner wall, by its place in the list.
    Inner(u32),
}

/// A door: which wall it's in, and how far along it (0 to 1, to its middle).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct DoorPlan {
    /// Its wall.
    pub wall: WallRef,
    /// How far along the wall its middle is (0 to 1).
    pub along: f32,
}

/// A building as an author draws it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct BuildingPlan {
    /// Its id.
    pub id: String,
    /// Its style (an id in the content's `buildings/styles.ron`).
    pub style: String,
    /// How many floors it has.
    #[serde(default = "one")]
    pub floors: u32,
    /// Its seed: the same seed always builds the same building.
    #[serde(default)]
    pub seed: u64,
    /// Its roof's shape.
    #[serde(default)]
    pub roof: RoofShape,
    /// Whether it has a chimney (`None`: as its style usually does).
    #[serde(default)]
    pub chimney: Option<bool>,
    /// Its outline: the outside of its walls, corner by corner, in metres
    /// (every corner a right angle).
    pub outline: Vec<(f32, f32)>,
    /// Walls inside it, from one point to another (along their middles), in metres.
    #[serde(default)]
    pub inner_walls: Vec<((f32, f32), (f32, f32))>,
    /// Its doors.
    #[serde(default)]
    pub doors: Vec<DoorPlan>,
}

fn one() -> u32 {
    1
}

/// A building as the bake leaves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BakedBuilding {
    /// What the author drew.
    pub plan: BuildingPlan,
    /// The height of its ground floor, in metres (the ground under it is
    /// levelled to this).
    pub floor: f32,
    /// The lowest the ground was under it, before it was levelled (for how
    /// deep its footing goes).
    pub lowest: f32,
}
