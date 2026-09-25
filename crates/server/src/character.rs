//! Characters: each player's class, level, experience, stats and points.
//!
//! A [`Character`] changes only through its own methods, which keep it
//! consistent with the rules in `venture_rpg` (points come with levels, stats
//! stay in range). Changed characters are sent to their players as their
//! character sheet automatically.

use bevy::prelude::*;
use venture_content::{ClassDef, ContentPack};
use venture_protocol::{BodyType, CharacterSheet, Outfit, PlayerLook, ServerMessage};
use venture_rpg::progression::{experience_to_next_level, total_skill_points, total_stat_points};
use venture_rpg::{Abilities, Ability, ClassRules, DerivedStats, Gear, LevelUp, MAX_LEVEL, Progress};

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::Player;
use crate::link::Link;

/// Sends players their character sheets when they change.
pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, send_changed_sheets.in_set(ServerSystems::Updates));
    }
}

/// A player's character: everything the RPG rules need to know about them.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Character {
    /// Their class (a class id).
    pub class: String,
    /// Their build (only how they look).
    pub body: BodyType,
    /// Their level and experience.
    pub progress: Progress,
    /// Their ability scores (their class's starting scores plus stat points spent).
    pub stats: Abilities,
    /// Stat points to spend.
    pub stat_points: u32,
    /// Skill points to spend.
    pub skill_points: u32,
}

/// Players with admin powers (see [`crate::admin`]).
#[derive(Component)]
pub struct Admin;

impl Character {
    /// A new level 1 character of a class.
    pub fn new(class: &ClassDef, body: BodyType) -> Self {
        Self {
            class: class.id.clone(),
            body,
            progress: Progress::default(),
            stats: class.stats,
            stat_points: 0,
            skill_points: 0,
        }
    }

    /// Their level.
    pub fn level(&self) -> u32 {
        self.progress.level
    }

    /// Adds experience, with the points any new levels bring.
    pub fn add_experience(&mut self, amount: u64) -> LevelUp {
        let gained = self.progress.add_experience(amount);
        self.stat_points += gained.stat_points;
        self.skill_points += gained.skill_points;
        gained
    }

    /// Sets their level (from 1 to the top). Going up keeps what they've
    /// spent and adds the points the new levels bring; going down starts
    /// their stats over, with the points their new level has earned to spend.
    pub fn set_level(&mut self, level: u32, class: &ClassDef) {
        let level = level.clamp(1, MAX_LEVEL);
        let old = self.progress.level;
        if level >= old {
            self.stat_points += total_stat_points(level) - total_stat_points(old);
            self.skill_points += total_skill_points(level) - total_skill_points(old);
            self.progress = Progress { level, experience: 0 };
        } else {
            self.progress = Progress { level, experience: 0 };
            self.refund(class);
        }
    }

    /// Starts over: level 1 of their class, with its starting stats.
    pub fn reset(&mut self, class: &ClassDef) {
        *self = Self::new(class, self.body);
    }

    /// How they look: their build, in their class's outfit.
    pub fn look(&self, class: &ClassDef) -> PlayerLook {
        PlayerLook { body: self.body, outfit: class.outfit }
    }

    /// Changes class, keeping their level: stats start over at the new
    /// class's, with every stat and skill point their level has earned to spend.
    pub fn change_class(&mut self, class: &ClassDef) {
        self.class = class.id.clone();
        self.refund(class);
    }

    /// Puts stats back to the class's starting scores, and gives back every
    /// point earned so far.
    fn refund(&mut self, class: &ClassDef) {
        self.stats = class.stats;
        self.stat_points = total_stat_points(self.progress.level);
        self.skill_points = total_skill_points(self.progress.level);
    }

    /// Their secondary stats. (Equipment comes later: for now, bare hands.)
    pub fn derived(&self, class: &ClassDef) -> DerivedStats {
        DerivedStats::of(&class.rules, self.level(), &self.stats, &Gear::default())
    }

    /// Their character sheet, as the player sees it.
    pub fn sheet(&self, class: &ClassDef) -> CharacterSheet {
        let derived = self.derived(class);
        let experience_to_next = if self.level() >= MAX_LEVEL { 0 } else { experience_to_next_level(self.level()) };
        CharacterSheet {
            class: class.name.clone(),
            level: self.level(),
            experience: self.progress.experience,
            experience_to_next,
            stats: self.stats,
            stat_points: self.stat_points,
            skill_points: self.skill_points,
            max_health: derived.max_health,
            max_mana: derived.max_mana,
            armour_class: derived.armour_class,
            attack_bonus: derived.attack_bonus,
            damage_bonus: derived.damage_bonus,
            attacks_per_second: derived.attacks_per_second,
        }
    }
}

