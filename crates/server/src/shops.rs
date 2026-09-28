//! Shops: buying from and selling to NPCs. The only code that runs shops.
//!
//! A shop is opened by an NPC's dialogue (`<<open_shop elsies_bakery>>`),
//! which sends an [`OpenShop`] here. The player then buys and sells through
//! [`ShopRequest`]s, until they close it or walk away. Every trade is one
//! inventory [`Change`] (Miasma for goods, or goods for Miasma), so it happens
//! completely or not at all.

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use venture_content::{ItemDef, ShopDef};
use venture_protocol::{ServerMessage, ShopStock};
use venture_simulation::TALK_RANGE;

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::{NetworkId, Player, Position};
use crate::inventory::{Change, Inventory, InventoryProblem, describe_items};
use crate::link::Link;
use crate::npcs::Npc;
use crate::requests::{ShopAction, ShopRequest};

/// Runs shops.
pub struct ShopsPlugin;

impl Plugin for ShopsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenShops>()
            .add_message::<OpenShop>()
            .add_systems(FixedUpdate, (open_shops, trade, close_distant_shops).chain().in_set(ServerSystems::Shops));
    }
}

/// Opens a shop for a player (sent by [`crate::dialogue`]).
#[derive(Message)]
pub struct OpenShop {
    /// The player.
    pub player: Entity,
    /// The NPC who runs the shop.
    pub npc: Entity,
    /// Which shop (an id from the content pack, already checked).
    pub shop: String,
}

/// The shops players have open, by player entity.
#[derive(Resource, Default)]
struct OpenShops(HashMap<Entity, OpenedShop>);

/// A shop a player has open. It keeps the shop as it was when opened, so the
/// stock the player sees is the stock they buy from, even if the content
/// reloads meanwhile.
struct OpenedShop {
    npc: Entity,
    shop: ShopDef,
}

fn open_shops(
    mut requests: MessageReader<OpenShop>,
    content: Res<LoadedContent>,
    mut open: ResMut<OpenShops>,
    mut link: ResMut<Link>,
    npcs: Query<&NetworkId, With<Npc>>,
    players: Query<&Player>,
) {
    for request in requests.read() {
        let (Ok(&NetworkId(npc)), Ok(player)) = (npcs.get(request.npc), players.get(request.player)) else { continue };
        let Some(shop) = content.pack.shops.get(&request.shop) else { continue };
        let stock = shop.stock.iter().map(|stock| ShopStock { item: stock.item.clone(), price: stock.price }).collect();
        link.send(player.connection, &ServerMessage::ShopOpened { npc, name: shop.name.clone(), stock });
        open.0.insert(request.player, OpenedShop { npc: request.npc, shop: shop.clone() });
    }
}

/// Carries out purchases and sales, and tells the player how each went.
fn trade(
    mut requests: MessageReader<ShopRequest>,
    content: Res<LoadedContent>,
    mut open: ResMut<OpenShops>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &mut Inventory)>,
) {
    let items = &content.pack.items;
    for request in requests.read() {
        let Ok((player, mut inventory)) = players.get_mut(request.player) else { continue };
        let Some(opened) = open.0.get(&request.player) else { continue };
        let outcome = match request.action {
            ShopAction::Close => {
                open.0.remove(&request.player);
                continue;
            }
            ShopAction::Buy { index, count } => buy(&opened.shop, index, count, &mut inventory, items),
            ShopAction::Sell { slot, count } => sell(&opened.shop, slot, count, &mut inventory, items),
        };
        // A request that doesn't match the shop or the bag (a stale click, or
        // a made-up message) gets no answer.
        if let Some(text) = outcome {
            link.send(player.connection, &ServerMessage::Notice { text });
        }
    }
}

/// Buys from a shop, returning what to tell the player.
fn buy(
    shop: &ShopDef,
    index: u32,
    count: u32,
    inventory: &mut Inventory,
    items: &BTreeMap<String, ItemDef>,
) -> Option<String> {
    let stock = shop.stock.get(index as usize)?;
    let cost = u64::from(stock.price) * u64::from(count);
    let change = Change::give(&stock.item, count).and_pay(cost);
    Some(match inventory.apply(&change, items) {
        Ok(()) => format!("You buy: {} for {cost} Miasma.", describe_items(&stock.item, count, items)),
        Err(InventoryProblem::NotEnoughMiasma | InventoryProblem::NotEnough(_)) => {
            "You don't have enough Miasma for that.".to_string()
        }
        Err(InventoryProblem::Full) => "You have no room in your inventory for that.".to_string(),
    })
}

