//! What using an item (eating, drinking) does to the numbers.

#[cfg(feature = "reflect")]
use bevy_reflect::prelude::ReflectDefault;
use serde::{Deserialize, Serialize};

/// How much something heals: a flat amount, or a share of the most health.
///
/// ```ron
/// Flat(5)       // 5 health
/// Percent(20.0) // a fifth of the most health
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect), reflect(Default))]
pub enum Healing {
    /// This much health.
    Flat(u32),
    /// This percentage of the most health (rounded, and at least 1).
    Percent(f32),
}

impl Default for Healing {
    fn default() -> Self {
        Healing::Flat(5)
    }
}

impl Healing {
    /// How much health it gives someone with `max_health` at most.
    pub fn amount(self, max_health: u32) -> u32 {
        match self {
            Healing::Flat(amount) => amount,
            Healing::Percent(percent) => {
                let share = (max_health as f32 * percent.clamp(0.0, 100.0) / 100.0).round() as u32;
                if percent > 0.0 { share.max(1) } else { 0 }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_healing_is_the_same_for_everyone() {
        assert_eq!(Healing::Flat(5).amount(10), 5);
        assert_eq!(Healing::Flat(5).amount(200), 5);
    }

    #[test]
    fn percent_healing_is_a_share_of_the_most_health() {
        assert_eq!(Healing::Percent(20.0).amount(50), 10);
        assert_eq!(Healing::Percent(25.0).amount(10), 3);
        assert_eq!(Healing::Percent(1.0).amount(10), 1, "always at least 1");
        assert_eq!(Healing::Percent(0.0).amount(10), 0);
        assert_eq!(Healing::Percent(500.0).amount(10), 10, "never more than all of it");
    }
}
