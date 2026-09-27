//! Players' inventories: the items each one carries, in a fixed number of
//! slots, and their money (Miasma).
//!
//! An [`Inventory`] can only change through [`Inventory::apply`], which makes
//! a whole [`Change`] (items and Miasma taken and given) or none of it. So a
//! trade can never take the Miasma and fail to hand over the goods, items can't be
//! duplicated, and counts never go below zero, whichever module asks (shops,
//! dialogue, quests). Changed inventories are sent to their players
//! automatically.

use std::collections::BTreeMap;

use bevy::prelude::*;
use venture_content::ItemDef;
use venture_protocol::{ItemStack, ServerMessage};

use crate::ServerSystems;
use crate::entities::Player;
use crate::link::Link;

/// How many slots an inventory has.
pub const INVENTORY_SLOTS: usize = 50;

/// Sends players their inventories when they change.
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, send_changed_inventories.in_set(ServerSystems::Updates));
    }
}

/// The items a player carries.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Inventory {
    slots: Vec<Option<ItemStack>>,
    miasma: u64,
}

/// Items to take from an inventory and items to give it, all at once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    /// Items to take: (item id, how many).
    pub take: Vec<(String, u32)>,
    /// Items to give: (item id, how many).
    pub give: Vec<(String, u32)>,
    /// Miasma to take.
    pub pay: u64,
    /// Miasma to give.
    pub earn: u64,
}

impl Change {
    /// A change that only gives items.
    pub fn give(item: &str, count: u32) -> Self {
        Self { give: vec![(item.to_string(), count)], ..Self::default() }
    }

    /// A change that only takes items.
    pub fn take(item: &str, count: u32) -> Self {
        Self { take: vec![(item.to_string(), count)], ..Self::default() }
    }

    /// A change that only takes Miasma.
    pub fn pay(amount: u64) -> Self {
        Self { pay: amount, ..Self::default() }
    }

    /// A change that only gives Miasma.
    pub fn earn(amount: u64) -> Self {
        Self { earn: amount, ..Self::default() }
    }

    /// Also takes this much Miasma.
    pub fn and_pay(mut self, amount: u64) -> Self {
        self.pay += amount;
        self
    }

    /// Also gives this much Miasma.
    pub fn and_earn(mut self, amount: u64) -> Self {
        self.earn += amount;
        self
    }

    /// Also takes these items.
    pub fn and_take(mut self, item: &str, count: u32) -> Self {
        self.take.push((item.to_string(), count));
        self
    }

    /// Also gives these items.
    pub fn and_give(mut self, item: &str, count: u32) -> Self {
        self.give.push((item.to_string(), count));
        self
    }
}

/// Why a [`Change`] couldn't be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryProblem {
    /// There aren't enough of this item to take.
    NotEnough(String),
    /// There isn't enough Miasma to pay.
    NotEnoughMiasma,
    /// There's no room for the items given.
    Full,
}

impl Inventory {
    /// An inventory with nothing in it.
    pub fn empty() -> Self {
        Self { slots: vec![None; INVENTORY_SLOTS], miasma: 0 }
    }

    /// An inventory from a saved profile. Anything unusable (empty stacks)
    /// is dropped, and items are packed into the first slots if there are
    /// more saved slots than the inventory has.
    pub fn from_saved(saved: Vec<Option<ItemStack>>, miasma: u64) -> Self {
        let mut slots: Vec<Option<ItemStack>> =
            saved.into_iter().map(|slot| slot.filter(|stack| stack.count > 0)).collect();
        if slots.len() > INVENTORY_SLOTS {
            slots.retain(Option::is_some);
            slots.truncate(INVENTORY_SLOTS);
        }
        slots.resize(INVENTORY_SLOTS, None);
        Self { slots, miasma }
    }

    /// How much Miasma they have.
    pub fn miasma(&self) -> u64 {
        self.miasma
    }

    /// Every slot, in order.
    pub fn slots(&self) -> &[Option<ItemStack>] {
        &self.slots
    }

    /// What's in a slot, if anything.
    pub fn slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index)?.as_ref()
    }

    /// How many of an item the inventory holds.
    pub fn count(&self, item: &str) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|stack| stack.item == item)
            .fold(0, |total, stack| total.saturating_add(stack.count))
    }

    /// Makes the whole change, or (if any of it can't be done) none of it.
    /// `items` says which items stack.
    pub fn apply(&mut self, change: &Change, items: &BTreeMap<String, ItemDef>) -> Result<(), InventoryProblem> {
        let miasma = self.miasma.checked_sub(change.pay).ok_or(InventoryProblem::NotEnoughMiasma)?;
        let miasma = miasma.checked_add(change.earn).ok_or(InventoryProblem::Full)?;
        let mut slots = self.slots.clone();
        for (item, count) in &change.take {
            take(&mut slots, item, *count)?;
        }
        for (item, count) in &change.give {
            let stackable = items.get(item).is_some_and(|def| def.stackable);
            give(&mut slots, item, *count, stackable)?;
        }
        self.slots = slots;
        self.miasma = miasma;
        Ok(())
    }
}

/// Takes `count` of an item, from the last slots first.
fn take(slots: &mut [Option<ItemStack>], item: &str, mut count: u32) -> Result<(), InventoryProblem> {
    for slot in slots.iter_mut().rev() {
        if count == 0 {
            break;
        }
        let Some(stack) = slot.as_mut().filter(|stack| stack.item == item) else { continue };
        let taken = stack.count.min(count);
        stack.count -= taken;
        count -= taken;
        if stack.count == 0 {
            *slot = None;
        }
    }
    if count > 0 { Err(InventoryProblem::NotEnough(item.to_string())) } else { Ok(()) }
}

