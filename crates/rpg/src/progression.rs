//! Experience and levels, and what each new level gives.
//!
//! A character's level is also their **combat level**, shown to others. The
//! experience curve and the points per level are our own (the SRD's are made
//! for a tabletop game that levels slowly); proficiency follows the SRD.

use serde::{Deserialize, Serialize};

/// The highest level a character can reach.
pub const MAX_LEVEL: u32 = 60;
/// The highest level a skill (such as fishing) can reach.
pub const MAX_SKILL_LEVEL: u32 = 99;
/// Stat points gained at each new level.
const STAT_POINTS_PER_LEVEL: u32 = 2;
/// Ability points gained at each new level...
const ABILITY_POINTS_PER_LEVEL: u32 = 1;
/// ...plus one more every this many levels.
const BONUS_ABILITY_POINT_EVERY: u32 = 5;

/// Experience needed to go from `level` to the next: `round(100 × level^1.5)`.
/// So 100 for level 1 to 2, 283 for 2 to 3, 3162 for 10 to 11.
pub fn experience_to_next_level(level: u32) -> u64 {
    (100.0 * f64::from(level.max(1)).powf(1.5)).round() as u64
}

/// Experience shared equally among `sharers` (a party at a kill): each
/// share, in order. What doesn't divide evenly goes one point each to the
/// first sharers (the killer comes first), so the shares always add up to
/// `total`. No sharers, no shares.
pub fn split_experience(total: u64, sharers: usize) -> Vec<u64> {
    let Some(count) = u64::try_from(sharers).ok().filter(|&count| count > 0) else { return Vec::new() };
    let (each, left_over) = (total / count, total % count);
    (0..count).map(|index| each + u64::from(index < left_over)).collect()
}

/// Stat points gained on reaching `level`.
pub fn stat_points_at(level: u32) -> u32 {
    if level <= 1 { 0 } else { STAT_POINTS_PER_LEVEL }
}

/// Ability points gained on reaching `level`.
pub fn ability_points_at(level: u32) -> u32 {
    match level {
        0 | 1 => 0,
        level if level % BONUS_ABILITY_POINT_EVERY == 0 => ABILITY_POINTS_PER_LEVEL + 1,
        _ => ABILITY_POINTS_PER_LEVEL,
    }
}

/// All the stat points a character has earned by reaching `level`.
pub fn total_stat_points(level: u32) -> u32 {
    (2..=level.min(MAX_LEVEL)).map(stat_points_at).sum()
}

/// All the ability points a character has earned by reaching `level`.
pub fn total_ability_points(level: u32) -> u32 {
    (2..=level.min(MAX_LEVEL)).map(ability_points_at).sum()
}

/// The proficiency bonus at a level (SRD 5.1): +2 at levels 1 to 4, +3 at 5
/// to 8, and so on, one more every four levels.
pub fn proficiency_bonus(level: u32) -> i32 {
    2 + (level.max(1) as i32 - 1) / 4
}

/// A character's level, and their experience towards the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Their level, from 1 to [`MAX_LEVEL`].
    pub level: u32,
    /// Experience earned since reaching it.
    pub experience: u64,
}

/// What reaching new levels gave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelUp {
    /// How many levels were gained.
    pub levels: u32,
    /// Stat points gained.
    pub stat_points: u32,
    /// Ability points gained.
    pub ability_points: u32,
}

impl Default for Progress {
    fn default() -> Self {
        Self { level: 1, experience: 0 }
    }
}

impl Progress {
    /// Adds a character's experience, going up as many levels as it's enough
    /// for. At the top level ([`MAX_LEVEL`]), experience stops counting.
    pub fn add_experience(&mut self, amount: u64) -> LevelUp {
        self.add_experience_up_to(amount, MAX_LEVEL)
    }

    /// Adds experience to a skill, which goes on to [`MAX_SKILL_LEVEL`].
    /// (Skills give no points: only the levels count.)
    pub fn add_skill_experience(&mut self, amount: u64) -> u32 {
        self.add_experience_up_to(amount, MAX_SKILL_LEVEL).levels
    }

    fn add_experience_up_to(&mut self, amount: u64, top: u32) -> LevelUp {
        let mut gained = LevelUp::default();
        if self.level >= top {
            return gained;
        }
        self.experience = self.experience.saturating_add(amount);
        while self.level < top && self.experience >= experience_to_next_level(self.level) {
            self.experience -= experience_to_next_level(self.level);
            self.level += 1;
            gained.levels += 1;
            gained.stat_points += stat_points_at(self.level);
            gained.ability_points += ability_points_at(self.level);
        }
        if self.level >= top {
            self.experience = 0;
        }
        gained
    }

