//! The character window: everything about the player's own character, in
//! tabs. Each tab has a key (see [`Tab::key`]): Stats (**C**), Skills (**K**),
//! Abilities (**L**), Bag (**B**), Gear (**G**), Quests (**J**) and Emotes
//! (**O**). Pressing a tab's key opens the window on that tab, or closes it
//! if it's already showing. (Tabs' first letters are used where they're
//! free; W, A, S, D, Q, E, R and F move the character and camera.)
//!
//! The tabs are icons (from the theme), with the open tab's name as the
//! window's title.
//!
//! This module is the window itself (the tab bar, which tab is open, and the
//! latest [`CharacterSheet`] from the server). Each tab is its own module,
//! which fills in its page.

pub mod abilities;
pub mod bag;
pub mod emotes;
pub mod equipment;
pub mod quests;
pub mod skills;
pub mod stats;

use std::collections::HashMap;

use bevy::prelude::*;
use venture_protocol::{CharacterSheet, ServerMessage};

use crate::dev::settings::DevSettings;
use crate::network::{ClientState, FromServer};
use crate::ui::text_input::KeyboardFocus;
use crate::ui::theme::{Piece, Theme, Themed};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER, spawn_close_button, window_frame};

/// Width of the window, in pixels.
pub const WINDOW_WIDTH: f32 = 340.0;
/// Size of a tab, in pixels.
const TAB_SIZE: f32 = 36.0;
/// Size of a tab's icon, in pixels.
const TAB_ICON_SIZE: f32 = 26.0;
/// How far below the top of the screen the window sits (under the toolbar), in pixels.
const TOP: f32 = 50.0;
/// Colour of the open tab's label.
const OPEN_TAB_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of the other tabs' labels.
const TAB_COLOUR: Color = Color::srgb(0.62, 0.58, 0.5);
/// Colour of section headings within pages.
pub const HEADING_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of ordinary text within pages.
pub const TEXT_COLOUR: Color = Color::srgb(0.85, 0.85, 0.82);
/// Colour of quieter text within pages.
pub const QUIET_COLOUR: Color = Color::srgb(0.6, 0.6, 0.56);
/// Background of a clickable box (a slot, an ability) within pages.
pub const BOX_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.05);
/// Background of a clickable box under the mouse.
pub const BOX_HOVER_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);

/// Shows the character window, and the tabs in it.
pub struct CharacterWindowPlugin;

impl Plugin for CharacterWindowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenTab>()
            .init_resource::<Sheet>()
            .add_systems(Startup, spawn_window)
            .add_systems(Update, (record_sheet, switch_tabs, show_open_tab, highlight_boxes, show_tab_icons).chain())
            .add_systems(OnExit(ClientState::Playing), (close_window, forget_sheet))
            .add_plugins((
                stats::StatsPlugin,
                skills::SkillsPlugin,
                abilities::AbilitiesPlugin,
                bag::BagPlugin,
                equipment::EquipmentPlugin,
                quests::QuestsPlugin,
                emotes::EmotesPlugin,
            ));
    }
}

/// A tab of the character window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    /// Name, class, level, stats.
    Stats,
    /// Gathering and making skills.
    Skills,
    /// Actions and abilities.
    Abilities,
    /// The inventory.
    Bag,
    /// What's worn.
    Equipment,
    /// The quest journal.
    Quests,
    /// Gestures.
    Emotes,
}

