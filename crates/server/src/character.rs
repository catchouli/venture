//! Characters: each player's class, level, experience, stats, points and
//! abilities.
//!
//! A [`Character`] changes only through its own methods, which keep it
//! consistent with the rules in `venture_rpg` (points come with levels, stats
//! stay in range, abilities are learned only from the class, at their level,
//! for their cost). Players spend points through [`CharacterRequest`]s,
//! handled here. Changed characters (or equipment) are sent to their players
//! as their character sheet automatically.

use bevy::prelude::*;
use venture_content::{ClassDef, ContentPack};
use venture_protocol::{AbilityOffer, BodyType, CharacterSheet, ServerMessage};
use venture_rpg::progression::{experience_to_next_level, total_ability_points, total_stat_points};
use venture_rpg::stats::MAX_SCORE;
use venture_rpg::{ClassRules, DerivedStats, Gear, LevelUp, MAX_LEVEL, Progress, Stat, Stats};

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::Player;
use crate::equipment::Equipment;
use crate::link::Link;
use crate::requests::{CharacterAction, CharacterRequest};

/// Handles players spending points, and sends them their character sheets.
pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, spend_points.in_set(ServerSystems::Gameplay))
            .add_systems(FixedUpdate, send_changed_sheets.in_set(ServerSystems::Updates));
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
    /// Their stats (their class's starting scores plus stat points spent).
    pub stats: Stats,
    /// Stat points to spend.
    pub stat_points: u32,
    /// Ability points to spend.
    pub ability_points: u32,
    /// The abilities they know (ability ids), in the order learned.
    pub abilities: Vec<String>,
}

/// Players with admin powers (see [`crate::admin`]).
#[derive(Component)]
pub struct Admin;

impl Character {
    /// A new level 1 character of a class, knowing `common_abilities` (everyone's)
    /// and the class's starting abilities.
    pub fn new(class: &ClassDef, body: BodyType, common_abilities: &[String]) -> Self {
        Self {
            class: class.id.clone(),
            body,
            progress: Progress::default(),
            stats: class.stats,
            stat_points: 0,
            ability_points: 0,
            abilities: starting_abilities(class, common_abilities),
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
        self.ability_points += gained.ability_points;
        gained
    }

    /// Sets their level (from 1 to the top). Going up keeps what they've
    /// spent and adds the points the new levels bring; going down starts
    /// them over (stats and abilities), with the points their new level has
    /// earned to spend.
    pub fn set_level(&mut self, level: u32, class: &ClassDef, common_abilities: &[String]) {
        let level = level.clamp(1, MAX_LEVEL);
        let old = self.progress.level;
        if level >= old {
            self.stat_points += total_stat_points(level) - total_stat_points(old);
            self.ability_points += total_ability_points(level) - total_ability_points(old);
            self.progress = Progress { level, experience: 0 };
        } else {
            self.progress = Progress { level, experience: 0 };
            self.refund(class, common_abilities);
        }
    }

    /// Starts over: level 1 of their class, with its starting stats and abilities.
    pub fn reset(&mut self, class: &ClassDef, common_abilities: &[String]) {
        *self = Self::new(class, self.body, common_abilities);
    }

    /// Changes class, keeping their level: stats and abilities start over at the
    /// new class's, with every stat and ability point their level has earned to spend.
    pub fn change_class(&mut self, class: &ClassDef, common_abilities: &[String]) {
        self.class = class.id.clone();
        self.refund(class, common_abilities);
    }

    /// Spends a stat point on a stat.
    pub fn spend_stat_point(&mut self, stat: Stat) -> Result<(), String> {
        if self.stat_points == 0 {
            return Err("You have no stat points to spend.".to_string());
        }
        let score = self.stats.score_mut(stat);
        if *score >= MAX_SCORE {
            return Err(format!("Your {} can't go any higher.", stat.short_name()));
        }
        *score += 1;
        self.stat_points -= 1;
        Ok(())
    }

    /// Learns one of the class's abilities, if they're a high enough level and
    /// have the ability points.
    pub fn learn_ability(&mut self, class: &ClassDef, ability: &str) -> Result<(), String> {
        let Some(offer) = class.abilities.iter().find(|offer| offer.ability == ability) else {
            return Err(format!("{}s can't learn that ability.", class.name));
        };
        if self.abilities.iter().any(|known| known == ability) {
            return Err("You already know that ability.".to_string());
        }
        if self.level() < offer.level {
            return Err(format!("You need to be level {} to learn that ability.", offer.level));
        }
        if self.ability_points < offer.cost {
            return Err(format!("That ability costs {} ability points.", offer.cost));
        }
        self.ability_points -= offer.cost;
        self.abilities.push(ability.to_string());
        Ok(())
    }

    /// Puts stats and abilities back to the class's starting ones, and gives
    /// back every point earned so far.
    fn refund(&mut self, class: &ClassDef, common_abilities: &[String]) {
        self.stats = class.stats;
        self.stat_points = total_stat_points(self.progress.level);
        self.ability_points = total_ability_points(self.progress.level);
        self.abilities = starting_abilities(class, common_abilities);
    }

    /// Their secondary stats, with this gear.
    pub fn derived(&self, class: &ClassDef, gear: &Gear) -> DerivedStats {
        DerivedStats::of(&class.rules, self.level(), &self.stats, gear)
    }

    /// Their character sheet, as the player sees it.
    pub fn sheet(&self, class: &ClassDef, equipment: &Equipment, pack: &ContentPack) -> CharacterSheet {
        let derived = self.derived(class, &equipment.gear(&pack.items));
        let experience_to_next = if self.level() >= MAX_LEVEL { 0 } else { experience_to_next_level(self.level()) };
        let class_abilities = class
            .abilities
            .iter()
            .map(|offer| AbilityOffer { ability: offer.ability.clone(), level: offer.level, cost: offer.cost })
            .collect();
        CharacterSheet {
            class: class.name.clone(),
            level: self.level(),
            experience: self.progress.experience,
            experience_to_next,
            stats: self.stats,
            stat_points: self.stat_points,
            ability_points: self.ability_points,
            max_health: derived.max_health,
            max_mana: derived.max_mana,
            armour_class: derived.armour_class,
            attack_bonus: derived.attack_bonus,
            damage_bonus: derived.damage_bonus,
            attacks_per_second: derived.attacks_per_second,
            damage: derived.damage,
            abilities: self.abilities.clone(),
            class_abilities,
            equipment: equipment.saved(),
        }
    }
}

/// Everyone's abilities, then the class's starting ones (each once).
fn starting_abilities(class: &ClassDef, common_abilities: &[String]) -> Vec<String> {
    let mut abilities: Vec<String> = Vec::new();
    for ability in common_abilities.iter().chain(&class.starting_abilities) {
        if !abilities.contains(ability) {
            abilities.push(ability.clone());
        }
    }
    abilities
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
            rules: ClassRules { hit_die: 8, mana_per_level: 4, casting_stat: Stat::Intelligence },
            stats: Stats::default(),
            starting_equipment: Vec::new(),
            starting_abilities: Vec::new(),
            abilities: Vec::new(),
        })
}

