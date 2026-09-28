//! The inventory tab: the player's inventory, as a grid of slots. The slots scroll
//! (with the mouse wheel) when there are more than fit, and the bottom shows
//! how full it is and the player's Miasma. Pointing at an item shows what it
//! is. Clicking an item equips it, if it can be equipped, or uses it (eats
//! it), if it can be used; while a shop is open, clicking asks to sell one
//! instead (see [`crate::ui::shop`]). Items that can be used can be dragged
//! onto the hotbar, which then uses the last one in the bag
//! ([`UseItemKind`]).
//!
//! The inventory itself belongs to the server, which sends it whenever it
//! changes ([`ServerMessage::Inventory`]). This is only a view of it.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use venture_content::ItemEffect;
use venture_protocol::{ClientMessage, HotbarEntry, ItemStack, ServerMessage};
use venture_rpg::Healing;

use crate::content::ContentStore;
use crate::fishing::traps::SetTrapFromBag;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::character_window::{CharacterWindow, OpenTab, Pages, Tab};
use crate::ui::chat::SystemMessage;
use crate::ui::hotbar::{DragOnly, Draggable};
use crate::ui::shop::OpenShop;
use crate::ui::theme::{Piece, Themed};
use crate::ui::tooltip::Tooltip;

/// Slots per row.
const COLUMNS: usize = 6;
/// Size of a slot, in pixels.
const SLOT_SIZE: f32 = 40.0;
/// Size of an item's icon in its slot, in pixels.
const ICON_SIZE: f32 = 32.0;
/// Space between slots, in pixels.
const GAP: f32 = 3.0;
/// How many rows of slots are in view at once (the rest scroll).
const VISIBLE_ROWS: usize = 7;
/// Width of the scrollbar, in pixels.
const SCROLLBAR_WIDTH: f32 = 5.0;
/// Colour of the inventory's fullness.
const FOOTER_COLOUR: Color = Color::srgb(0.75, 0.75, 0.7);
/// Colour of the Miasma amount.
const MIASMA_COLOUR: Color = Color::srgb(0.8, 0.65, 0.95);
/// The content icon shown by the Miasma amount.
const MIASMA_ICON: &str = "miasma.png";
/// Colour of the counts on stacks.
const COUNT_COLOUR: Color = Color::srgb(1.0, 0.9, 0.35);
/// Colour of an empty slot.
const SLOT_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.05);
/// Colour of a slot under the mouse.
const SLOT_HOVER_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
/// How far the mouse may move between press and release for a click, in pixels.
const CLICK_DISTANCE: f32 = 5.0;

/// Shows the inventory tab.
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInventory>()
            .add_message::<UseItemKind>()
            .add_systems(Startup, spawn_inventory.after(super::spawn_window))
            .add_systems(
                Update,
                (
                    record_inventory,
                    rebuild_slots.run_if(resource_changed::<PlayerInventory>),
                    fill_slots,
                    highlight_slots,
                    use_on_click,
                    use_item_kinds,
                    show_footer,
                    (scroll_slots, place_scroll_thumb).chain(),
                )
                    .chain(),
            )
            .add_systems(OnExit(ClientState::Playing), forget_inventory);
    }
}

/// What the player carries, as the server last said.
#[derive(Resource, Default)]
pub struct PlayerInventory {
    /// Each slot: empty, or holding some of one item.
    pub slots: Vec<Option<ItemStack>>,
    /// Their money.
    pub miasma: u64,
}

impl PlayerInventory {
    /// How many of an item the player has.
    pub fn count(&self, item: &str) -> u32 {
        self.slots.iter().flatten().filter(|stack| stack.item == item).map(|stack| stack.count).sum()
    }
}

/// Use one of a kind of item (from the hotbar): the last one in the bag.
#[derive(Message, Clone, Debug)]
pub struct UseItemKind(pub String);

/// The scrolling view of the slots.
#[derive(Component)]
struct SlotView;

/// The grid the slots go in.
#[derive(Component)]
struct SlotGrid;

/// The scrollbar's thumb.
#[derive(Component)]
struct ScrollThumb;

/// How many slots are used ("12/50").
#[derive(Component)]
struct FullnessText;

/// The Miasma icon.
#[derive(Component)]
struct MiasmaIcon;

/// How much Miasma the player has.
#[derive(Component)]
struct MiasmaText;

/// One slot, by its position in the inventory.
#[derive(Component)]
pub struct InventorySlot(pub usize);

/// A slot's icon.
#[derive(Component)]
struct SlotIcon;

/// A slot's count (for stacks).
#[derive(Component)]
struct SlotCount;

