//! The equipment (gear) tab: a slot for each part of the body, laid out like
//! a person facing you: head, body and legs down the middle, the dominant
//! hand on the left and the other hand on the right. Tooltips say what each
//! slot is and what's in it; clicking a filled slot takes it off, into the
//! inventory (items are put on from the inventory tab). The server does the
//! swapping, and says how it went in chat.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, EquipSlot};

use crate::content::ContentStore;
use crate::network::ToServer;
use crate::ui::character_window::{Pages, QUIET_COLOUR, Sheet, Tab, icon_box, text};
use crate::ui::tooltip::Tooltip;

/// Size of a slot, in pixels.
const BOX_SIZE: f32 = 48.0;
/// Size of an icon in its slot, in pixels.
const ICON_SIZE: f32 = 38.0;
/// Space between slots, in pixels.
const GAP: f32 = 10.0;
/// How faded an empty slot is.
const EMPTY_TINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.0);
/// The slots, row by row (`None` is a gap), like a person facing you.
const LAYOUT: [[Option<EquipSlot>; 3]; 3] = [
    [None, Some(EquipSlot::Head), None],
    [Some(EquipSlot::DominantHand), Some(EquipSlot::Body), Some(EquipSlot::NonDominantHand)],
    [None, Some(EquipSlot::Legs), None],
];

/// Shows the equipment tab.
pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (show_equipment, take_off_on_click));
    }
}

/// A slot's box.
#[derive(Component)]
struct SlotBox(EquipSlot);

/// A slot's icon.
#[derive(Component)]
struct SlotIcon(EquipSlot);

/// What the equipment adds up to.
#[derive(Component)]
struct Summary;

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Equipment)).with_children(|page| {
        page.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(GAP),
            padding: UiRect::vertical(px(8)),
            ..default()
        })
        .with_children(|body| {
            for row in LAYOUT {
                body.spawn(Node { column_gap: px(GAP), ..default() }).with_children(|line| {
                    for cell in row {
                        let Some(slot) = cell else {
                            line.spawn(Node { width: px(BOX_SIZE), height: px(BOX_SIZE), ..default() });
                            continue;
                        };
                        line.spawn((SlotBox(slot), icon_box(BOX_SIZE))).with_child((
                            SlotIcon(slot),
                            ImageNode::default().with_color(EMPTY_TINT),
                            Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                        ));
                    }
                });
            }
        });
        page.spawn((Summary, text("", 12.0, QUIET_COLOUR), Node { align_self: AlignSelf::Center, ..default() }));
    });
}

/// Fills the slots in (icons and tooltips) when the equipment or content changes.
fn show_equipment(
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    mut boxes: Query<(&SlotBox, &mut Tooltip)>,
    mut icons: Query<(&SlotIcon, &mut ImageNode)>,
    mut summary: Single<&mut Text, With<Summary>>,
) {
    if !sheet.is_changed() && !content.is_changed() {
        return;
    }
    let Some(sheet) = &sheet.0 else { return };
    let worn = |slot: EquipSlot| sheet.equipment.iter().find(|(filled, _)| *filled == slot).map(|(_, item)| item);
    for (SlotBox(slot), mut tooltip) in &mut boxes {
        *tooltip = match worn(*slot) {
            Some(item) => match content.items.get(item) {
                Some(def) => Tooltip {
                    title: def.name.clone(),
                    body: format!("{}\n{}\nClick to take it off.", slot.name(), def.description),
                },
                None => Tooltip { title: item.clone(), body: slot.name().to_string() },
            },
            None => Tooltip { title: slot.name().to_string(), body: "Nothing".to_string() },
        };
    }
    for (SlotIcon(slot), mut image) in &mut icons {
        match worn(*slot).and_then(|item| content.icon_for(item)) {
            Some(icon) => {
                image.image = icon;
                image.color = Color::WHITE;
            }
            None => image.color = EMPTY_TINT,
        }
    }
    summary.0 = format!("Armour class {}, damage {}", sheet.armour_class, sheet.damage);
}

/// Clicking a filled slot takes off what's in it.
fn take_off_on_click(
    sheet: Res<Sheet>,
    boxes: Query<(&Interaction, &SlotBox), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    let Some(sheet) = &sheet.0 else { return };
    for (interaction, SlotBox(slot)) in &boxes {
        if *interaction == Interaction::Pressed && sheet.equipment.iter().any(|(filled, _)| filled == slot) {
            to_server.write(ToServer(ClientMessage::Unequip { slot: *slot }));
        }
    }
}