impl Tab {
    /// Every tab, in the order they're shown.
    pub const ALL: [Tab; 7] =
        [Tab::Stats, Tab::Skills, Tab::Abilities, Tab::Bag, Tab::Equipment, Tab::Quests, Tab::Emotes];

    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Stats => "Stats",
            Tab::Skills => "Skills",
            Tab::Abilities => "Abilities",
            Tab::Bag => "Bag",
            Tab::Equipment => "Gear",
            Tab::Quests => "Quests",
            Tab::Emotes => "Emotes",
        }
    }

    /// The key that opens it.
    pub fn key(self) -> KeyCode {
        match self {
            Tab::Stats => KeyCode::KeyC,
            Tab::Skills => KeyCode::KeyK,
            Tab::Abilities => KeyCode::KeyL,
            Tab::Bag => KeyCode::KeyB,
            Tab::Equipment => KeyCode::KeyG,
            Tab::Quests => KeyCode::KeyJ,
            Tab::Emotes => KeyCode::KeyO,
        }
    }

    /// The key's name, for hints.
    pub fn key_name(self) -> &'static str {
        match self {
            Tab::Stats => "C",
            Tab::Skills => "K",
            Tab::Abilities => "L",
            Tab::Bag => "B",
            Tab::Equipment => "G",
            Tab::Quests => "J",
            Tab::Emotes => "O",
        }
    }

    /// Its name in the theme's tab icons and in `VENTURE_OPEN` (see [`DevSettings`]).
    pub fn id(self) -> &'static str {
        match self {
            Tab::Stats => "stats",
            Tab::Skills => "skills",
            Tab::Abilities => "abilities",
            Tab::Bag => "bag",
            Tab::Equipment => "gear",
            Tab::Quests => "quests",
            Tab::Emotes => "emotes",
        }
    }
}

/// Which tab is open, if the window is open at all.
#[derive(Resource, Default)]
pub struct OpenTab(pub Option<Tab>);

impl OpenTab {
    /// Opens the window on a tab, or closes it if that tab is already showing.
    pub fn toggle(&mut self, tab: Tab) {
        self.0 = if self.0 == Some(tab) { None } else { Some(tab) };
    }
}

/// The player's character sheet, as the server last sent it.
#[derive(Resource, Default)]
pub struct Sheet(pub Option<CharacterSheet>);

/// The window.
#[derive(Component)]
pub struct CharacterWindow;

/// A tab's button in the tab bar.
#[derive(Component)]
struct TabButton(Tab);

/// A tab's page. Tab modules fill theirs in (see [`Pages`]).
#[derive(Component)]
struct Page(Tab);

/// The ✕ button.
#[derive(Component)]
struct CloseButton;

/// The window's title (the open tab's name).
#[derive(Component)]
struct Title;

/// A tab's icon.
#[derive(Component)]
struct TabIcon(Tab);

/// A tab's short name, shown when the theme has no icon for it.
#[derive(Component)]
struct TabLabel(Tab);

/// Each tab's page entity, for its module to fill in at startup.
#[derive(Resource)]
pub struct Pages(HashMap<Tab, Entity>);

impl Pages {
    /// A tab's page.
    pub fn of(&self, tab: Tab) -> Entity {
        self.0[&tab]
    }
}

/// A clickable box within a page (a slot, an ability, a button), lit up under the mouse.
#[derive(Component)]
pub struct HoverBox;

/// Makes the window: a header with the title, the tabs, and a page for each tab.
pub fn spawn_window(mut commands: Commands) {
    let mut pages = HashMap::new();
    commands
        .spawn((
            CharacterWindow,
            // Blocks clicks (and the mouse wheel) on the world behind it.
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP),
                right: px(12),
                width: px(WINDOW_WIDTH),
                ..window_frame()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            Themed(Piece::Window),
            WINDOW_LAYER,
            Visibility::Hidden,
        ))
        .with_children(|window| {
            window
                .spawn((
                    Node {
                        justify_content: JustifyContent::Center,
                        padding: UiRect::axes(px(8), px(5)),
                        margin: UiRect::bottom(px(2)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    Themed(Piece::Header),
                ))
                .with_children(|header| {
                    header.spawn((
                        Title,
                        Text::default(),
                        TextFont { font_size: FontSize::Px(15.0), ..default() },
                        TextColor(OPEN_TAB_COLOUR),
                        TextShadow { offset: Vec2::splat(1.0), ..default() },
                    ));
                    spawn_close_button(header, CloseButton);
                });
            window
                .spawn(Node { column_gap: px(3), justify_content: JustifyContent::Center, ..default() })
                .with_children(|bar| {
                    for tab in Tab::ALL {
                        bar.spawn((
                            TabButton(tab),
                            Button,
                            crate::ui::tooltip::Tooltip {
                                title: tab.label().to_string(),
                                body: format!("Key: {}", tab.key_name()),
                            },
                            Node {
                                width: px(TAB_SIZE),
                                height: px(TAB_SIZE),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border_radius: BorderRadius::top(px(4)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            Themed(Piece::Tab),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                TabIcon(tab),
                                ImageNode::default(),
                                Node { width: px(TAB_ICON_SIZE), height: px(TAB_ICON_SIZE), ..default() },
                                Visibility::Hidden,
                            ));
                            button.spawn((
                                TabLabel(tab),
                                Text::new(&tab.label()[..3]),
                                TextFont { font_size: FontSize::Px(11.0), ..default() },
                                TextColor(TAB_COLOUR),
                                Node { position_type: PositionType::Absolute, ..default() },
                            ));
                        });
                    }
                });
            for tab in Tab::ALL {
                let page = window
                    .spawn((
                        Page(tab),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6),
                            padding: UiRect::new(px(4), px(4), px(8), px(2)),
                            ..default()
                        },
                    ))
                    .id();
                pages.insert(tab, page);
            }
        });
    commands.insert_resource(Pages(pages));
}