/// A class by id, falling back to the pack's first class, or (for a pack
/// with no classes at all) a plain built-in one, so the game always works.
pub fn class_or_default(pack: &ContentPack, id: Option<&str>) -> ClassDef {
    id.and_then(|id| pack.classes.iter().find(|class| class.id == id))
        .or_else(|| pack.classes.first())
        .cloned()
        .unwrap_or_else(|| ClassDef {
            id: "adventurer".to_string(),
            name: "Adventurer".to_string(),
            description: "Someone setting out to see the world.".to_string(),
            rules: ClassRules { hit_die: 8, mana_per_level: 4, casting_ability: Ability::Intelligence },
            stats: Abilities::default(),
            outfit: Outfit::default(),
        })
}

/// A class by its id or name, ignoring case (for `/setclass`).
pub fn find_class<'a>(pack: &'a ContentPack, id_or_name: &str) -> Option<&'a ClassDef> {
    pack.classes
        .iter()
        .find(|class| class.id.eq_ignore_ascii_case(id_or_name) || class.name.eq_ignore_ascii_case(id_or_name))
}

fn send_changed_sheets(
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    players: Query<(&Player, &Character), Changed<Character>>,
) {
    for (player, character) in &players {
        let class = class_or_default(&content.pack, Some(&character.class));
        link.send(player.connection, &ServerMessage::Character(character.sheet(&class)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fighter() -> ClassDef {
        ClassDef {
            id: "fighter".to_string(),
            name: "Fighter".to_string(),
            description: String::new(),
            rules: ClassRules { hit_die: 10, mana_per_level: 3, casting_ability: Ability::Wisdom },
            stats: Abilities { strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10 },
            outfit: Outfit::default(),
        }
    }

    fn mage() -> ClassDef {
        ClassDef {
            id: "mage".to_string(),
            name: "Mage".to_string(),
            stats: Abilities { strength: 8, dexterity: 12, constitution: 12, intelligence: 15, wisdom: 13 },
            ..fighter()
        }
    }

    #[test]
    fn levelling_up_brings_points_and_keeps_what_was_spent() {
        let mut character = Character::new(&fighter(), BodyType::Feminine);
        character.stats.strength += 2; // as if they'd spent points
        character.set_level(5, &fighter());
        assert_eq!(character.level(), 5);
        assert_eq!((character.stat_points, character.skill_points), (8, 5));
        assert_eq!(character.stats.strength, 17);
    }

    #[test]
    fn going_down_a_level_starts_stats_over() {
        let mut character = Character::new(&fighter(), BodyType::Feminine);
        character.set_level(10, &fighter());
        character.stats.strength += 4;
        character.stat_points -= 4;
        character.set_level(3, &fighter());
        assert_eq!(character.stats, fighter().stats);
        assert_eq!((character.stat_points, character.skill_points), (4, 2));
    }

    #[test]
    fn changing_class_refunds_everything_and_keeps_the_level() {
        let mut character = Character::new(&fighter(), BodyType::Feminine);
        character.set_level(6, &fighter());
        character.stats.strength += 10;
        character.change_class(&mage());
        assert_eq!(character.class, "mage");
        assert_eq!(character.level(), 6);
        assert_eq!(character.stats, mage().stats);
        // Levels 2 to 6: 2 stat points each; a skill point each, and a bonus one at 5.
        assert_eq!((character.stat_points, character.skill_points), (10, 6));
    }

    #[test]
    fn resetting_starts_at_level_one() {
        let mut character = Character::new(&fighter(), BodyType::Feminine);
        character.set_level(20, &fighter());
        character.reset(&fighter());
        assert_eq!(character, Character::new(&fighter(), BodyType::Feminine));
    }

    #[test]
    fn the_sheet_shows_the_rules_numbers() {
        let sheet = Character::new(&fighter(), BodyType::Feminine).sheet(&fighter());
        assert_eq!((sheet.level, sheet.experience_to_next), (1, 100));
        assert_eq!(sheet.max_health, 12, "d10 + CON 14 (+2)");
        assert_eq!(sheet.attack_bonus, 4, "+2 proficiency, +2 STR");
    }
}
