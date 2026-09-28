//! Felling trees and cutting them up for logs (part of the Woodworking
//! skill: see `docs/plan-trees.md`).
//!
//! A player chops with an axe, one swing at a time. Each swing may **bite**:
//! more likely the further their level is above the tree's, and with a
//! better axe. A tree falls after enough bites (more for bigger and harder
//! trees). Then each bite into the fallen tree gives a log, until its logs
//! run out (more in bigger trees).

use serde::{Deserialize, Serialize};

/// The least chance a swing has of biting, however hard the tree.
const LEAST_BITE: f32 = 0.08;
/// The most chance a swing has of biting, however easy the tree.
const MOST_BITE: f32 = 0.9;
/// The chance of a bite at exactly the tree's level, with a plain axe (power 1).
const BITE_AT_LEVEL: f32 = 0.3;
/// How much each level above the tree's adds to the chance.
const BITE_PER_LEVEL: f32 = 0.015;

/// How long a fallen tree's trunk is, by size (young, mature, old), in
/// metres: a player can cut it up anywhere along it.
pub const FALLEN_LENGTHS: [f32; 3] = [7.0, 15.0, 20.0];

/// How big a tree is (the tree generator's variants).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum TreeSize {
    /// A young tree.
    Young,
    /// A mature one.
    Mature,
    /// An old one.
    Old,
}

impl TreeSize {
    /// The size for a tree variant (0 young, 1 mature, 2 old).
    pub fn of_variant(variant: u8) -> TreeSize {
        match variant {
            0 => TreeSize::Young,
            1 => TreeSize::Mature,
            _ => TreeSize::Old,
        }
    }
}

/// Whether a player of `skill_level` can fell a tree of `tree_level`.
pub fn can_fell(skill_level: u32, tree_level: u32) -> bool {
    skill_level >= tree_level
}

/// The chance (0 to 1) a swing bites, for a player of `skill_level` on a
/// tree of `tree_level`, with an axe of `axe_power` (1 for a stone axe;
/// better axes more).
pub fn bite_chance(skill_level: u32, tree_level: u32, axe_power: f32) -> f32 {
    let above = skill_level as f32 - tree_level as f32;
    ((BITE_AT_LEVEL + BITE_PER_LEVEL * above) * axe_power.max(0.1)).clamp(LEAST_BITE, MOST_BITE)
}

/// How many bites fell a tree of `size`, for a wood of `hardness` (the
/// bites a mature tree takes).
pub fn bites_to_fell(size: TreeSize, hardness: u32) -> u32 {
    let share = match size {
        TreeSize::Young => 0.5,
        TreeSize::Mature => 1.0,
        TreeSize::Old => 1.6,
    };
    ((hardness.max(1) as f32 * share).round() as u32).max(1)
}

/// How many logs a fallen tree of `size` gives.
pub fn logs_in(size: TreeSize) -> u32 {
    match size {
        TreeSize::Young => 2,
        TreeSize::Mature => 4,
        TreeSize::Old => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_levels_and_better_axes_bite_more_often() {
        assert!(bite_chance(20, 10, 1.0) > bite_chance(10, 10, 1.0));
        assert!(bite_chance(10, 10, 1.5) > bite_chance(10, 10, 1.0));
        assert_eq!(bite_chance(99, 1, 3.0), MOST_BITE);
        assert_eq!(bite_chance(1, 60, 0.5), LEAST_BITE);
        assert!((bite_chance(10, 10, 1.0) - BITE_AT_LEVEL).abs() < 1e-6);
    }

    #[test]
    fn only_trees_at_or_below_your_level_can_be_felled() {
        assert!(can_fell(10, 10));
        assert!(!can_fell(9, 10));
    }

    #[test]
    fn bigger_trees_take_longer_and_give_more() {
        assert!(bites_to_fell(TreeSize::Young, 4) < bites_to_fell(TreeSize::Mature, 4));
        assert!(bites_to_fell(TreeSize::Mature, 4) < bites_to_fell(TreeSize::Old, 4));
        assert_eq!(bites_to_fell(TreeSize::Young, 1), 1, "always at least one");
        assert!(logs_in(TreeSize::Young) < logs_in(TreeSize::Old));
        assert_eq!(TreeSize::of_variant(1), TreeSize::Mature);
    }
}