/// A class by its id or name, ignoring case (for `/setclass`).
pub fn find_class<'a>(pack: &'a ContentPack, id_or_name: &str) -> Option<&'a ClassDef> {
    pack.classes
        .iter()
        .find(|class| class.id.eq_ignore_ascii_case(id_or_name) || class.name.eq_ignore_ascii_case(id_or_name))
}

/// Spends players' stat and ability points as they ask, telling them if they can't.
fn spend_points(
    mut requests: MessageReader<CharacterRequest>,
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &mut Character)>,
) {
    for request in requests.read() {
        let Ok((player, mut character)) = players.get_mut(request.player) else { continue };
        let outcome = match &request.action {
            CharacterAction::SpendStatPoint(stat) => character.spend_stat_point(*stat),
            CharacterAction::LearnAbility(ability) => {
                let class = class_or_default(&content.pack, Some(&character.class));
                character.learn_ability(&class, ability).map(|()| {
                    let name = content.pack.abilities.get(ability).map_or(ability.as_str(), |def| def.name.as_str());
                    link.send(player.connection, &ServerMessage::Notice { text: format!("You learn {name}.") });
                })
            }
        };
        if let Err(text) = outcome {
            link.send(player.connection, &ServerMessage::Notice { text });
        }
    }
}

fn send_changed_sheets(
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    players: Query<(&Player, &Character, &Equipment), Or<(Changed<Character>, Changed<Equipment>)>>,
) {
    for (player, character, equipment) in &players {
        let class = class_or_default(&content.pack, Some(&character.class));
        link.send(player.connection, &ServerMessage::Character(character.sheet(&class, equipment, &content.pack)));
    }
}

#[cfg(test)]
mod tests {
    use venture_content::ClassAbility;

    use super::*;

