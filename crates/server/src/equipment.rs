//! Equipment: what each player wears and wields. The only code that changes it.
//!
//! Equipping moves an item from the bag into the slots it fills (a robe fills
//! body and legs), and whatever was in those slots back into the bag, as one
//! inventory [`Change`], so nothing is lost or duplicated. What's worn adds
//! to the character's stats ([`Equipment::gear`]) and shows on them
//! ([`player_look`]); when it changes, everyone is told how the player now looks.

use std::collections::BTreeMap;

use bevy::prelude::*;
use venture_content::{ClassDef, ItemDef};
use venture_protocol::{EquipSlot, ItemLook, PlayerLook, ServerMessage};
use venture_rpg::{Gear, UNARMED_DAMAGE};

use crate::ServerSystems;
use crate::character::Character;
use crate::content::LoadedContent;
use crate::entities::{NetworkId, Player};
use crate::inventory::{Change, Inventory, InventoryProblem, describe_items};
use crate::link::Link;
use crate::requests::{EquipmentAction, EquipmentRequest};

/// Handles equipping and unequipping.
pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, change_equipment.in_set(ServerSystems::Gameplay));
    }
}

/// What a player has equipped: the item (an id) in each filled slot. An item
/// filling several slots is in each of them.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Equipment {
    slots: BTreeMap<EquipSlot, String>,
}

impl Equipment {
    /// Equipment from a saved profile.
    pub fn from_saved(saved: Vec<(EquipSlot, String)>) -> Self {
        Self { slots: saved.into_iter().collect() }
    }

    /// A class's starting equipment (items that can't be equipped are skipped).
    pub fn starting(class: &ClassDef, items: &BTreeMap<String, ItemDef>) -> Self {
        let mut equipment = Self::default();
        for item in &class.starting_equipment {
            let Some(equip) = items.get(item).and_then(|def| def.equip.as_ref()) else { continue };
            for slot in &equip.slots {
                equipment.slots.insert(*slot, item.clone());
            }
        }
        equipment
    }

    /// What to save: each filled slot and its item.
    pub fn saved(&self) -> Vec<(EquipSlot, String)> {
        self.slots.iter().map(|(slot, item)| (*slot, item.clone())).collect()
    }

    /// The item in the dominant hand, if any.
    pub fn in_hand(&self) -> Option<&str> {
        self.slots.get(&EquipSlot::DominantHand).map(String::as_str)
    }

    /// Each item worn, once (even if it fills several slots), in slot order.
    fn items(&self) -> Vec<&str> {
        let mut worn: Vec<&str> = Vec::new();
        for item in self.slots.values() {
            if !worn.contains(&item.as_str()) {
                worn.push(item);
            }
        }
        worn
    }

    /// What the equipment adds to the character's stats: armour from
    /// everything worn, and the weapon's (or bare fists').
    pub fn gear(&self, items: &BTreeMap<String, ItemDef>) -> Gear {
        let armour =
            self.items().iter().filter_map(|item| items.get(*item)?.equip.as_ref()).map(|equip| equip.armour).sum();
        let weapon = self.slots.get(&EquipSlot::DominantHand).and_then(|item| items.get(item)?.equip.as_ref()?.weapon);
        match weapon {
            Some(weapon) => {
                Gear { armour, weapon_stat: weapon.stat, weapon_speed: weapon.speed, weapon_damage: weapon.damage }
            }
            None => Gear { armour, weapon_damage: UNARMED_DAMAGE, ..Gear::default() },
        }
    }

    /// How each worn item looks.
    pub fn looks(&self, items: &BTreeMap<String, ItemDef>) -> Vec<ItemLook> {
        self.items().iter().filter_map(|item| Some(items.get(*item)?.equip.as_ref()?.look)).collect()
    }