/// Sells to a shop, returning what to tell the player.
fn sell(
    shop: &ShopDef,
    slot: u32,
    count: u32,
    inventory: &mut Inventory,
    items: &BTreeMap<String, ItemDef>,
) -> Option<String> {
    let item = inventory.slot(slot as usize)?.item.clone();
    let Some(price) = items.get(&item).and_then(ItemDef::sell_price) else {
        return Some(format!("{} won't buy that.", shop.name));
    };
    let payment = u64::from(price) * u64::from(count);
    let change = Change::take(&item, count).and_earn(payment);
    Some(match inventory.apply(&change, items) {
        Ok(()) => format!("You sell: {} for {payment} Miasma.", describe_items(&item, count, items)),
        Err(InventoryProblem::NotEnough(_) | InventoryProblem::NotEnoughMiasma) => {
            "You don't have that many to sell.".to_string()
        }
        Err(InventoryProblem::Full) => "You can't carry any more Miasma.".to_string(),
    })
}

/// Closes shops when the player walks away, or the NPC or player is gone.
fn close_distant_shops(
    mut open: ResMut<OpenShops>,
    mut link: ResMut<Link>,
    positions: Query<&Position>,
    players: Query<&Player>,
) {
    open.0.retain(|&player_entity, opened| {
        let Ok(player) = players.get(player_entity) else { return false };
        let in_range = match (positions.get(player_entity), positions.get(opened.npc)) {
            (Ok(&Position(a)), Ok(&Position(b))) => (a.0 - b.0).hypot(a.1 - b.1) <= TALK_RANGE,
            _ => false,
        };
        if !in_range {
            link.send(player.connection, &ServerMessage::ShopClosed);
        }
        in_range
    });
}

#[cfg(test)]
mod tests {
    use venture_content::ShopItem;

    use super::*;

    fn items() -> BTreeMap<String, ItemDef> {
        let item = |id: &str, stackable, value| ItemDef {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            icon: String::new(),
            stackable,
            value,
            equip: None,
            on_use: Default::default(),
            fishing: None,
            woodworking: None,
            notes: String::new(),
        };
        BTreeMap::from([("bread".to_string(), item("bread", false, 4)), ("pipe".to_string(), item("pipe", false, 0))])
    }

    fn bakery() -> ShopDef {
        ShopDef {
            id: "bakery".to_string(),
            name: "The Bakery".to_string(),
            stock: vec![ShopItem { item: "bread".to_string(), price: 5 }],
            notes: String::new(),
        }
    }

    fn inventory_with(miasma: u64) -> Inventory {
        let mut inventory = Inventory::empty();
        inventory.apply(&Change::give("pipe", 1).and_earn(miasma), &items()).unwrap();
        inventory
    }

    #[test]
    fn buying_swaps_miasma_for_goods() {
        let mut inventory = inventory_with(12);
        assert_eq!(buy(&bakery(), 0, 2, &mut inventory, &items()).unwrap(), "You buy: bread (2) for 10 Miasma.");
        assert_eq!((inventory.miasma(), inventory.count("bread")), (2, 2));
        assert_eq!(buy(&bakery(), 0, 1, &mut inventory, &items()).unwrap(), "You don't have enough Miasma for that.");
        assert_eq!(buy(&bakery(), 5, 1, &mut inventory, &items()), None, "no such stock");
    }

    #[test]
    fn selling_pays_half_value_and_refuses_worthless_things() {
        let mut inventory = inventory_with(0);
        inventory.apply(&Change::give("bread", 1), &items()).unwrap();
        let bread_slot = inventory.slots().iter().position(|slot| slot.as_ref().is_some_and(|s| s.item == "bread"));
        let bread_slot = bread_slot.unwrap() as u32;
        let pipe_slot = inventory.slots().iter().position(|slot| slot.as_ref().is_some_and(|s| s.item == "pipe"));

        assert_eq!(
            sell(&bakery(), pipe_slot.unwrap() as u32, 1, &mut inventory, &items()).unwrap(),
            "The Bakery won't buy that."
        );
        assert_eq!(
            sell(&bakery(), bread_slot, 2, &mut inventory, &items()).unwrap(),
            "You don't have that many to sell."
        );
        assert_eq!(sell(&bakery(), bread_slot, 1, &mut inventory, &items()).unwrap(), "You sell: bread for 2 Miasma.");
        assert_eq!(inventory.miasma(), 2);
        assert_eq!(sell(&bakery(), 27, 1, &mut inventory, &items()), None, "an empty slot");
    }
}
