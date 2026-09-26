//! The character window: everything about the player's own character, in
//! tabs. Each tab has a key: Stats (**C**), Skills (**K**), Bag (**I**),
//! Equipment (**G**), Quests (**J**) and Emotes (**O**). Pressing a tab's key
//! opens the window on that tab, or closes it if it's already showing.
//!
//! This module is the window itself (the tab bar, which tab is open, and the
//! latest [`CharacterSheet`] from the server). Each tab is its own module,
//! which fills in its page.

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
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER, spawn_close_button};

/// Width of the window, in pixels.
pub const WINDOW_WIDTH: f32 = 320.0;
/// How far below the top of the screen the window sits (under the toolbar), in pixels.
const TOP: f32 = 44.0;
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
/// Background of a clickable box (a slot, a skill) within pages.
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
            .add_systems(Update, (record_sheet, switch_tabs, show_open_tab, highlight_boxes).chain())
            .add_systems(OnExit(ClientState::Playing), (close_window, forget_sheet))
            .add_plugins((
                stats::StatsPlugin,
                skills::SkillsPlugin,
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
    /// Actions and skills.
    Skills,
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
    pub const ALL: [Tab; 6] = [Tab::Stats, Tab::Skills, Tab::Bag, Tab::Equipment, Tab::Quests, Tab::Emotes];

    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Stats => "Stats",
            Tab::Skills => "Skills",
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
            Tab::Bag => KeyCode::KeyI,
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
            Tab::Bag => "I",
            Tab::Equipment => "G",
            Tab::Quests => "J",
            Tab::Emotes => "O",
        }
    }

    /// Its name in `VENTURE_OPEN` (see [`DevSettings`]).
    fn dev_name(self) -> &'static str {
        match self {
            Tab::Stats => "stats",
            Tab::Skills => "skills",
            Tab::Bag => "bag",
            Tab::Equipment => "equipment",
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

/// Each tab's page entity, for its module to fill in at startup.
#[derive(Resource)]
pub struct Pages(HashMap<Tab, Entity>);

impl Pages {
    /// A tab's page.
    pub fn of(&self, tab: Tab) -> Entity {
        self.0[&tab]
    }
}

/// A clickable box within a page (a slot, a skill, a button), lit up under the mouse.
#[derive(Component)]
pub struct HoverBox;

/// Makes the window, with a page for each tab.
pub fn spawn_window(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(13.0), ..default() };
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
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(10)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            WINDOW_LAYER,
            Visibility::Hidden,
        ))
        .with_children(|window| {
            window.spawn(Node { column_gap: px(2), margin: UiRect::right(px(16)), ..default() }).with_children(|bar| {
                for tab in Tab::ALL {
                    bar.spawn((
                        TabButton(tab),
                        Button,
                        crate::ui::tooltip::Tooltip {
                            title: tab.label().to_string(),
                            body: format!("Key: {}", tab.key_name()),
                        },
                        Node {
                            padding: UiRect::axes(px(5), px(3)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_child((Text::new(tab.label()), font.clone(), TextColor(TAB_COLOUR)));
                }
            });
            for tab in Tab::ALL {
                let page = window
                    .spawn((Page(tab), Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }))
                    .id();
                pages.insert(tab, page);
            }
            spawn_close_button(window, CloseButton);
        });
    commands.insert_resource(Pages(pages));
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
        if let Some(tab) = Tab::ALL.into_iter().find(|tab| dev.open.iter().any(|name| name == tab.dev_name())) {
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
    tabs: Query<(&TabButton, &Children, &mut BackgroundColor)>,
    mut labels: Query<&mut TextColor>,
) {
    let playing = *state.get() == ClientState::Playing;
    **window = if playing && open.0.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if !open.is_changed() {
        return;
    }
    for (page, mut node) in &mut pages {
        node.display = if Some(page.0) == open.0 { Display::Flex } else { Display::None };
    }
    let mut tabs = tabs;
    for (tab, children, mut background) in &mut tabs {
        let chosen = Some(tab.0) == open.0;
        background.0 = if chosen { Color::srgba(1.0, 1.0, 1.0, 0.1) } else { Color::NONE };
        for child in children {
            if let Ok(mut colour) = labels.get_mut(*child) {
                colour.0 = if chosen { OPEN_TAB_COLOUR } else { TAB_COLOUR };
            }
        }
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

/// A section heading within a page.
pub fn heading(text: &str) -> impl Bundle {
    (
        Text::new(text),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(HEADING_COLOUR),
        Node { margin: UiRect::top(px(4)), ..default() },
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
        children![(
            Text::new(label),
            TextFont { font_size: FontSize::Px(12.0), ..default() },
            TextColor(HEADING_COLOUR)
        )],
    )
}
