//! The shop window: what an NPC's shop sells, beside the inventory.
//!
//! The server opens it ([`ServerMessage::ShopOpened`], after the NPC's
//! dialogue offers to trade) and closes it ([`ServerMessage::ShopClosed`],
//! when the player walks away). Clicking an item asks to buy one (five with
//! Shift); while it's open, clicking an item in the inventory asks to sell one (see
//! [`crate::ui::character_window::bag`]). The server decides, and says how it went in chat.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, ServerMessage, ShopStock};

use crate::content::ContentStore;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::theme::{Piece, Themed};
use crate::ui::tooltip::Tooltip;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER, spawn_close_button};

/// Items per row.
const COLUMNS: usize = 5;
/// Size of an item's box, in pixels.
const SLOT_WIDTH: f32 = 44.0;
/// Height of an item's box, with its price, in pixels.
const SLOT_HEIGHT: f32 = 58.0;
/// Size of an item's icon, in pixels.
const ICON_SIZE: f32 = 32.0;
/// Space between items, in pixels.
const GAP: f32 = 4.0;
/// Space around the window's contents, in pixels.
const PADDING: f32 = 10.0;
/// How far from the right edge the window sits, in pixels: just left of the character window.
const RIGHT: f32 = crate::ui::character_window::RIGHT + crate::ui::character_window::WINDOW_WIDTH + 10.0;
/// How many one Shift-click buys.
const SHIFT_COUNT: u32 = 5;
/// Colour of the shop's name.
const TITLE_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of prices.
const PRICE_COLOUR: Color = Color::srgb(1.0, 0.9, 0.35);
/// Colour of the hint at the bottom.
const HINT_COLOUR: Color = Color::srgb(0.7, 0.7, 0.66);
/// Colour of an item's box.
const SLOT_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.05);
/// Colour of an item's box under the mouse.
const SLOT_HOVER_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);

/// Shows shops.
pub struct ShopPlugin;

impl Plugin for ShopPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenShop>()
            .add_systems(Startup, spawn_window)
            .add_systems(Update, (record_shop, show_stock, buy_on_click, highlight_stock, close_on_click).chain())
            .add_systems(OnExit(ClientState::Playing), forget_shop);
    }
}

/// The shop the player has open, if any.
#[derive(Resource, Default)]
pub struct OpenShop(pub Option<ShopView>);

/// A shop, as the server described it.
pub struct ShopView {
    /// Its name.
    pub name: String,
    /// What it sells.
    pub stock: Vec<ShopStock>,
}

/// The window.
#[derive(Component)]
struct ShopWindow;

/// The shop's name.
#[derive(Component)]
struct ShopTitle;

/// The grid of items for sale.
#[derive(Component)]
struct StockGrid;

/// An item for sale, by its position in the stock.
#[derive(Component)]
struct StockSlot(u32);

/// The ✕ button.
#[derive(Component)]
struct CloseButton;

fn spawn_window(mut commands: Commands) {
    commands
        .spawn((
            ShopWindow,
            // Blocks clicks on the world behind it.
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(crate::ui::character_window::BOTTOM),
                right: px(RIGHT),
                // (Widths include the padding and the 1 pixel border.)
                width: px(COLUMNS as f32 * SLOT_WIDTH + (COLUMNS - 1) as f32 * GAP + 2.0 * (PADDING + 1.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(PADDING)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            Themed(Piece::Window),
            WINDOW_LAYER,
            Visibility::Hidden,
        ))
        .with_children(|window| {
            window.spawn((
                ShopTitle,
                Text::default(),
                TextFont { font_size: FontSize::Px(16.0), ..default() },
                TextColor(TITLE_COLOUR),
                TextShadow { offset: Vec2::splat(1.0), ..default() },
            ));
            window.spawn(crate::ui::character_window::divider());
            window.spawn((
                StockGrid,
                Node { flex_wrap: FlexWrap::Wrap, column_gap: px(GAP), row_gap: px(GAP), ..default() },
            ));
            window.spawn((
                Text::new("Click to buy (Shift: 5). Click things in your inventory to sell them."),
                TextFont { font_size: FontSize::Px(12.0), ..default() },
                TextColor(HINT_COLOUR),
            ));
            spawn_close_button(window, CloseButton);
        });
}

fn record_shop(mut events: MessageReader<FromServer>, mut shop: ResMut<OpenShop>) {
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::ShopOpened { name, stock, .. } => {
                shop.0 = Some(ShopView { name: name.clone(), stock: stock.clone() });
            }
            ServerMessage::ShopClosed => shop.0 = None,
            _ => {}
        }
    }
}