/// Gives `count` of an item: onto its stack if it stacks, otherwise one per
/// empty slot.
fn give(slots: &mut [Option<ItemStack>], item: &str, count: u32, stackable: bool) -> Result<(), InventoryProblem> {
    if count == 0 {
        return Ok(());
    }
    if stackable {
        if let Some(stack) = slots.iter_mut().flatten().find(|stack| stack.item == item) {
            stack.count = stack.count.checked_add(count).ok_or(InventoryProblem::Full)?;
            return Ok(());
        }
        let empty = slots.iter_mut().find(|slot| slot.is_none()).ok_or(InventoryProblem::Full)?;
        *empty = Some(ItemStack { item: item.to_string(), count });
        return Ok(());
    }
    for _ in 0..count {
        let empty = slots.iter_mut().find(|slot| slot.is_none()).ok_or(InventoryProblem::Full)?;
        *empty = Some(ItemStack { item: item.to_string(), count: 1 });
    }
    Ok(())
}

/// An item's name, with the count if there's more than one: "Bread (2)".
pub fn describe_items(item: &str, count: u32, items: &BTreeMap<String, ItemDef>) -> String {
    let name = items.get(item).map_or(item, |def| def.name.as_str());
    if count == 1 { name.to_string() } else { format!("{name} ({count})") }
}

fn send_changed_inventories(mut link: ResMut<Link>, players: Query<(&Player, &Inventory), Changed<Inventory>>) {
    for (player, inventory) in &players {
        let message = ServerMessage::Inventory { slots: inventory.slots.clone(), miasma: inventory.miasma };
        link.send(player.connection, &message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> BTreeMap<String, ItemDef> {
        let item = |id: &str, stackable| ItemDef {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            icon: String::new(),
            stackable,
            value: 1,
            equip: None,
            on_use: Default::default(),
            notes: String::new(),
        };
        BTreeMap::from([("arrows".to_string(), item("arrows", true)), ("bread".to_string(), item("bread", false))])
    }

    #[test]
    fn stackable_items_share_a_slot_and_others_take_one_each() {
        let mut inventory = Inventory::empty();
        inventory.apply(&Change::give("arrows", 10).and_give("bread", 2), &items()).unwrap();
        inventory.apply(&Change::give("arrows", 5), &items()).unwrap();
        assert_eq!(inventory.count("arrows"), 15);
        assert_eq!(inventory.count("bread"), 2);
        assert_eq!(inventory.slots().iter().flatten().count(), 3);
    }

    #[test]
    fn a_change_happens_completely_or_not_at_all() {
        let mut inventory = Inventory::empty();
        inventory.apply(&Change::give("arrows", 3), &items()).unwrap();
        let before = inventory.clone();

        // Not enough arrows to trade: the bread isn't handed over either.
        let purchase = Change::take("arrows", 4).and_give("bread", 1);
        assert_eq!(inventory.apply(&purchase, &items()), Err(InventoryProblem::NotEnough("arrows".to_string())));
        assert_eq!(inventory, before);

        // No room: the arrows aren't taken.
        let too_much = Change::take("arrows", 1).and_give("bread", INVENTORY_SLOTS as u32);
        assert_eq!(inventory.apply(&too_much, &items()), Err(InventoryProblem::Full));
        assert_eq!(inventory, before);
    }

    #[test]
    fn taking_empties_slots_and_frees_room() {
        let mut inventory = Inventory::empty();
        inventory.apply(&Change::give("bread", INVENTORY_SLOTS as u32), &items()).unwrap();
        assert_eq!(inventory.apply(&Change::give("arrows", 1), &items()), Err(InventoryProblem::Full));
        // Swapping a loaf for arrows works, because the loaf's slot is freed first.
        inventory.apply(&Change::take("bread", 1).and_give("arrows", 4), &items()).unwrap();
        assert_eq!(inventory.count("arrows"), 4);
    }

    #[test]
    fn saved_inventories_are_cleaned_up() {
        let stack = |count| Some(ItemStack { item: "arrows".to_string(), count });
        let inventory = Inventory::from_saved(vec![stack(0), stack(5)], 7);
        assert_eq!(inventory.slots().len(), INVENTORY_SLOTS);
        assert_eq!(inventory.slot(0), None);
        assert_eq!(inventory.count("arrows"), 5);
        assert_eq!(inventory.miasma(), 7);
    }

    #[test]
    fn miasma_is_paid_and_earned_with_the_rest_of_a_change() {
        let mut inventory = Inventory::from_saved(Vec::new(), 10);
        let purchase = Change::give("bread", 1).and_pay(4);
        inventory.apply(&purchase, &items()).unwrap();
        assert_eq!((inventory.miasma(), inventory.count("bread")), (6, 1));
        let too_dear = Change::give("bread", 1).and_pay(7);
        assert_eq!(inventory.apply(&too_dear, &items()), Err(InventoryProblem::NotEnoughMiasma));
        assert_eq!(inventory.count("bread"), 1, "nothing changed");
        inventory.apply(&Change::take("bread", 1).and_earn(2), &items()).unwrap();
        assert_eq!(inventory.miasma(), 8);
    }
}
