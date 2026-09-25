//! The bag: the player's inventory, as a grid of slots in a window on the
//! right. **I** (or the "Bag" button) opens and closes it; pointing at an
//! item shows what it is. While a shop is open, the bag opens too, and
//! clicking an item asks to sell one (see [`crate::ui::shop`]).
//!
//! The inventory itself belongs to the server, which sends it whenever it
//! changes ([`ServerMessage::Inventory`]). This is only a view of it.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, ItemStack, ServerMessage};

use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::shop::OpenShop;
use crate::ui::text_input::KeyboardFocus;
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
                    (toggle_bag, show_only_while_playing).chain(),
                )
                    .chain(),
            )
            .add_systems(OnExit(ClientState::Playing), forget_inventory);
    }
}

/// What the player carries, slot by slot, as the server last said.
#[derive(Resource, Default)]
pub struct PlayerInventory(pub Vec<Option<ItemStack>>);

/// Whether the bag window is open.
#[derive(Component)]
struct BagWindow {
    open: bool,
}

/// The grid the slots go in.
#[derive(Component)]
struct SlotGrid;

/// One slot, by its position in the inventory.
#[derive(Component)]
pub struct InventorySlot(pub usize);

/// A slot's icon.
#[derive(Component)]
struct SlotIcon;

/// A slot's count (for stacks).
#[derive(Component)]
struct SlotCount;

/// The button that opens and closes the bag.
#[derive(Component)]
struct BagButton;

fn spawn_bag(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(12),
            right: px(12),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(6),
            ..default()
        })
        .with_children(|column| {
            column
                .spawn((
                    BagButton,
                    Button,
                    Node {
                        padding: UiRect::axes(px(10), px(4)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    BackgroundColor(WINDOW_BACKGROUND),
                    BorderColor::all(BORDER_COLOUR),
                ))
                .with_child((Text::new("Bag (I)"), font, TextColor(Color::srgb(0.96, 0.9, 0.75))));
            column.spawn((
                BagWindow { open: false },
                // Blocks clicks on the world behind it.
                Interaction::default(),
                SlotGrid,
                Node {
                    // (Widths include the padding and the 1 pixel border.)
                    width: px(COLUMNS as f32 * SLOT_SIZE + (COLUMNS - 1) as f32 * GAP + 2.0 * (PADDING + 1.0)),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(GAP),
                    row_gap: px(GAP),
                    padding: UiRect::all(px(PADDING)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(WINDOW_BACKGROUND),
                BorderColor::all(BORDER_COLOUR),
                Visibility::Hidden,
            ));
        });
}

fn record_inventory(mut events: MessageReader<FromServer>, mut inventory: ResMut<PlayerInventory>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Inventory { slots } = message {
            inventory.0.clone_from(slots);
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
    if slots.iter().count() == inventory.0.len() {
        return;
    }
    let font = TextFont { font_size: FontSize::Px(12.0), ..default() };
    commands.entity(*grid).despawn_children().with_children(|grid| {
        for index in 0..inventory.0.len() {
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
        let stack = inventory.0.get(index).and_then(Option::as_ref);
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
        let holds_something = inventory.0.get(index).is_some_and(Option::is_some);
        if *interaction == Interaction::Pressed && holds_something {
            to_server.write(ToServer(ClientMessage::Sell { slot: index as u32, count: 1 }));
        }
    }
}

/// **I** or the "Bag" button opens and closes the bag (unless the player is
/// typing). A shop opening opens it too.
fn toggle_bag(
    mut events: MessageReader<FromServer>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    button: Query<&Interaction, (With<BagButton>, Changed<Interaction>)>,
    mut bag: Single<&mut BagWindow>,
    dev: Res<DevSettings>,
    mut opened_for_dev: Local<bool>,
) {
    if dev.open_bag && !*opened_for_dev {
        bag.open = true;
        *opened_for_dev = true;
    }
    if events.read().any(|FromServer(message)| matches!(message, ServerMessage::ShopOpened { .. })) {
        bag.open = true;
    }
    let pressed_key = focus.0.is_none() && keys.just_pressed(KeyCode::KeyI);
    let clicked = button.iter().any(|interaction| *interaction == Interaction::Pressed);
    if pressed_key || clicked {
        bag.open = !bag.open;
    }
}

fn show_only_while_playing(
    state: Res<State<ClientState>>,
    mut bag: Single<(&BagWindow, &mut Visibility), Without<BagButton>>,
    mut button: Single<&mut Visibility, With<BagButton>>,
) {
    let playing = *state.get() == ClientState::Playing;
    let (window, ref mut visibility) = *bag;
    let shown = |show: bool| if show { Visibility::Inherited } else { Visibility::Hidden };
    **visibility = shown(playing && window.open);
    **button = shown(playing);
}

fn forget_inventory(mut inventory: ResMut<PlayerInventory>) {
    inventory.0.clear();
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