fn spawn_inventory(mut commands: Commands, pages: Res<Pages>) {
    let grid_width = COLUMNS as f32 * SLOT_SIZE + (COLUMNS - 1) as f32 * GAP;
    let view_height = VISIBLE_ROWS as f32 * SLOT_SIZE + (VISIBLE_ROWS - 1) as f32 * GAP;
    let font = TextFont { font_size: FontSize::Px(13.0), ..default() };
    commands.entity(pages.of(Tab::Inventory)).with_children(|bag| {
        bag.spawn(Node { column_gap: px(5), ..default() }).with_children(|row| {
            // The slots scroll within a view a few rows tall.
            row.spawn((
                SlotView,
                ScrollPosition::default(),
                Node {
                    width: px(grid_width),
                    max_height: px(view_height),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_child((
                SlotGrid,
                Node {
                    width: px(grid_width),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(GAP),
                    row_gap: px(GAP),
                    ..default()
                },
            ));
            row.spawn((
                Node {
                    width: px(SCROLLBAR_WIDTH),
                    height: px(view_height),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
            ))
            .with_child((
                ScrollThumb,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.85, 0.7, 0.4, 0.6)),
            ));
        });
        // How full the inventory is, and the player's money.
        bag.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() })
            .with_children(|footer| {
                footer.spawn((FullnessText, Text::default(), font.clone(), TextColor(FOOTER_COLOUR)));
                footer.spawn(Node { align_items: AlignItems::Center, column_gap: px(4), ..default() }).with_children(
                    |money| {
                        money.spawn((
                            MiasmaIcon,
                            ImageNode::default(),
                            Node { width: px(16), height: px(16), ..default() },
                        ));
                        money.spawn((MiasmaText, Text::default(), font, TextColor(MIASMA_COLOUR)));
                    },
                );
            });
    });
}

/// Shows how many slots are used, and the player's Miasma.
fn show_footer(
    inventory: Res<PlayerInventory>,
    content: Res<ContentStore>,
    mut fullness: Single<&mut Text, (With<FullnessText>, Without<MiasmaText>)>,
    mut miasma: Single<&mut Text, (With<MiasmaText>, Without<FullnessText>)>,
    mut icon: Single<&mut ImageNode, With<MiasmaIcon>>,
) {
    if !inventory.is_changed() && !content.is_changed() {
        return;
    }
    let used = inventory.slots.iter().flatten().count();
    fullness.0 = format!("{used}/{}", inventory.slots.len());
    miasma.0 = format!("{} Miasma", inventory.miasma);
    if let Some(image) = content.icons.get(MIASMA_ICON) {
        icon.image = image.clone();
    }
}

/// The mouse wheel over the inventory scrolls its slots.
fn scroll_slots(
    mouse_scroll: Res<AccumulatedMouseScroll>,
    open: Res<OpenTab>,
    window: Single<&Interaction, With<CharacterWindow>>,
    mut view: Single<(&mut ScrollPosition, &ComputedNode), With<SlotView>>,
) {
    if open.0 != Some(Tab::Inventory) || **window == Interaction::None || mouse_scroll.delta.y == 0.0 {
        return;
    }
    let lines = match mouse_scroll.unit {
        MouseScrollUnit::Line => mouse_scroll.delta.y,
        MouseScrollUnit::Pixel => mouse_scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    let (ref mut position, computed) = *view;
    let scale = computed.inverse_scale_factor();
    let furthest = ((computed.content_size().y - computed.size().y) * scale).max(0.0);
    position.0.y = (position.0.y - lines * (SLOT_SIZE + GAP)).clamp(0.0, furthest);
}

/// Sizes and places the scrollbar's thumb to match what's in view.
fn place_scroll_thumb(
    view: Single<(&ScrollPosition, &ComputedNode), With<SlotView>>,
    mut thumb: Single<(&mut Node, &mut Visibility), With<ScrollThumb>>,
) {
    let (position, computed) = *view;
    let content = computed.content_size().y * computed.inverse_scale_factor();
    let visible = computed.size().y * computed.inverse_scale_factor();
    let (ref mut node, ref mut visibility) = *thumb;
    if content <= visible + 1.0 {
        **visibility = Visibility::Hidden;
        return;
    }
    **visibility = Visibility::Inherited;
    let shown = visible / content;
    let scrolled = position.0.y / (content - visible);
    node.height = percent(shown * 100.0);
    node.top = percent(scrolled.clamp(0.0, 1.0) * (1.0 - shown) * 100.0);
}

fn record_inventory(mut events: MessageReader<FromServer>, mut inventory: ResMut<PlayerInventory>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Inventory { slots, miasma } = message {
            inventory.slots.clone_from(slots);
            inventory.miasma = *miasma;
        }
    }
}

