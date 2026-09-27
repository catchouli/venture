//! Resolving one attack (SRD 5.1): roll a d20 and add the attack bonus; it
//! hits if the total reaches the target's armour class. A natural 20 always
//! hits and is a critical hit (the damage dice are rolled twice); a natural 1
//! always misses.

use crate::dice::Dice;

/// How one attack went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackOutcome {
    /// It missed.
    Miss,
    /// It hit, for this much damage.
    Hit(u32),
    /// A critical hit, for this much damage.
    Critical(u32),
}

impl AttackOutcome {
    /// The damage done (0 for a miss).
    pub fn damage(&self) -> u32 {
        match *self {
            AttackOutcome::Miss => 0,
            AttackOutcome::Hit(damage) | AttackOutcome::Critical(damage) => damage,
        }
    }
}

/// Resolves an attack. `roll_die(sides)` rolls one die (1 to `sides`): the
/// server passes real randomness, tests pass fixed rolls. Every hit does at
/// least 1 damage.
pub fn resolve_attack(
    attack_bonus: i32,
    armour_class: i32,
    damage: Dice,
    damage_bonus: i32,
    roll_die: &mut impl FnMut(u32) -> u32,
) -> AttackOutcome {
    let natural = roll_die(20).clamp(1, 20);
    let with_bonus = |dice: Dice| Dice { bonus: dice.bonus + damage_bonus, ..dice };
    match natural {
        1 => AttackOutcome::Miss,
        20 => AttackOutcome::Critical(with_bonus(damage.doubled()).roll(roll_die).max(1)),
        _ if natural as i32 + attack_bonus >= armour_class => {
            AttackOutcome::Hit(with_bonus(damage).roll(roll_die).max(1))
        }
        _ => AttackOutcome::Miss,
    }
}

/// The chance an attack hits (including critical hits), from 0 to 1: for
/// showing on the stats tab, and for tuning monsters.
pub fn hit_chance(attack_bonus: i32, armour_class: i32) -> f32 {
    // Natural 2 to 19 hit if they reach the armour class; 20 always does.
    let hitting_rolls = (2..=19).filter(|natural| natural + attack_bonus >= armour_class).count() + 1;
    hitting_rolls as f32 / 20.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A die roller that returns these rolls in turn.
    fn rolls(values: &[u32]) -> impl FnMut(u32) -> u32 + '_ {
        let mut next = values.iter();
        move |_| *next.next().expect("enough rolls")
    }

    #[test]
    fn attacks_hit_when_they_reach_the_armour_class() {
        let sword = Dice::new(1, 6);
        // 10 + 4 = 14 against AC 14: a hit for 3 + 2.
        assert_eq!(resolve_attack(4, 14, sword, 2, &mut rolls(&[10, 3])), AttackOutcome::Hit(5));
        // 9 + 4 = 13: a miss.
        assert_eq!(resolve_attack(4, 14, sword, 2, &mut rolls(&[9])), AttackOutcome::Miss);
    }

    #[test]
    fn natural_twenties_and_ones_ignore_the_numbers() {
        let sword = Dice::new(1, 6);
        // A critical hit rolls the die twice: 4 + 5 + 2.
        assert_eq!(resolve_attack(0, 30, sword, 2, &mut rolls(&[20, 4, 5])), AttackOutcome::Critical(11));
        assert_eq!(resolve_attack(50, 5, sword, 2, &mut rolls(&[1])), AttackOutcome::Miss);
    }

    #[test]
    fn hits_always_hurt_a_little() {
        let feeble = Dice::new(1, 4);
        assert_eq!(resolve_attack(5, 10, feeble, -3, &mut rolls(&[15, 1])), AttackOutcome::Hit(1));
    }

    #[test]
    fn hit_chances_add_up() {
        assert_eq!(hit_chance(4, 14), 0.55);
        assert_eq!(hit_chance(0, 40), 0.05, "only a natural 20");
        assert_eq!(hit_chance(40, 0), 0.95, "a natural 1 still misses");
    }
}