    /// All the experience it took to get here: every level so far, plus
    /// what's been earned towards the next.
    pub fn total_experience(&self) -> u64 {
        (1..self.level).map(experience_to_next_level).sum::<u64>() + self.experience
    }

    /// Experience still needed to reach a character's next level (0 at the top).
    pub fn experience_needed(&self) -> u64 {
        self.experience_needed_below(MAX_LEVEL)
    }

    /// Experience still needed to reach a skill's next level (0 at the top).
    pub fn skill_experience_needed(&self) -> u64 {
        self.experience_needed_below(MAX_SKILL_LEVEL)
    }

    fn experience_needed_below(&self, top: u32) -> u64 {
        if self.level >= top { 0 } else { experience_to_next_level(self.level) - self.experience }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experience_is_split_equally_and_none_is_lost() {
        assert_eq!(split_experience(30, 1), [30]);
        assert_eq!(split_experience(30, 3), [10, 10, 10]);
        // The killer (first) gets the odd points.
        assert_eq!(split_experience(10, 3), [4, 3, 3]);
        assert_eq!(split_experience(2, 4), [1, 1, 0, 0]);
        assert!(split_experience(10, 0).is_empty());
        for (total, sharers) in [(0, 2), (7, 8), (1_000_003, 6)] {
            assert_eq!(split_experience(total, sharers).iter().sum::<u64>(), total);
        }
    }

    #[test]
    fn skills_go_past_the_character_level_cap() {
        let mut character = Progress { level: MAX_LEVEL, experience: 0 };
        assert_eq!(character.add_experience(1_000_000), LevelUp::default());
        assert_eq!(character.experience_needed(), 0);

        let mut skill = Progress { level: MAX_LEVEL, experience: 0 };
        assert_eq!(skill.add_skill_experience(experience_to_next_level(MAX_LEVEL)), 1);
        assert_eq!(skill.level, MAX_LEVEL + 1);
        assert!(skill.skill_experience_needed() > 0);
        skill.add_skill_experience(u64::MAX);
        assert_eq!((skill.level, skill.experience, skill.skill_experience_needed()), (MAX_SKILL_LEVEL, 0, 0));
    }

    #[test]
    fn total_experience_counts_every_level_so_far() {
        assert_eq!(Progress { level: 1, experience: 25 }.total_experience(), 25);
        let level_3 = Progress { level: 3, experience: 10 };
        assert_eq!(level_3.total_experience(), experience_to_next_level(1) + experience_to_next_level(2) + 10);
    }

    #[test]
    fn the_curve_starts_quick_and_grows() {
        assert_eq!(experience_to_next_level(1), 100);
        assert_eq!(experience_to_next_level(2), 283);
        assert_eq!(experience_to_next_level(10), 3162);
        assert!((1..MAX_LEVEL).all(|level| experience_to_next_level(level + 1) > experience_to_next_level(level)));
    }

    #[test]
    fn points_come_with_each_level() {
        assert_eq!((2..=5).map(ability_points_at).collect::<Vec<_>>(), [1, 1, 1, 2]);
        assert_eq!(stat_points_at(2), 2);
        assert_eq!(stat_points_at(1), 0, "nothing for the level you start at");
    }

    #[test]
    fn totals_add_up_every_level() {
        assert_eq!(total_stat_points(1), 0);
        assert_eq!(total_stat_points(10), 18);
        assert_eq!(total_ability_points(10), 11, "9 levels, 2 of them bonus levels");
    }

    #[test]
    fn proficiency_follows_the_srd() {
        let table = [(1, 2), (4, 2), (5, 3), (8, 3), (9, 4), (17, 6), (20, 6)];
        for (level, bonus) in table {
            assert_eq!(proficiency_bonus(level), bonus, "level {level}");
        }
    }

    #[test]
    fn experience_can_carry_over_several_levels() {
        let mut progress = Progress::default();
        let gained = progress.add_experience(100 + 283 + 10);
        assert_eq!(gained, LevelUp { levels: 2, stat_points: 4, ability_points: 2 });
        assert_eq!(progress, Progress { level: 3, experience: 10 });
        assert_eq!(progress.experience_needed(), experience_to_next_level(3) - 10);
    }

    #[test]
    fn the_top_level_is_the_limit() {
        let mut progress = Progress { level: MAX_LEVEL - 1, experience: 0 };
        progress.add_experience(u64::MAX);
        assert_eq!(progress, Progress { level: MAX_LEVEL, experience: 0 });
        assert_eq!(progress.add_experience(1000), LevelUp::default());
    }
}
