//! The bag: the player's inventory, as a grid of slots in a window on the
//! right, opened from the toolbar (see [`crate::ui::toolbar`]). The slots
//! scroll (with the mouse wheel) when there are more than fit, and the
//! bottom shows how full it is and the player's Miasma. Pointing at an item
//! shows what it is. While a shop is open, clicking an item asks to
//! sell one (see [`crate::ui::shop`]).
//!
//! The inventory itself belongs to the server, which sends it whenever it
//! changes ([`ServerMessage::Inventory`]). This is only a view of it.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use venture_protocol::{ClientMessage, ItemStack, ServerMessage};

use crate::content::ContentStore;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::shop::OpenShop;
use crate::ui::toolbar::OpenPanels;
use crate::ui::tooltip::Tooltip;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND};

/// Slots per row.
const COLUMNS: usize = 4;
/// Size of a slot, in pixels.
const SLOT_SIZE: f32 = 40.0;
/// Size of an item's icon in its slot, in pixels.
const ICON_SIZE: f32 = 32.0;
/// Space between slots, in pixels.
const GAP: f32 = 3.0;
/// Space around the grid, in pixels.
const PADDING: f32 = 8.0;
/// How far below the top of the screen the bag sits (under the toolbar), in pixels.
const TOP: f32 = 44.0;
/// How many rows of slots are in view at once (the rest scroll).
const VISIBLE_ROWS: usize = 7;
/// Width of the scrollbar, in pixels.
const SCROLLBAR_WIDTH: f32 = 5.0;
/// Colour of the bag's fullness.
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

/// Shows the bag.
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInventory>()
            .add_systems(Startup, spawn_bag)
            .add_systems(
                Update,
                (
                    record_inventory,
                    rebuild_slots.run_if(resource_changed::<PlayerInventory>),
                    fill_slots,
                    highlight_slots,
                    sell_on_click,
                    show_footer,
                    (scroll_slots, place_scroll_thumb).chain(),
                    show_when_open,
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

/// The bag window.
#[derive(Component)]
struct BagWindow;

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

fn spawn_bag(mut commands: Commands) {
    let grid_width = COLUMNS as f32 * SLOT_SIZE + (COLUMNS - 1) as f32 * GAP;
    let view_height = VISIBLE_ROWS as f32 * SLOT_SIZE + (VISIBLE_ROWS - 1) as f32 * GAP;
    let font = TextFont { font_size: FontSize::Px(13.0), ..default() };
    commands
        .spawn((
            BagWindow,
            // Blocks clicks (and the mouse wheel) on the world behind it.
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP),
                right: px(12),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::all(px(PADDING)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            Visibility::Hidden,
        ))
        .with_children(|bag| {
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
            // How full the bag is, and the player's money.
            bag.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
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

/// The mouse wheel over the bag scrolls its slots.
fn scroll_slots(
    mouse_scroll: Res<AccumulatedMouseScroll>,
    bag: Single<&Interaction, With<BagWindow>>,
    mut view: Single<(&mut ScrollPosition, &ComputedNode), With<SlotView>>,
) {
    if **bag == Interaction::None || mouse_scroll.delta.y == 0.0 {
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
    inventory: Res<PlayerInventory>,
    content: Res<ContentStore>,
    shop: Res<OpenShop>,
    new_slots: Query<(), Added<InventorySlot>>,
    mut slots: Query<(&InventorySlot, &Children, &mut Tooltip)>,
    mut icons: Query<(&mut ImageNode, &mut Visibility), With<SlotIcon>>,
    mut counts: Query<&mut Text, With<SlotCount>>,
) {
    if !inventory.is_changed() && !content.is_changed() && !shop.is_changed() && new_slots.is_empty() {
        return;
    }
    for (&InventorySlot(index), children, mut tooltip) in &mut slots {
        let stack = inventory.slots.get(index).and_then(Option::as_ref);
        let definition = stack.and_then(|stack| content.items.get(&stack.item));
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
                        Some(price) => body.push_str(&format!("\nSells for {price} coins.")),
                        None => body.push_str(&format!("\n{} won't buy this.", shop.name)),
                    }
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

/// While a shop is open, clicking an item asks to sell one.
fn sell_on_click(
    shop: Res<OpenShop>,
    inventory: Res<PlayerInventory>,
    slots: Query<(&Interaction, &InventorySlot), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    if shop.0.is_none() {
        return;
    }
    for (interaction, &InventorySlot(index)) in &slots {
        let holds_something = inventory.slots.get(index).is_some_and(Option::is_some);
        if *interaction == Interaction::Pressed && holds_something {
            to_server.write(ToServer(ClientMessage::Sell { slot: index as u32, count: 1 }));
        }
    }
}

fn show_when_open(panels: Res<OpenPanels>, mut bag: Single<&mut Visibility, With<BagWindow>>) {
    **bag = if panels.bag { Visibility::Inherited } else { Visibility::Hidden };
}

fn forget_inventory(mut inventory: ResMut<PlayerInventory>) {
    inventory.slots.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_shortened_to_fit() {
        assert_eq!(short_count(99_999), "99999");
        assert_eq!(short_count(123_456), "123K");
        assert_eq!(short_count(12_345_678), "12M");
    }
}
