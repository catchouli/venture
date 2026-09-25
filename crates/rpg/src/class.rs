//! What a class contributes to a character's numbers. (Everything else about
//! a class, such as its name and skills, is in the content pack.)

use serde::{Deserialize, Serialize};

use crate::abilities::Ability;

/// A class's part in the rules.
///
/// ```ron
/// (hit_die: 10, mana_per_level: 3, casting_ability: Wisdom)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassRules {
    /// The size of the die for health per level (SRD 5.1: d10 for fighters,
    /// d6 for wizards).
    pub hit_die: u32,
    /// Mana gained per level, before the casting ability's modifier.
    pub mana_per_level: u32,
    /// The ability that adds to mana (and powers spells).
    pub casting_ability: Ability,
}

impl ClassRules {
    /// Health gained per level after the first: the hit die's average,
    /// rounded up (SRD 5.1's fixed value: 6 for a d10, 4 for a d6).
    pub fn health_per_level(&self) -> u32 {
        self.hit_die / 2 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_per_level_is_the_srd_fixed_value() {
        let rules = |hit_die| ClassRules { hit_die, mana_per_level: 0, casting_ability: Ability::Wisdom };
        assert_eq!(rules(10).health_per_level(), 6);
        assert_eq!(rules(8).health_per_level(), 5);
        assert_eq!(rules(6).health_per_level(), 4);
    }
}
