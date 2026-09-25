//! Dice, written the usual way: `1d6` is one six-sided die, `2d4+1` is two
//! four-sided dice plus one.
//!
//! Rolling takes a function that rolls one die (returning 1 to `sides`), so
//! the server can use real randomness and tests can use fixed rolls.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A number of dice of one size, plus a fixed bonus (which may be negative).
/// In content files it's written as text: `"1d6"`, `"2d4+1"`, `"1d8-1"`, or
/// just a number like `"3"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Dice {
    /// How many dice.
    pub count: u32,
    /// How many sides each has.
    pub sides: u32,
    /// Added to the total.
    pub bonus: i32,
}

/// The most dice one roll may have, so content can't ask for absurd rolls.
const MAX_DICE: u32 = 100;
/// The most sides a die may have.
const MAX_SIDES: u32 = 1000;

impl Dice {
    /// `count` dice with `sides` sides each, and no bonus.
    pub const fn new(count: u32, sides: u32) -> Self {
        Self { count, sides, bonus: 0 }
    }

    /// Rolls them, using `roll_die(sides)` for each die (which must return 1
    /// to `sides`). Totals below zero count as zero.
    pub fn roll(&self, roll_die: &mut impl FnMut(u32) -> u32) -> u32 {
        let dice: u32 = (0..self.count).map(|_| roll_die(self.sides).clamp(1, self.sides.max(1))).sum();
        (dice as i32 + self.bonus).max(0) as u32
    }

    /// The same dice with twice as many dice (for critical hits; the bonus
    /// isn't doubled).
    pub fn doubled(&self) -> Self {
        Self { count: self.count * 2, ..*self }
    }

    /// The average total, rounded down (for showing damage ranges and tuning).
    pub fn average(&self) -> u32 {
        ((self.count * (self.sides + 1)) as i32 / 2 + self.bonus).max(0) as u32
    }

    /// The lowest and highest totals.
    pub fn range(&self) -> (u32, u32) {
        let low = (self.count as i32 + self.bonus).max(0) as u32;
        let high = ((self.count * self.sides) as i32 + self.bonus).max(0) as u32;
        (low, high)
    }
}

impl fmt::Display for Dice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.count, self.bonus) {
            (0, bonus) => write!(f, "{bonus}"),
            (count, 0) => write!(f, "{count}d{}", self.sides),
            (count, bonus) => write!(f, "{count}d{}{bonus:+}", self.sides),
        }
    }
}

impl FromStr for Dice {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let problem = || format!("\"{text}\" isn't dice (write it like \"1d6\", \"2d4+1\" or \"3\")");
        let Some((count, rest)) = text.split_once('d') else {
            let bonus = text.parse().map_err(|_| problem())?;
            return Ok(Self { count: 0, sides: 1, bonus });
        };
        let (sides, bonus) = match rest.find(['+', '-']) {
            Some(at) => (&rest[..at], rest[at..].trim_start_matches('+').parse().map_err(|_| problem())?),
            None => (rest, 0),
        };
        let count: u32 = if count.is_empty() { 1 } else { count.parse().map_err(|_| problem())? };
        let sides: u32 = sides.parse().map_err(|_| problem())?;
        if count > MAX_DICE || sides == 0 || sides > MAX_SIDES {
            return Err(format!("\"{text}\": at most {MAX_DICE} dice, with 1 to {MAX_SIDES} sides"));
        }
        Ok(Self { count, sides, bonus })
    }
}

impl TryFrom<String> for Dice {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl From<Dice> for String {
    fn from(dice: Dice) -> String {
        dice.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dice_are_read_and_written() {
        for text in ["1d6", "2d4+1", "1d8-1", "3", "d20"] {
            let dice: Dice = text.parse().unwrap();
            let expected = if text == "d20" { "1d20" } else { text };
            assert_eq!(dice.to_string(), expected);
        }
        assert!("1d0".parse::<Dice>().is_err());
        assert!("lots".parse::<Dice>().is_err());
        assert!("1000d6".parse::<Dice>().is_err());
    }

    #[test]
    fn rolls_add_up_and_never_go_below_zero() {
        let dice: Dice = "2d6+1".parse().unwrap();
        assert_eq!(dice.roll(&mut |_| 4), 9);
        assert_eq!(dice.range(), (3, 13));
        assert_eq!(dice.average(), 8);
        let weak: Dice = "1d4-3".parse().unwrap();
        assert_eq!(weak.roll(&mut |_| 1), 0);
        assert_eq!(dice.doubled().roll(&mut |_| 6), 25);
    }
}
