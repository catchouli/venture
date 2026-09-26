//! Experience and levels, and what each new level gives.
//!
//! A character's level is also their **combat level**, shown to others. The
//! experience curve and the points per level are our own (the SRD's are made
//! for a tabletop game that levels slowly); proficiency follows the SRD.

use serde::{Deserialize, Serialize};

/// The highest level a character can reach.
pub const MAX_LEVEL: u32 = 60;
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
    /// Adds experience, going up as many levels as it's enough for. At the
    /// top level, experience stops counting.
    pub fn add_experience(&mut self, amount: u64) -> LevelUp {
        let mut gained = LevelUp::default();
        if self.level >= MAX_LEVEL {
            return gained;
        }
        self.experience = self.experience.saturating_add(amount);
        while self.level < MAX_LEVEL && self.experience >= experience_to_next_level(self.level) {
            self.experience -= experience_to_next_level(self.level);
            self.level += 1;
            gained.levels += 1;
            gained.stat_points += stat_points_at(self.level);
            gained.ability_points += ability_points_at(self.level);
        }
        if self.level >= MAX_LEVEL {
            self.experience = 0;
        }
        gained
    }

    /// Experience still needed to reach the next level (0 at the top).
    pub fn experience_needed(&self) -> u64 {
        if self.level >= MAX_LEVEL { 0 } else { experience_to_next_level(self.level) - self.experience }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