/// Shows or hides the window, and fills it with the shop's stock.
fn show_stock(
    mut commands: Commands,
    shop: Res<OpenShop>,
    content: Res<ContentStore>,
    window: Single<&mut Visibility, With<ShopWindow>>,
    mut title: Single<&mut Text, With<ShopTitle>>,
    grid: Single<Entity, With<StockGrid>>,
) {
    if !shop.is_changed() && !content.is_changed() {
        return;
    }
    let mut visibility = window.into_inner();
    let Some(shop) = &shop.0 else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Inherited;
    title.0.clone_from(&shop.name);
    let font = TextFont { font_size: FontSize::Px(12.0), ..default() };
    commands.entity(*grid).despawn_children().with_children(|grid| {
        for (index, stock) in shop.stock.iter().enumerate() {
            let tooltip = match content.items.get(&stock.item) {
                Some(item) => {
                    let fishing = item.fishing.as_ref().map_or(String::new(), |part| format!("\n{}", part.describe()));
                    Tooltip {
                        title: item.name.clone(),
                        body: format!("{}{fishing}\nCosts {} Miasma.", item.description, stock.price),
                    }
                }
                None => Tooltip { title: stock.item.clone(), body: format!("Costs {} Miasma.", stock.price) },
            };
            grid.spawn((
                StockSlot(index as u32),
                Button,
                tooltip,
                Node {
                    width: px(SLOT_WIDTH),
                    height: px(SLOT_HEIGHT),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    row_gap: px(2),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(SLOT_COLOUR),
                Themed(Piece::Slot),
            ))
            .with_children(|slot| {
                let icon = content.icon_for(&stock.item).unwrap_or_default();
                slot.spawn((ImageNode::new(icon), Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() }));
                slot.spawn((Text::new(stock.price.to_string()), font.clone(), TextColor(PRICE_COLOUR)));
            });
        }
    });
}

/// Asks to buy what the player clicked.
fn buy_on_click(
    keys: Res<ButtonInput<KeyCode>>,
    slots: Query<(&Interaction, &StockSlot), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for (interaction, &StockSlot(index)) in &slots {
        if *interaction == Interaction::Pressed {
            let count = if shift { SHIFT_COUNT } else { 1 };
            to_server.write(ToServer(ClientMessage::Buy { index, count }));
        }
    }
}

fn highlight_stock(mut slots: Query<(&Interaction, &mut BackgroundColor), (With<StockSlot>, Changed<Interaction>)>) {
    for (interaction, mut background) in &mut slots {
        background.0 = if *interaction == Interaction::None { SLOT_COLOUR } else { SLOT_HOVER_COLOUR };
    }
}

/// The ✕ closes the shop.
fn close_on_click(
    close: Query<&Interaction, (With<CloseButton>, Changed<Interaction>)>,
    mut shop: ResMut<OpenShop>,
    mut to_server: MessageWriter<ToServer>,
) {
    if close.iter().any(|interaction| *interaction == Interaction::Pressed) && shop.0.is_some() {
        to_server.write(ToServer(ClientMessage::CloseShop));
        shop.0 = None;
    }
}

fn forget_shop(mut shop: ResMut<OpenShop>) {
    shop.0 = None;
}
