//! Previews worked out with the game's own rules (`venture_rpg`): what a
//! class is like at each level, and how hard a creature hits.

use std::collections::HashMap;

use bevy_egui::egui;
use venture_content::{ClassDef, CreatureDef, ItemDef};
use venture_protocol::EquipSlot;
use venture_rpg::progression::{total_ability_points, total_stat_points};
use venture_rpg::{DerivedStats, Gear, combat::hit_chance};

/// Levels the class preview shows.
const LEVELS: [u32; 8] = [1, 2, 3, 5, 10, 20, 40, 60];

/// A class at each level, in its starting equipment, before spending any
/// stat points.
pub fn class_preview(ui: &mut egui::Ui, class: &ClassDef, items: &HashMap<String, ItemDef>) {
    let mut gear = Gear::default();
    for equip in class.starting_equipment.iter().filter_map(|id| items.get(id)?.equip.as_ref()) {
        gear.armour += equip.armour;
        if let Some(weapon) = equip.weapon.as_ref().filter(|_| equip.slots.contains(&EquipSlot::DominantHand)) {
            gear.weapon_stat = weapon.stat;
            gear.weapon_speed = weapon.speed;
            gear.weapon_damage = weapon.damage;
        }
    }
    egui::CollapsingHeader::new("At each level").default_open(true).show(ui, |ui| {
        ui.weak("In its starting equipment, with no stat points spent.");
        egui::Grid::new("class levels").striped(true).show(ui, |ui| {
            for heading in [
                "Level",
                "Health",
                "Mana",
                "Armour class",
                "Attack",
                "Damage",
                "Attacks a second",
                "Stat points",
                "Ability points",
            ] {
                ui.strong(heading);
            }
            ui.end_row();
            for level in LEVELS {
                let stats = DerivedStats::of(&class.rules, level, &class.stats, &gear);
                ui.label(level.to_string());
                ui.label(stats.max_health.to_string());
                ui.label(stats.max_mana.to_string());
                ui.label(stats.armour_class.to_string());
                ui.label(format!("{:+}", stats.attack_bonus));
                ui.label(stats.damage.to_string());
                ui.label(format!("{:.2}", stats.attacks_per_second));
                ui.label(total_stat_points(level).to_string());
                ui.label(total_ability_points(level).to_string());
                ui.end_row();
            }
        });
    });
}

/// How often a creature hits different armour, and what that does.
pub fn creature_preview(ui: &mut egui::Ui, creature: &CreatureDef) {
    let attack = &creature.attack;
    egui::CollapsingHeader::new("Its attack").default_open(true).show(ui, |ui| {
        let (low, high) = attack.damage.range();
        ui.label(format!(
            "{} damage a hit ({low} to {high}), {:.2} attacks a second.",
            attack.damage.average(),
            attack.speed
        ));
        egui::Grid::new("creature hits").striped(true).show(ui, |ui| {
            for heading in ["Against armour class", "Hits", "Damage a second"] {
                ui.strong(heading);
            }
            ui.end_row();
            for armour_class in [10, 12, 14, 16, 18] {
                let chance = hit_chance(attack.bonus, armour_class);
                ui.label(armour_class.to_string());
                ui.label(format!("{:.0}%", chance * 100.0));
                ui.label(format!("{:.1}", chance * attack.damage.average() as f32 * attack.speed));
                ui.end_row();
            }
        });
    });
}