    /// The outfit parts what's worn puts on the character (each once).
    pub fn outfit(&self, items: &BTreeMap<String, ItemDef>) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        for item in self.items() {
            let Some(equip) = items.get(item).and_then(|def| def.equip.as_ref()) else { continue };
            for part in &equip.parts {
                if !parts.contains(part) {
                    parts.push(part.clone());
                }
            }
        }
        parts
    }

    /// Equips the item in an inventory slot, putting whatever it replaces in
    /// the bag. Returns what to tell the player, or `None` for a request that
    /// doesn't make sense (such as an empty slot).
    pub fn equip(
        &mut self,
        inventory: &mut Inventory,
        slot: usize,
        items: &BTreeMap<String, ItemDef>,
    ) -> Option<Result<String, String>> {
        let item = inventory.slot(slot)?.item.clone();
        let Some(equip) = items.get(&item).and_then(|def| def.equip.as_ref()) else {
            return Some(Err("You can't equip that.".to_string()));
        };
        // Everything in the slots the item needs comes off (each item once).
        let mut replaced: Vec<String> = Vec::new();
        for slot in &equip.slots {
            if let Some(worn) = self.slots.get(slot)
                && !replaced.contains(worn)
            {
                replaced.push(worn.clone());
            }
        }
        let mut change = Change::take(&item, 1);
        change.give = replaced.iter().map(|worn| (worn.clone(), 1)).collect();
        if let Err(problem) = inventory.apply(&change, items) {
            return Some(Err(match problem {
                InventoryProblem::Full => "You have no room in your inventory for what you'd take off.".to_string(),
                _ => "You can't equip that.".to_string(),
            }));
        }
        self.slots.retain(|_, worn| !replaced.contains(worn));
        for slot in &equip.slots {
            self.slots.insert(*slot, item.clone());
        }
        Some(Ok(format!("You equip: {}.", describe_items(&item, 1, items))))
    }

    /// Takes off what's in a slot (all of it, if it fills several), into the bag.
    pub fn unequip(
        &mut self,
        inventory: &mut Inventory,
        slot: EquipSlot,
        items: &BTreeMap<String, ItemDef>,
    ) -> Option<Result<String, String>> {
        let item = self.slots.get(&slot)?.clone();
        if inventory.apply(&Change::give(&item, 1), items).is_err() {
            return Some(Err("You have no room in your inventory to take that off.".to_string()));
        }
        self.slots.retain(|_, worn| *worn != item);
        Some(Ok(format!("You take off: {}.", describe_items(&item, 1, items))))
    }
}

/// How a player looks: their build, and what they wear.
pub fn player_look(character: &Character, equipment: &Equipment, items: &BTreeMap<String, ItemDef>) -> PlayerLook {
    PlayerLook {
        body: character.body,
        features: character.features.clone(),
        worn: equipment.looks(items),
        outfit: equipment.outfit(items),
    }
}

/// Carries out equip and unequip requests, and shows everyone the new look.
fn change_equipment(
    mut requests: MessageReader<EquipmentRequest>,
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &NetworkId, &Character, &mut Equipment, &mut Inventory)>,
    everyone: Query<&Player>,
) {
    let items = &content.pack.items;
    for request in requests.read() {
        let Ok((player, &NetworkId(id), character, mut equipment, mut inventory)) = players.get_mut(request.player)
        else {
            continue;
        };
        let outcome = match request.action {
            EquipmentAction::Equip(slot) => equipment.equip(&mut inventory, slot as usize, items),
            EquipmentAction::Unequip(slot) => equipment.unequip(&mut inventory, slot, items),
        };
        let Some(outcome) = outcome else { continue };
        let changed = outcome.is_ok();
        let text = outcome.unwrap_or_else(|problem| problem);
        link.send(player.connection, &ServerMessage::Notice { text });
        if changed {
            let message = ServerMessage::PlayerChanged {
                id,
                level: character.level(),
                look: player_look(character, &equipment, items),
                celebrate: false,
            };
            link.broadcast(everyone.iter().map(|player| player.connection), &message);
        }
    }
}

