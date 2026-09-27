//! Using items: eating bread heals, say. The only code that uses items up.
//!
//! An item's `on_use` effect (from the content) says what it does. Using one
//! takes it from the bag through [`Inventory::apply`] and heals through
//! [`Health::heal`], and only when it would do something (bread isn't eaten
//! at full health). After using an item, a player waits a moment
//! ([`ItemCooldown`], from `server.ron`) before using another.

use std::collections::BTreeMap;

use bevy::prelude::*;
use venture_content::{ItemDef, ItemEffect};
use venture_protocol::ServerMessage;

use crate::ServerSystems;
use crate::combat::Health;
use crate::content::LoadedContent;
use crate::entities::{NetworkId, Player, Position};
use crate::inventory::{Change, Inventory, describe_items};
use crate::link::Link;
use crate::requests::ItemUseRequest;

/// How far away (in metres) other players see someone heal.
const VIEW_RANGE: f32 = 60.0;

/// Handles using items.
pub struct ConsumablesPlugin;

impl Plugin for ConsumablesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, (use_items, count_down).in_set(ServerSystems::Gameplay));
    }
}

/// How long a player waits after using an item before using another, in seconds.
#[derive(Resource, Clone, Copy)]
pub struct ItemCooldown(pub f32);

/// Seconds until a player can use another item.
#[derive(Component)]
struct Waiting(f32);

/// What using an item did.
#[derive(Debug, PartialEq)]
pub struct Used {
    /// What to tell the player.
    pub text: String,
    /// How much health it gave.
    pub healed: u32,
}

/// Uses the item in an inventory slot, using one up if it does something.
/// Returns what it did, why not (in words for the player), or `None` for a
/// request that doesn't make sense (an empty slot).
pub fn use_item(
    inventory: &mut Inventory,
    health: &mut Health,
    slot: usize,
    items: &BTreeMap<String, ItemDef>,
) -> Option<Result<Used, String>> {
    let item = inventory.slot(slot)?.item.clone();
    let name = describe_items(&item, 1, items);
    let effect = items.get(&item).map_or(ItemEffect::Nothing, |def| def.on_use);
    let healing = match effect {
        ItemEffect::Nothing => return Some(Err(format!("You can't use {name}."))),
        ItemEffect::Heal(healing) => healing,
    };
    if health.current() >= health.max() {
        return Some(Err("You're not hurt.".to_string()));
    }
    if inventory.apply(&Change::take(&item, 1), items).is_err() {
        return Some(Err(format!("You can't use {name}.")));
    }
    let healed = health.heal(healing.amount(health.max()));
    Some(Ok(Used { text: format!("You use {name}: +{healed} health."), healed }))
}

/// Carries out item use requests.
fn use_items(
    mut commands: Commands,
    mut requests: MessageReader<ItemUseRequest>,
    content: Res<LoadedContent>,
    cooldown: Res<ItemCooldown>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &NetworkId, &Position, &mut Inventory, &mut Health, Has<Waiting>)>,
    viewers: Query<(&Player, &Position)>,
) {
    for request in requests.read() {
        let Ok((player, &NetworkId(id), &Position(at), mut inventory, mut health, waiting)) =
            players.get_mut(request.player)
        else {
            continue;
        };
        if waiting {
            continue;
        }
        let Some(outcome) = use_item(&mut inventory, &mut health, request.slot as usize, &content.pack.items) else {
            continue;
        };
        match outcome {
            Ok(used) => {
                commands.entity(request.player).insert(Waiting(cooldown.0));
                link.send(player.connection, &ServerMessage::Notice { text: used.text });
                let nearby: Vec<_> = viewers
                    .iter()
                    .filter(|(_, Position(viewer))| (viewer.0 - at.0).hypot(viewer.1 - at.1) <= VIEW_RANGE)
                    .map(|(viewer, _)| viewer.connection)
                    .collect();
                link.broadcast(nearby, &ServerMessage::Healed { id, amount: used.healed });
            }
            Err(text) => link.send(player.connection, &ServerMessage::Notice { text }),
        }
    }
}

fn count_down(mut commands: Commands, time: Res<Time>, mut waiting: Query<(Entity, &mut Waiting)>) {
    for (entity, mut waiting) in &mut waiting {
        waiting.0 -= time.delta_secs();
        if waiting.0 <= 0.0 {
            commands.entity(entity).remove::<Waiting>();
        }
    }
}

#[cfg(test)]
mod tests {
    use venture_rpg::Healing;

    use super::*;

    fn items() -> BTreeMap<String, ItemDef> {
        let item = |id: &str, on_use| ItemDef {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            icon: String::new(),
            stackable: true,
            value: 1,
            equip: None,
            on_use,
            fishing: None,
            notes: String::new(),
        };
        [
            item("bread", ItemEffect::Heal(Healing::Flat(5))),
            item("tonic", ItemEffect::Heal(Healing::Percent(50.0))),
            item("pebble", ItemEffect::Nothing),
        ]
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect()
    }

    fn bag(item: &str, count: u32) -> Inventory {
        let mut inventory = Inventory::empty();
        inventory.apply(&Change::give(item, count), &items()).unwrap();
        inventory
    }

    fn hurt(max: u32, damage: u32) -> Health {
        Health::partly(max - damage, max)
    }

    #[test]
    fn eating_heals_and_uses_one_up() {
        let mut inventory = bag("bread", 2);
        let mut health = hurt(20, 10);
        let used = use_item(&mut inventory, &mut health, 0, &items()).unwrap().unwrap();
        assert_eq!(used.healed, 5);
        assert_eq!(health.current(), 15);
        assert_eq!(inventory.count("bread"), 1);
    }

    #[test]
    fn healing_stops_at_full_and_percentages_scale() {
        let mut inventory = bag("tonic", 1);
        let mut health = hurt(40, 4);
        let used = use_item(&mut inventory, &mut health, 0, &items()).unwrap().unwrap();
        assert_eq!(used.healed, 4, "never past the most health");
        assert_eq!(health.current(), 40);
        assert_eq!(inventory.count("tonic"), 0);
    }

    #[test]
    fn nothing_is_used_up_for_nothing() {
        let mut inventory = bag("bread", 1);
        let mut health = Health::full(20);
        assert_eq!(use_item(&mut inventory, &mut health, 0, &items()), Some(Err("You're not hurt.".to_string())));
        assert_eq!(inventory.count("bread"), 1);

        let mut inventory = bag("pebble", 1);
        let mut health = hurt(20, 10);
        assert!(matches!(use_item(&mut inventory, &mut health, 0, &items()), Some(Err(_))));
        assert_eq!(inventory.count("pebble"), 1);
        assert_eq!(health.current(), 10);

        assert_eq!(use_item(&mut Inventory::empty(), &mut health, 3, &items()), None);
    }
}