/// Makes one slot for each inventory slot (the number only changes if the
/// server's inventory size does).
fn rebuild_slots(
    mut commands: Commands,
    inventory: Res<PlayerInventory>,
    grid: Single<Entity, With<SlotGrid>>,
    slots: Query<(), With<InventorySlot>>,
) {
    if slots.iter().count() == inventory.slots.len() {
        return;
    }
    let font = TextFont { font_size: FontSize::Px(12.0), ..default() };
    commands.entity(*grid).despawn_children().with_children(|grid| {
        for index in 0..inventory.slots.len() {
            grid.spawn((
                InventorySlot(index),
                Button,
                Tooltip::default(),
                Node {
                    width: px(SLOT_SIZE),
                    height: px(SLOT_SIZE),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(SLOT_COLOUR),
                Themed(Piece::Slot),
            ))
            .with_children(|slot| {
                slot.spawn((
                    SlotIcon,
                    ImageNode::default(),
                    Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                ));
                slot.spawn((
                    SlotCount,
                    Text::default(),
                    font.clone(),
                    TextColor(COUNT_COLOUR),
                    TextShadow { offset: Vec2::splat(1.0), ..default() },
                    Node { position_type: PositionType::Absolute, top: px(1), left: px(3), ..default() },
                ));
            });
        }
    });
}

/// Shows each slot's item: its icon, its count if it's a stack, and a
/// tooltip (with what it sells for, while a shop is open). Runs when any of
/// those change, or new slots appear.
fn fill_slots(
    mut commands: Commands,
    inventory: Res<PlayerInventory>,
    content: Res<ContentStore>,
    shop: Res<OpenShop>,
    new_slots: Query<(), Added<InventorySlot>>,
    mut slots: Query<(Entity, &InventorySlot, &Children, &mut Tooltip)>,
    mut icons: Query<(&mut ImageNode, &mut Visibility), With<SlotIcon>>,
    mut counts: Query<&mut Text, With<SlotCount>>,
) {
    if !inventory.is_changed() && !content.is_changed() && !shop.is_changed() && new_slots.is_empty() {
        return;
    }
    for (slot, &InventorySlot(index), children, mut tooltip) in &mut slots {
        let stack = inventory.slots.get(index).and_then(Option::as_ref);
        let definition = stack.and_then(|stack| content.items.get(&stack.item));
        // Things that can be used can go on the hotbar (as a kind of item).
        match definition.filter(|item| item.on_use != ItemEffect::Nothing) {
            Some(item) => commands.entity(slot).insert((Draggable(HotbarEntry::Item(item.id.clone())), DragOnly)),
            None => commands.entity(slot).remove::<(Draggable, DragOnly)>(),
        };
        for child in children {
            if let Ok((mut image, mut visibility)) = icons.get_mut(*child) {
                match stack.and_then(|stack| content.icon_for(&stack.item)) {
                    Some(icon) => {
                        image.image = icon;
                        *visibility = Visibility::Inherited;
                    }
                    None => *visibility = Visibility::Hidden,
                }
            }
            if let Ok(mut text) = counts.get_mut(*child) {
                text.0 = match stack {
                    Some(stack) if stack.count > 1 => short_count(stack.count),
                    // An item the client doesn't know yet still shows that something's there.
                    Some(_) if definition.is_none() => "?".to_string(),
                    _ => String::new(),
                };
            }
        }
        *tooltip = match (stack, definition) {
            (Some(stack), Some(item)) => {
                let title =
                    if stack.count > 1 { format!("{} ({})", item.name, stack.count) } else { item.name.clone() };
                let mut body = item.description.clone();
                if let Some(shop) = &shop.0 {
                    match item.sell_price() {
                        Some(price) => body.push_str(&format!("\nSells for {price} Miasma.")),
                        None => body.push_str(&format!("\n{} won't buy this.", shop.name)),
                    }
                } else if item.equip.is_some() {
                    body.push_str("\nClick to equip.");
                } else if let ItemEffect::Heal(healing) = item.on_use {
                    let amount = match healing {
                        Healing::Flat(amount) => format!("{amount} health"),
                        Healing::Percent(percent) => format!("{percent}% of your health"),
                    };
                    body.push_str(&format!("\nHeals {amount}. Click to use, or drag it to the hotbar."));
                }
                Tooltip { title, body }
            }
            (Some(stack), None) => Tooltip { title: stack.item.clone(), body: String::new() },
            (None, _) => Tooltip::default(),
        };
    }
}

/// A count short enough for a slot: 1234, 123K, 12M.
fn short_count(count: u32) -> String {
    match count {
        0..100_000 => count.to_string(),
        100_000..10_000_000 => format!("{}K", count / 1000),
        _ => format!("{}M", count / 1_000_000),
    }
}

fn highlight_slots(
    mut slots: Query<(&Interaction, &mut BackgroundColor), (With<InventorySlot>, Changed<Interaction>)>,
) {
    for (interaction, mut background) in &mut slots {
        background.0 = if *interaction == Interaction::None { SLOT_COLOUR } else { SLOT_HOVER_COLOUR };
    }
}

/// Clicking an item sells one while a shop is open, and otherwise equips it
/// (if it can be equipped), sets it (a fishing trap) or uses it (if it can be used). A click is a
/// press and release on the same slot without moving (moving is a drag, to
/// the hotbar).
fn use_on_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    shop: Res<OpenShop>,
    inventory: Res<PlayerInventory>,
    content: Res<ContentStore>,
    slots: Query<(&Interaction, &InventorySlot)>,
    mut pressed: Local<Option<(usize, Vec2)>>,
    mut to_server: MessageWriter<ToServer>,
    mut traps: MessageWriter<SetTrapFromBag>,
) {
    let cursor = window.cursor_position();
    if mouse.just_pressed(MouseButton::Left) {
        let slot = slots.iter().find(|(interaction, _)| **interaction == Interaction::Pressed);
        *pressed = slot.zip(cursor).map(|((_, &InventorySlot(index)), cursor)| (index, cursor));
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some((index, start)) = pressed.take() else { return };
    let still_over = slots.iter().any(|(interaction, slot)| slot.0 == index && *interaction != Interaction::None);
    if !still_over || cursor.is_none_or(|cursor| cursor.distance(start) > CLICK_DISTANCE) {
        return;
    }
    let Some(stack) = inventory.slots.get(index).and_then(Option::as_ref) else { return };
    let slot = index as u32;
    if shop.0.is_some() {
        to_server.write(ToServer(ClientMessage::Sell { slot, count: 1 }));
    } else if let Some(item) = content.items.get(&stack.item) {
        if item.equip.is_some() {
            to_server.write(ToServer(ClientMessage::Equip { slot }));
        } else if matches!(item.fishing, Some(venture_content::FishingPart::Trap { .. })) {
            traps.write(SetTrapFromBag(stack.item.clone()));
        } else if item.on_use != ItemEffect::Nothing {
            to_server.write(ToServer(ClientMessage::UseItem { slot }));
        }
    }
}

/// Uses the last one of a kind of item in the bag, or says there are none left.
fn use_item_kinds(
    mut requests: MessageReader<UseItemKind>,
    inventory: Res<PlayerInventory>,
    content: Res<ContentStore>,
    mut to_server: MessageWriter<ToServer>,
    mut log: MessageWriter<SystemMessage>,
) {
    for UseItemKind(item) in requests.read() {
        match last_slot_of(&inventory, item) {
            Some(slot) => {
                to_server.write(ToServer(ClientMessage::UseItem { slot }));
            }
            None => {
                let name = content.items.get(item).map_or(item.as_str(), |def| def.name.as_str());
                log.write(SystemMessage(format!("You don't have any {name} left.")));
            }
        }
    }
}

/// The last slot holding an item, if any does.
fn last_slot_of(inventory: &PlayerInventory, item: &str) -> Option<u32> {
    let slot = inventory.slots.iter().rposition(|stack| stack.as_ref().is_some_and(|stack| stack.item == item))?;
    Some(slot as u32)
}

fn forget_inventory(mut inventory: ResMut<PlayerInventory>) {
    inventory.slots.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hotbar_uses_the_last_of_a_kind() {
        let stack = |item: &str, count| Some(ItemStack { item: item.to_string(), count });
        let inventory = PlayerInventory {
            slots: vec![stack("bread", 1), None, stack("pie", 2), stack("bread", 3), None],
            miasma: 0,
        };
        assert_eq!(last_slot_of(&inventory, "bread"), Some(3));
        assert_eq!(inventory.count("bread"), 4);
        assert_eq!(last_slot_of(&inventory, "roll"), None);
        assert_eq!(inventory.count("roll"), 0);
    }

    #[test]
    fn counts_are_shortened_to_fit() {
        assert_eq!(short_count(99_999), "99999");
        assert_eq!(short_count(123_456), "123K");
        assert_eq!(short_count(12_345_678), "12M");
    }
}
