//! The five abilities (stats) every character and creature has, and the
//! modifier each gives (SRD 5.1).

use serde::{Deserialize, Serialize};

/// The lowest an ability score can be.
pub const MIN_SCORE: i32 = 1;
/// The highest an ability score can be.
pub const MAX_SCORE: i32 = 99;

/// One of the five abilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ability {
    /// Strength: melee hits and damage.
    Strength,
    /// Dexterity: armour class, attack speed, and finesse weapons.
    Dexterity,
    /// Constitution: health.
    Constitution,
    /// Intelligence: spells.
    Intelligence,
    /// Wisdom: mana and resisting magic.
    Wisdom,
}

impl Ability {
    /// Every ability, in the order they're shown.
    pub const ALL: [Ability; 5] =
        [Ability::Strength, Ability::Dexterity, Ability::Constitution, Ability::Intelligence, Ability::Wisdom];

    /// Its short name, as shown on the stats tab: "STR".
    pub fn short_name(self) -> &'static str {
        match self {
            Ability::Strength => "STR",
            Ability::Dexterity => "DEX",
            Ability::Constitution => "CON",
            Ability::Intelligence => "INT",
            Ability::Wisdom => "WIS",
        }
    }
}

/// A set of ability scores. 10 is average for a person; each 2 points above
/// or below changes the modifier by 1.
///
/// ```ron
/// (strength: 14, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Abilities {
    /// Strength.
    pub strength: i32,
    /// Dexterity.
    pub dexterity: i32,
    /// Constitution.
    pub constitution: i32,
    /// Intelligence.
    pub intelligence: i32,
    /// Wisdom.
    pub wisdom: i32,
}

impl Default for Abilities {
    /// Average in everything.
    fn default() -> Self {
        Self { strength: 10, dexterity: 10, constitution: 10, intelligence: 10, wisdom: 10 }
    }
}

impl Abilities {
    /// One ability's score.
    pub fn score(&self, ability: Ability) -> i32 {
        match ability {
            Ability::Strength => self.strength,
            Ability::Dexterity => self.dexterity,
            Ability::Constitution => self.constitution,
            Ability::Intelligence => self.intelligence,
            Ability::Wisdom => self.wisdom,
        }
    }

    /// One ability's score, to change.
    pub fn score_mut(&mut self, ability: Ability) -> &mut i32 {
        match ability {
            Ability::Strength => &mut self.strength,
            Ability::Dexterity => &mut self.dexterity,
            Ability::Constitution => &mut self.constitution,
            Ability::Intelligence => &mut self.intelligence,
            Ability::Wisdom => &mut self.wisdom,
        }
    }

    /// One ability's modifier.
    pub fn modifier(&self, ability: Ability) -> i32 {
        modifier(self.score(ability))
    }

    /// Whether every score is within [`MIN_SCORE`] and [`MAX_SCORE`].
    pub fn is_valid(&self) -> bool {
        Ability::ALL.iter().all(|&ability| (MIN_SCORE..=MAX_SCORE).contains(&self.score(ability)))
    }
}

/// The modifier an ability score gives: `floor((score − 10) / 2)` (SRD 5.1).
/// So 10 and 11 give +0, 12 gives +1, 8 gives −1.
pub fn modifier(score: i32) -> i32 {
    (score - 10).div_euclid(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_follow_the_srd_table() {
        let table = [(1, -5), (3, -4), (8, -1), (9, -1), (10, 0), (11, 0), (12, 1), (15, 2), (20, 5), (30, 10)];
        for (score, expected) in table {
            assert_eq!(modifier(score), expected, "score {score}");
        }
    }

    #[test]
    fn scores_are_read_and_changed_by_ability() {
        let mut abilities = Abilities::default();
        *abilities.score_mut(Ability::Wisdom) += 4;
        assert_eq!(abilities.score(Ability::Wisdom), 14);
        assert_eq!(abilities.modifier(Ability::Wisdom), 2);
        assert!(abilities.is_valid());
        abilities.strength = 0;
        assert!(!abilities.is_valid());
    }
}