/// Gives the tabs their icons from the theme (or their short names, if it has none).
fn show_tab_icons(
    theme: Res<Theme>,
    mut icons: Query<(&TabIcon, &mut ImageNode, &mut Visibility)>,
    mut labels: Query<(&TabLabel, &mut Visibility), Without<TabIcon>>,
) {
    if !theme.is_changed() {
        return;
    }
    for (TabIcon(tab), mut image, mut visibility) in &mut icons {
        let icon = theme.tab_icon(tab.id());
        *visibility = if icon.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        image.image = icon.unwrap_or_default();
    }
    for (TabLabel(tab), mut visibility) in &mut labels {
        *visibility = if theme.tab_icon(tab.id()).is_some() { Visibility::Hidden } else { Visibility::Inherited };
    }
}

fn record_sheet(mut events: MessageReader<FromServer>, mut sheet: ResMut<Sheet>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Character(new_sheet) = message {
            sheet.0 = Some(new_sheet.clone());
        }
    }
}

/// Each tab's key (unless the player is typing) or button opens it; the ✕
/// closes the window. A shop opening shows the bag, to sell from.
fn switch_tabs(
    mut events: MessageReader<FromServer>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    buttons: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    close: Query<&Interaction, (With<CloseButton>, Changed<Interaction>)>,
    dev: Res<DevSettings>,
    mut opened_for_dev: Local<bool>,
    mut open: ResMut<OpenTab>,
) {
    for tab in Tab::ALL {
        if focus.0.is_none() && keys.just_pressed(tab.key()) {
            open.toggle(tab);
        }
    }
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            open.0 = Some(button.0);
        }
    }
    if close.iter().any(|interaction| *interaction == Interaction::Pressed) {
        open.0 = None;
    }
    if events.read().any(|FromServer(message)| matches!(message, ServerMessage::ShopOpened { .. })) {
        open.0 = Some(Tab::Bag);
    }
    if !*opened_for_dev {
        *opened_for_dev = true;
        // (`equipment` is the gear tab's old name.)
        let named = |tab: Tab, name: &str| name == tab.id() || (tab == Tab::Equipment && name == "equipment");
        if let Some(tab) = Tab::ALL.into_iter().find(|tab| dev.open.iter().any(|name| named(*tab, name))) {
            open.0 = Some(tab);
        }
    }
}

