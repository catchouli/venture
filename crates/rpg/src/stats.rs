//! The five stats every character and creature has, and the
//! modifier each gives (SRD 5.1).

use serde::{Deserialize, Serialize};

/// The lowest a stat can be.
pub const MIN_SCORE: i32 = 1;
/// The highest a stat can be.
pub const MAX_SCORE: i32 = 99;

/// One of the five stats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Stat {
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

impl Stat {
    /// Every stat, in the order they're shown.
    pub const ALL: [Stat; 5] = [Stat::Strength, Stat::Dexterity, Stat::Constitution, Stat::Intelligence, Stat::Wisdom];

    /// Its short name, as shown on the stats tab: "STR".
    pub fn short_name(self) -> &'static str {
        match self {
            Stat::Strength => "STR",
            Stat::Dexterity => "DEX",
            Stat::Constitution => "CON",
            Stat::Intelligence => "INT",
            Stat::Wisdom => "WIS",
        }
    }
}

/// A set of stats. 10 is average for a person; each 2 points above
/// or below changes the modifier by 1.
///
/// ```ron
/// (strength: 14, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct Stats {
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

impl Default for Stats {
    /// Average in everything.
    fn default() -> Self {
        Self { strength: 10, dexterity: 10, constitution: 10, intelligence: 10, wisdom: 10 }
    }
}

impl Stats {
    /// One stat's score.
    pub fn score(&self, stat: Stat) -> i32 {
        match stat {
            Stat::Strength => self.strength,
            Stat::Dexterity => self.dexterity,
            Stat::Constitution => self.constitution,
            Stat::Intelligence => self.intelligence,
            Stat::Wisdom => self.wisdom,
        }
    }

    /// One stat's score, to change.
    pub fn score_mut(&mut self, stat: Stat) -> &mut i32 {
        match stat {
            Stat::Strength => &mut self.strength,
            Stat::Dexterity => &mut self.dexterity,
            Stat::Constitution => &mut self.constitution,
            Stat::Intelligence => &mut self.intelligence,
            Stat::Wisdom => &mut self.wisdom,
        }
    }

    /// One stat's modifier.
    pub fn modifier(&self, stat: Stat) -> i32 {
        modifier(self.score(stat))
    }

    /// Whether every score is within [`MIN_SCORE`] and [`MAX_SCORE`].
    pub fn is_valid(&self) -> bool {
        Stat::ALL.iter().all(|&stat| (MIN_SCORE..=MAX_SCORE).contains(&self.score(stat)))
    }
}

/// The modifier a stat gives: `floor((score − 10) / 2)` (SRD 5.1).
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
        let mut stats = Stats::default();
        *stats.score_mut(Stat::Wisdom) += 4;
        assert_eq!(stats.score(Stat::Wisdom), 14);
        assert_eq!(stats.modifier(Stat::Wisdom), 2);
        assert!(stats.is_valid());
        stats.strength = 0;
        assert!(!stats.is_valid());
    }
}
