//! The RPG rules: every number behind a character and a fight, in one place.
//!
//! - [`abilities`]: the five stats (STR, DEX, CON, INT, WIS) and their modifiers.
//! - [`dice`]: dice like `1d6+2`, and rolling them.
//! - [`progression`]: experience, levels, and the points each level gives.
//! - [`class`]: what a class contributes to the numbers (its hit die, its mana).
//! - [`derived`]: the secondary stats worked out from those (health, mana,
//!   armour class, attack bonus, attack speed).
//! - [`combat`]: resolving an attack: hit or miss, damage, critical hits.
//!
//! The server uses these to decide what happens; the client uses them to show
//! the same numbers. The crate has no engine or networking code, and every
//! rule is a small function with tests, so tuning the game means changing one
//! function (or one number in the content pack).
//!
//! # Where the rules come from
//!
//! The core mechanics (ability modifiers, hit dice, proficiency, attack rolls
//! against armour class, damage dice, critical hits) follow the System
//! Reference Document 5.1, adapted for a real-time game. The rest (attack
//! speed, mana, the experience curve, points per level) is our own. See
//! "Credits" in the README.
//!
//! This work includes material taken from the System Reference Document 5.1
//! ("SRD 5.1") by Wizards of the Coast LLC and available at
//! <https://dnd.wizards.com/resources/systems-reference-document>. The SRD 5.1
//! is licensed under the Creative Commons Attribution 4.0 International
//! License available at <https://creativecommons.org/licenses/by/4.0/legalcode>.

pub mod abilities;
pub mod class;
pub mod combat;
pub mod derived;
pub mod dice;
pub mod progression;

pub use abilities::{Abilities, Ability, modifier};
pub use class::ClassRules;
pub use combat::{AttackOutcome, hit_chance, resolve_attack};
pub use derived::{DerivedStats, Gear};
pub use dice::Dice;
pub use progression::{LevelUp, MAX_LEVEL, Progress};