    fn fighter() -> ClassDef {
        ClassDef {
            id: "fighter".to_string(),
            name: "Fighter".to_string(),
            description: String::new(),
            rules: ClassRules { hit_die: 10, mana_per_level: 3, casting_stat: Stat::Wisdom },
            stats: Stats { strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10 },
            starting_equipment: Vec::new(),
            starting_abilities: vec!["whack".to_string()],
            abilities: vec![ClassAbility { ability: "bash".to_string(), level: 3, cost: 2 }],
        }
    }

    fn mage() -> ClassDef {
        ClassDef {
            id: "mage".to_string(),
            name: "Mage".to_string(),
            stats: Stats { strength: 8, dexterity: 12, constitution: 12, intelligence: 15, wisdom: 13 },
            starting_abilities: vec!["air_blast".to_string()],
            abilities: Vec::new(),
            ..fighter()
        }
    }

    fn common() -> Vec<String> {
        vec!["rest".to_string()]
    }

    fn new_fighter() -> Character {
        Character::new(&fighter(), BodyType::Feminine, &common())
    }

    #[test]
    fn new_characters_know_everyones_abilities_and_their_classs() {
        assert_eq!(new_fighter().abilities, ["rest", "whack"]);
    }

    #[test]
    fn levelling_up_brings_points_and_keeps_what_was_spent() {
        let mut character = new_fighter();
        character.stats.strength += 2; // as if they'd spent points
        character.set_level(5, &fighter(), &common());
        assert_eq!(character.level(), 5);
        assert_eq!((character.stat_points, character.ability_points), (8, 5));
        assert_eq!(character.stats.strength, 17);
    }

    #[test]
    fn going_down_a_level_starts_stats_over() {
        let mut character = new_fighter();
        character.set_level(10, &fighter(), &common());
        character.stats.strength += 4;
        character.stat_points -= 4;
        character.set_level(3, &fighter(), &common());
        assert_eq!(character.stats, fighter().stats);
        assert_eq!((character.stat_points, character.ability_points), (4, 2));
    }

    #[test]
    fn changing_class_refunds_everything_and_keeps_the_level() {
        let mut character = new_fighter();
        character.set_level(6, &fighter(), &common());
        character.stats.strength += 10;
        character.learn_ability(&fighter(), "bash").unwrap();
        character.change_class(&mage(), &common());
        assert_eq!(character.class, "mage");
        assert_eq!(character.level(), 6);
        assert_eq!(character.stats, mage().stats);
        // Levels 2 to 6: 2 stat points each; an ability point each, and a bonus one at 5.
        assert_eq!((character.stat_points, character.ability_points), (10, 6));
        assert_eq!(character.abilities, ["rest", "air_blast"]);
    }

    #[test]
    fn resetting_starts_at_level_one() {
        let mut character = new_fighter();
        character.set_level(20, &fighter(), &common());
        character.reset(&fighter(), &common());
        assert_eq!(character, new_fighter());
    }

    #[test]
    fn stat_points_raise_abilities() {
        let mut character = new_fighter();
        assert!(character.spend_stat_point(Stat::Strength).is_err(), "no points yet");
        character.set_level(2, &fighter(), &common());
        character.spend_stat_point(Stat::Strength).unwrap();
        assert_eq!((character.stats.strength, character.stat_points), (16, 1));
    }

    #[test]
    fn abilities_are_learned_at_their_level_for_their_cost() {
        let mut character = new_fighter();
        assert_eq!(character.learn_ability(&fighter(), "fireball").unwrap_err(), "Fighters can't learn that ability.");
        character.set_level(2, &fighter(), &common());
        assert!(character.learn_ability(&fighter(), "bash").unwrap_err().contains("level 3"));
        character.set_level(3, &fighter(), &common());
        character.learn_ability(&fighter(), "bash").unwrap();
        assert_eq!((character.ability_points, character.abilities.len()), (0, 3));
        assert!(character.learn_ability(&fighter(), "bash").unwrap_err().contains("already"));
    }

    #[test]
    fn the_sheet_shows_the_rules_numbers() {
        let sheet = new_fighter().sheet(&fighter(), &Equipment::default(), &ContentPack::default());
        assert_eq!((sheet.level, sheet.experience_to_next), (1, 100));
        assert_eq!(sheet.max_health, 12, "d10 + CON 14 (+2)");
        assert_eq!(sheet.attack_bonus, 4, "+2 proficiency, +2 STR");
        assert_eq!(sheet.damage.to_string(), "3", "fists: 1 + STR");
        assert_eq!(sheet.class_abilities.len(), 1);
    }
}