/// Shows the window (while playing) on the open tab.
fn show_open_tab(
    open: Res<OpenTab>,
    state: Res<State<ClientState>>,
    mut window: Single<&mut Visibility, With<CharacterWindow>>,
    mut pages: Query<(&Page, &mut Node)>,
    mut tabs: Query<(&TabButton, &mut Themed, &mut BackgroundColor)>,
    mut labels: Query<(&TabLabel, &mut TextColor)>,
    mut title: Single<&mut Text, With<Title>>,
) {
    let playing = *state.get() == ClientState::Playing;
    **window = if playing && open.0.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if !open.is_changed() {
        return;
    }
    for (page, mut node) in &mut pages {
        node.display = if Some(page.0) == open.0 { Display::Flex } else { Display::None };
    }
    if let Some(tab) = open.0 {
        title.0 = tab.label().to_string();
    }
    for (tab, mut themed, mut background) in &mut tabs {
        let chosen = Some(tab.0) == open.0;
        themed.0 = if chosen { Piece::TabOpen } else { Piece::Tab };
        background.0 = if chosen { Color::srgba(1.0, 1.0, 1.0, 0.1) } else { Color::NONE };
    }
    for (label, mut colour) in &mut labels {
        colour.0 = if Some(label.0) == open.0 { OPEN_TAB_COLOUR } else { TAB_COLOUR };
    }
}

/// Lights up clickable boxes under the mouse.
fn highlight_boxes(mut boxes: Query<(&Interaction, &mut BackgroundColor), (With<HoverBox>, Changed<Interaction>)>) {
    for (interaction, mut background) in &mut boxes {
        background.0 = if *interaction == Interaction::None { BOX_COLOUR } else { BOX_HOVER_COLOUR };
    }
}

fn close_window(mut open: ResMut<OpenTab>) {
    open.0 = None;
}

fn forget_sheet(mut sheet: ResMut<Sheet>) {
    sheet.0 = None;
}

/// A section heading within a page, with a line under it.
pub fn heading(text: &str) -> impl Bundle {
    (
        Node { flex_direction: FlexDirection::Column, row_gap: px(1), margin: UiRect::top(px(4)), ..default() },
        children![
            (
                Text::new(text),
                TextFont { font_size: FontSize::Px(14.0), ..default() },
                TextColor(HEADING_COLOUR),
                TextShadow { offset: Vec2::splat(1.0), ..default() },
            ),
            divider(),
        ],
    )
}

/// A bar (such as experience) `height` pixels high, filled `fraction` of the
/// way (0 to 1) with `fill` (a bundle with a marker, to find it and change its
/// width) in `colour`.
pub fn bar(height: f32, colour: Color, fraction: f32, fill: impl Bundle) -> impl Bundle {
    (
        Node { height: px(height), width: percent(100), border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
        Themed(Piece::BarBack),
        children![(
            fill,
            Node {
                height: percent(100),
                width: percent(fraction.clamp(0.0, 1.0) * 100.0),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(colour),
            Themed(Piece::BarFill),
        )],
    )
}

/// The colour of experience bars.
pub const EXPERIENCE_COLOUR: Color = Color::srgb(0.85, 0.7, 0.3);

/// A thin line across (under headings).
pub fn divider() -> impl Bundle {
    (
        Node { width: percent(100), min_width: px(90), height: px(6), ..default() },
        BackgroundColor(Color::NONE),
        Themed(Piece::Divider),
    )
}

/// Ordinary text within a page, in a colour.
pub fn text(text: impl Into<String>, size: f32, colour: Color) -> impl Bundle {
    (Text::new(text), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(colour))
}

/// A square, clickable box for an icon (with a tooltip, filled in by its tab).
pub fn icon_box(size: f32) -> impl Bundle {
    (
        Button,
        HoverBox,
        crate::ui::tooltip::Tooltip::default(),
        Node {
            width: px(size),
            height: px(size),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(BOX_COLOUR),
        Themed(Piece::Slot),
    )
}

/// A small text button (such as "+" or "Learn").
pub fn small_button(label: &str) -> impl Bundle {
    (
        Button,
        HoverBox,
        Node {
            padding: UiRect::axes(px(7), px(2)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(BOX_COLOUR),
        BorderColor::all(BORDER_COLOUR),
        Themed(Piece::Button),
        children![(
            Text::new(label),
            TextFont { font_size: FontSize::Px(12.0), ..default() },
            TextColor(HEADING_COLOUR)
        )],
    )
}
