//! What an author chooses for one tree, on top of its species: age, height,
//! how branchy and how full it is, how it leans, and its seed.

use serde::{Deserialize, Serialize};

/// The choices for one tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct TreeParams {
    /// How old it is: 0.1 a sapling, 0.5 young, 1 mature, 2 ancient.
    pub age: f32,
    /// How tall it is for its species and age, from 0 (the shortest) to 1 (the tallest).
    pub height: f32,
    /// How many branches it has, compared with its species (1 is usual).
    pub branchiness: f32,
    /// How many leaves it has, compared with its species (1 is usual, 0 bare).
    pub fullness: f32,
    /// How much more (or less) it leans than its species usually does (1 is usual).
    pub lean: f32,
    /// The seed: the same seed (and choices) always grows the same tree.
    pub seed: u64,
}

impl Default for TreeParams {
    fn default() -> Self {
        Self { age: 1.0, height: 0.5, branchiness: 1.0, fullness: 1.0, lean: 1.0, seed: 1 }
    }
}

/// The youngest a tree can be.
const YOUNGEST: f32 = 0.05;
/// The oldest a tree can be.
const OLDEST: f32 = 3.0;

impl TreeParams {
    /// The age, kept within what trees can be.
    pub fn age(&self) -> f32 {
        if self.age.is_finite() { self.age.clamp(YOUNGEST, OLDEST) } else { 1.0 }
    }

    /// How tall it is compared with a mature tree: quick when young,
    /// levelling off once mature (old trees barely grow taller).
    pub fn height_growth(&self) -> f32 {
        let age = self.age();
        if age <= 1.0 { age.powf(0.75) } else { 1.0 + 0.06 * (age - 1.0) }
    }

    /// How thick its trunk is for its height, compared with a mature tree:
    /// trunks keep thickening long after trees stop growing taller.
    pub fn girth_growth(&self) -> f32 {
        0.65 + 0.35 * self.age()
    }

    /// How many of its species' branches it has grown (young trees have fewer).
    pub fn branch_growth(&self) -> f32 {
        (0.3 + 0.7 * self.age().min(1.0)) * self.branchiness.clamp(0.0, 4.0)
    }

    /// How far its crown has spread, compared with a mature tree's (old
    /// trees spread wider).
    pub fn spread_growth(&self) -> f32 {
        let age = self.age();
        if age <= 1.0 { 0.75 + 0.25 * age } else { 1.0 + 0.12 * (age - 1.0) }
    }

    /// How much of its trunk is clear of branches, compared with a mature
    /// tree (saplings branch low; old trees have shed their lower limbs).
    pub fn clear_growth(&self) -> f32 {
        let age = self.age();
        if age <= 1.0 { 0.4 + 0.6 * age } else { 1.0 + 0.15 * (age - 1.0) }
    }
}
