//! Secondary stats: the numbers worked out from a character's class, level,
//! abilities and equipment. Shown on the stats tab, and used in fights.

use crate::abilities::{Abilities, Ability};
use crate::class::ClassRules;
use crate::dice::Dice;
use crate::progression::proficiency_bonus;

/// Armour class with no armour and an average Dexterity (SRD 5.1).
const BASE_ARMOUR_CLASS: i32 = 10;
/// Each point of Dexterity modifier speeds attacks up by this fraction.
const ATTACK_SPEED_PER_DEXTERITY: f32 = 0.05;
/// The fastest anyone attacks, in attacks per second.
const MAX_ATTACKS_PER_SECOND: f32 = 3.0;
/// The slowest anyone attacks, in attacks per second.
const MIN_ATTACKS_PER_SECOND: f32 = 0.25;

/// A character's secondary stats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DerivedStats {
    /// Most health.
    pub max_health: u32,
    /// Most mana.
    pub max_mana: u32,
    /// How hard they are to hit: attacks hit if the roll (d20 + attack bonus)
    /// is at least this.
    pub armour_class: i32,
    /// Added to attack rolls with their weapon.
    pub attack_bonus: i32,
    /// Added to their weapon's damage.
    pub damage_bonus: i32,
    /// Their weapon's damage, bonus included (such as `1d6+2`).
    pub damage: Dice,
    /// Attacks per second with their weapon.
    pub attacks_per_second: f32,
}

/// What a character's equipment adds (the weapon's and armour's parts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gear {
    /// Armour class from armour.
    pub armour: i32,
    /// The ability the weapon attacks with (Strength for swords and staves).
    pub weapon_ability: Ability,
    /// The weapon's attacks per second, before Dexterity.
    pub weapon_speed: f32,
    /// The weapon's damage dice, before the ability's bonus.
    pub weapon_damage: Dice,
}

/// Damage with bare fists (SRD 5.1: an unarmed strike does 1, plus Strength).
pub const UNARMED_DAMAGE: Dice = Dice { count: 0, sides: 1, bonus: 1 };

impl Default for Gear {
    /// Nothing worn, fighting with fists.
    fn default() -> Self {
        Self { armour: 0, weapon_ability: Ability::Strength, weapon_speed: 1.0, weapon_damage: UNARMED_DAMAGE }
    }
}

impl DerivedStats {
    /// Works out the secondary stats.
    pub fn of(class: &ClassRules, level: u32, abilities: &Abilities, gear: &Gear) -> Self {
        let level = level.max(1);
        let weapon_modifier = abilities.modifier(gear.weapon_ability);
        Self {
            max_health: max_health(class, level, abilities.modifier(Ability::Constitution)),
            max_mana: max_mana(class, level, abilities.modifier(class.casting_ability)),
            armour_class: BASE_ARMOUR_CLASS + abilities.modifier(Ability::Dexterity) + gear.armour,
            attack_bonus: proficiency_bonus(level) + weapon_modifier,
            damage_bonus: weapon_modifier,
            damage: Dice { bonus: gear.weapon_damage.bonus + weapon_modifier, ..gear.weapon_damage },
            attacks_per_second: attacks_per_second(gear.weapon_speed, abilities.modifier(Ability::Dexterity)),
        }
    }
}

/// Most health (SRD 5.1): the whole hit die plus the Constitution modifier at
/// level 1, then the hit die's fixed value plus the modifier each level after.
/// Every level adds at least 1, however low the Constitution.
pub fn max_health(class: &ClassRules, level: u32, constitution_modifier: i32) -> u32 {
    let first = (class.hit_die as i32 + constitution_modifier).max(1);
    let later = (class.health_per_level() as i32 + constitution_modifier).max(1);
    (first + later * (level.max(1) as i32 - 1)) as u32
}

/// Most mana (our own): each level gives the class's mana per level plus the
/// casting ability's modifier (never less than nothing).
pub fn max_mana(class: &ClassRules, level: u32, casting_modifier: i32) -> u32 {
    (class.mana_per_level as i32 + casting_modifier).max(0) as u32 * level.max(1)
}

/// Attacks per second (our own): the weapon's speed, 5% faster per point of
/// Dexterity modifier (slower for negative ones), within limits.
pub fn attacks_per_second(weapon_speed: f32, dexterity_modifier: i32) -> f32 {
    let speed = weapon_speed * (1.0 + ATTACK_SPEED_PER_DEXTERITY * dexterity_modifier as f32);
    speed.clamp(MIN_ATTACKS_PER_SECOND, MAX_ATTACKS_PER_SECOND)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fighter() -> ClassRules {
        ClassRules { hit_die: 10, mana_per_level: 3, casting_ability: Ability::Wisdom }
    }

    #[test]
    fn health_grows_with_level_and_constitution() {
        // A level 1 fighter with CON 14 (+2): 10 + 2.
        assert_eq!(max_health(&fighter(), 1, 2), 12);
        // Level 3: 12 + 2 × (6 + 2).
        assert_eq!(max_health(&fighter(), 3, 2), 28);
        // Terrible Constitution still gives a point a level.
        assert_eq!(max_health(&fighter(), 3, -8), 4);
    }

    #[test]
    fn mana_comes_from_the_class_and_casting_ability() {
        assert_eq!(max_mana(&fighter(), 1, 0), 3);
        assert_eq!(max_mana(&fighter(), 4, 1), 16);
        assert_eq!(max_mana(&fighter(), 4, -5), 0);
    }

    #[test]
    fn dexterity_speeds_attacks_up_within_limits() {
        assert_eq!(attacks_per_second(1.0, 0), 1.0);
        assert!((attacks_per_second(1.0, 2) - 1.1).abs() < 1e-6);
        assert_eq!(attacks_per_second(10.0, 5), MAX_ATTACKS_PER_SECOND);
    }

    #[test]
    fn a_starting_fighter_adds_up() {
        let abilities = Abilities { strength: 14, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10 };
        let gear =
            Gear { armour: 1, weapon_ability: Ability::Strength, weapon_speed: 1.0, weapon_damage: Dice::new(1, 6) };
        let stats = DerivedStats::of(&fighter(), 1, &abilities, &gear);
        assert_eq!(stats.max_health, 12);
        assert_eq!(stats.armour_class, 12, "10 + 1 (DEX) + 1 (armour)");
        assert_eq!(stats.attack_bonus, 4, "+2 proficiency, +2 STR");
        assert_eq!(stats.damage_bonus, 2);
        assert_eq!(stats.damage.to_string(), "1d6+2");
        let unarmed = DerivedStats::of(&fighter(), 1, &abilities, &Gear::default());
        assert_eq!(unarmed.damage.to_string(), "3", "1 plus Strength's +2");
    }
}