#[cfg(test)]
mod tests {
    use venture_content::{Equippable, WeaponDef};
    use venture_protocol::LookStyle;
    use venture_rpg::{Dice, Stat};

    use super::*;

    fn items() -> BTreeMap<String, ItemDef> {
        let look = |style| ItemLook { style, main: (0.5, 0.5, 0.5), trim: (0.2, 0.2, 0.2) };
        let item = |id: &str, slots: Vec<EquipSlot>, armour, weapon, style| ItemDef {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            icon: String::new(),
            stackable: false,
            value: 1,
            equip: Some(Equippable { slots, armour, weapon, look: look(style), parts: Vec::new() }),
            on_use: Default::default(),
            fishing: None,
            notes: String::new(),
        };
        let sword = WeaponDef { damage: Dice::new(1, 6), speed: 1.0, stat: Stat::Strength };
        [
            item("sword", vec![EquipSlot::DominantHand], 0, Some(sword), LookStyle::Sword),
            item("chestguard", vec![EquipSlot::Body], 2, None, LookStyle::Plates),
            item("legguards", vec![EquipSlot::Legs], 1, None, LookStyle::LegPlates),
            item("robe", vec![EquipSlot::Body, EquipSlot::Legs], 0, None, LookStyle::Robe),
        ]
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect()
    }

    /// An inventory holding these items, one per slot.
    fn bag(contents: &[&str]) -> Inventory {
        let mut inventory = Inventory::empty();
        for item in contents {
            inventory.apply(&Change::give(item, 1), &items()).unwrap();
        }
        inventory
    }

    fn slot_of(inventory: &Inventory, item: &str) -> usize {
        inventory.slots().iter().position(|slot| slot.as_ref().is_some_and(|stack| stack.item == item)).unwrap()
    }

    #[test]
    fn gear_adds_armour_and_the_weapons_stats() {
        let mut inventory = bag(&["sword", "chestguard", "legguards"]);
        let mut equipment = Equipment::default();
        for item in ["sword", "chestguard", "legguards"] {
            let slot = slot_of(&inventory, item);
            equipment.equip(&mut inventory, slot, &items()).unwrap().unwrap();
        }
        let gear = equipment.gear(&items());
        assert_eq!((gear.armour, gear.weapon_damage), (3, Dice::new(1, 6)));
        assert_eq!(inventory.slots().iter().flatten().count(), 0, "all of it came out of the bag");
        assert_eq!(equipment.looks(&items()).len(), 3);
    }

    #[test]
    fn a_robe_takes_the_place_of_both_body_and_legs() {
        let mut inventory = bag(&["chestguard", "legguards", "robe"]);
        let mut equipment = Equipment::default();
        for item in ["chestguard", "legguards", "robe"] {
            let slot = slot_of(&inventory, item);
            equipment.equip(&mut inventory, slot, &items()).unwrap().unwrap();
        }
        assert_eq!(equipment.saved(), [(EquipSlot::Body, "robe".to_string()), (EquipSlot::Legs, "robe".to_string())]);
        assert_eq!(inventory.count("chestguard") + inventory.count("legguards"), 2, "both plates are back in the bag");

        // Taking the robe off frees both slots, and it's one robe in the bag.
        equipment.unequip(&mut inventory, EquipSlot::Legs, &items()).unwrap().unwrap();
        assert_eq!(equipment, Equipment::default());
        assert_eq!(inventory.count("robe"), 1);
    }

    #[test]
    fn nonsense_requests_change_nothing() {
        let mut inventory = bag(&[]);
        let mut equipment = Equipment::default();
        assert_eq!(equipment.equip(&mut inventory, 3, &items()), None, "an empty slot");
        assert_eq!(equipment.unequip(&mut inventory, EquipSlot::DominantHand, &items()), None, "nothing worn");
        assert_eq!(equipment.gear(&items()).weapon_damage, UNARMED_DAMAGE);
    }
}
