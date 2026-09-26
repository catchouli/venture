//! The equipment tab: what's in each slot (clicking takes it off, into the
//! bag), what it adds up to, and the items in the bag that could be
//! equipped (clicking puts one on). The server does the swapping, and says
//! how it went in chat.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, EquipSlot};

use crate::content::ContentStore;
use crate::network::ToServer;
use crate::ui::character_window::bag::PlayerInventory;
use crate::ui::character_window::{Pages, QUIET_COLOUR, Sheet, TEXT_COLOUR, Tab, heading, icon_box, text};
use crate::ui::tooltip::Tooltip;

/// Size of an item's box, in pixels.
const BOX_SIZE: f32 = 40.0;
/// Size of an icon in its box, in pixels.
const ICON_SIZE: f32 = 32.0;

/// Shows the equipment tab.
pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (show_equipment, change_on_click));
    }
}

/// Where the slots go.
#[derive(Component)]
struct SlotList;

/// Where the equippable items in the bag go.
#[derive(Component)]
struct BagList;

/// What the equipment adds up to.
#[derive(Component)]
struct Summary;

/// A filled slot's box: clicking takes it off.
#[derive(Component)]
struct WornSlot(EquipSlot);

/// An equippable item in the bag: clicking puts it on.
#[derive(Component)]
struct BagItem(u32);

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Equipment)).with_children(|page| {
        page.spawn(heading("Worn"));
        page.spawn((SlotList, Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }));
        page.spawn((Summary, text("", 12.0, QUIET_COLOUR)));
        page.spawn(heading("In your bag"));
        page.spawn((BagList, Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }));
    });
}

/// Rebuilds the page when the equipment, the bag or the content changes.
fn show_equipment(
    mut commands: Commands,
    sheet: Res<Sheet>,
    inventory: Res<PlayerInventory>,
    content: Res<ContentStore>,
    slots: Single<Entity, With<SlotList>>,
    bag: Single<Entity, With<BagList>>,
    mut summary: Single<&mut Text, With<Summary>>,
) {
    if !sheet.is_changed() && !inventory.is_changed() && !content.is_changed() {
        return;
    }
    let Some(sheet) = &sheet.0 else { return };
    let tooltip = |item: &str, hint: &str| match content.items.get(item) {
        Some(def) => Tooltip { title: def.name.clone(), body: format!("{}\n{hint}", def.description) },
        None => Tooltip { title: item.to_string(), body: hint.to_string() },
    };
    let icon = |item: &str| {
        (
            ImageNode::new(content.icon_for(item).unwrap_or_default()),
            Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
        )
    };

    commands.entity(*slots).despawn_children().with_children(|list| {
        for slot in EquipSlot::ALL {
            let worn = sheet.equipment.iter().find(|(filled, _)| *filled == slot).map(|(_, item)| item);
            list.spawn(Node { align_items: AlignItems::Center, column_gap: px(8), ..default() }).with_children(|row| {
                let mut slot_box = row.spawn(icon_box(BOX_SIZE));
                if let Some(item) = worn {
                    slot_box.insert((WornSlot(slot), tooltip(item, "Click to take it off."))).with_child(icon(item));
                }
                row.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|words| {
                    words.spawn(text(slot.name(), 12.0, QUIET_COLOUR));
                    let name = worn.map_or("Nothing", |item| {
                        content.items.get(item).map_or(item.as_str(), |def| def.name.as_str())
                    });
                    words.spawn(text(name, 13.0, TEXT_COLOUR));
                });
            });
        }
    });
    summary.0 = format!("Armour class {}, damage {}", sheet.armour_class, sheet.damage);

    commands.entity(*bag).despawn_children().with_children(|list| {
        let equippable = inventory.slots.iter().enumerate().filter_map(|(index, slot)| {
            let stack = slot.as_ref()?;
            content.items.get(&stack.item)?.equip.as_ref()?;
            Some((index as u32, stack.item.clone()))
        });
        let mut any = false;
        for (index, item) in equippable {
            any = true;
            list.spawn((BagItem(index), icon_box(BOX_SIZE), tooltip(&item, "Click to equip it.")))
                .with_child(icon(&item));
        }
        if !any {
            list.spawn(text("Nothing you could wear.", 12.0, QUIET_COLOUR));
        }
    });
}

/// Clicking a worn item takes it off; clicking one in the bag puts it on.
fn change_on_click(
    worn: Query<(&Interaction, &WornSlot), Changed<Interaction>>,
    bag: Query<(&Interaction, &BagItem), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    for (interaction, slot) in &worn {
        if *interaction == Interaction::Pressed {
            to_server.write(ToServer(ClientMessage::Unequip { slot: slot.0 }));
        }
    }
    for (interaction, item) in &bag {
        if *interaction == Interaction::Pressed {
            to_server.write(ToServer(ClientMessage::Equip { slot: item.0 }));
        }
    }
}
