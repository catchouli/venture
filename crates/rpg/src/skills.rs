//! Using skills (our own rules, built on the SRD 5.1 attack roll): a weapon
//! skill (such as Whack) is a weapon attack with the skill's own damage dice;
//! a spell (such as Air Blast) rolls with the spell attack bonus, and the
//! casting ability adds to its damage. Healing adds the casting ability too.
//! Resting speeds up recovery, and mana comes back on its own, even in a fight.

use crate::derived::DerivedStats;
use crate::dice::Dice;

/// How often health and mana come back while resting, in seconds (instead of
/// [`crate::derived::REGENERATION_SECONDS`]).
pub const REST_REGENERATION_SECONDS: f32 = 1.0;

/// What powers a skill's attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    /// The weapon: its attack bonus and ability (Whack, Cleave).
    Weapon,
    /// Magic: the spell attack bonus and the casting ability (Air Blast).
    Spell,
}

/// Mana that comes back every [`crate::derived::REGENERATION_SECONDS`]
/// (every [`REST_REGENERATION_SECONDS`] while resting): a twentieth of the
/// most mana, and at least 1. Characters with no mana get none.
pub fn mana_regeneration(max_mana: u32) -> u32 {
    if max_mana == 0 { 0 } else { (max_mana / 20).max(1) }
}

impl DerivedStats {
    /// A skill's attack: the attack bonus to roll with, and its damage dice
    /// with the ability's bonus added.
    pub fn skill_attack(&self, damage: Dice, power: Power) -> (i32, Dice) {
        match power {
            Power::Weapon => (self.attack_bonus, Dice { bonus: damage.bonus + self.damage_bonus, ..damage }),
            Power::Spell => (self.spell_attack_bonus, Dice { bonus: damage.bonus + self.spell_bonus, ..damage }),
        }
    }

    /// How much a healing skill heals: its dice plus the casting ability's
    /// modifier, and always at least 1.
    pub fn healing(&self, amount: Dice, roll_die: &mut impl FnMut(u32) -> u32) -> u32 {
        Dice { bonus: amount.bonus + self.spell_bonus, ..amount }.roll(roll_die).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abilities::{Abilities, Ability};
    use crate::class::ClassRules;
    use crate::derived::Gear;

    fn mage() -> DerivedStats {
        let rules = ClassRules { hit_die: 6, mana_per_level: 8, casting_ability: Ability::Intelligence };
        let abilities = Abilities { strength: 8, dexterity: 12, constitution: 12, intelligence: 16, wisdom: 12 };
        DerivedStats::of(&rules, 1, &abilities, &Gear::default())
    }

    #[test]
    fn spells_use_the_casting_ability_and_weapon_skills_the_weapon() {
        let stats = mage();
        let (bonus, damage) = stats.skill_attack(Dice::new(1, 6), Power::Spell);
        assert_eq!((bonus, damage.to_string()), (5, "1d6+3".to_string()), "+2 proficiency, +3 INT");
        let (bonus, damage) = stats.skill_attack(Dice::new(1, 8), Power::Weapon);
        assert_eq!((bonus, damage.to_string()), (1, "1d8-1".to_string()), "+2 proficiency, -1 STR");
    }

    #[test]
    fn healing_adds_the_casting_ability_and_always_heals_a_little() {
        let stats = mage();
        assert_eq!(stats.healing(Dice::new(2, 6), &mut |_| 4), 11);
        let mut weak = stats;
        weak.spell_bonus = -5;
        assert_eq!(weak.healing(Dice::new(1, 4), &mut |_| 1), 1);
    }

    #[test]
    fn mana_comes_back_slowly_unless_there_is_none() {
        assert_eq!(mana_regeneration(0), 0);
        assert_eq!(mana_regeneration(11), 1);
        assert_eq!(mana_regeneration(60), 3);
    }
}
