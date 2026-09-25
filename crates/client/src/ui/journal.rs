//! The quest journal: each quest the player has started, and what their
//! journal says about where they've got to. Opened from the toolbar (see
//! [`crate::ui::toolbar`]).
//!
//! The quests themselves are on the server, which sends the journal whenever
//! it changes ([`ServerMessage::Quests`]).

use bevy::prelude::*;
use venture_protocol::{QuestEntry, ServerMessage};

use crate::network::{ClientState, FromServer};
use crate::ui::toolbar::OpenPanels;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, spawn_close_button};

/// Width of the window, in pixels.
const WIDTH: f32 = 320.0;
/// How far below the top of the screen the window sits, in pixels.
const TOP: f32 = 44.0;
/// How far from the right edge the window sits: left of the bag, in pixels.
const RIGHT: f32 = 212.0;
/// Colour of the title, and of quests in progress.
const TITLE_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of finished quests' names.
const COMPLETE_COLOUR: Color = Color::srgb(0.6, 0.85, 0.55);
/// Colour of the journal text.
const TEXT_COLOUR: Color = Color::srgb(0.85, 0.85, 0.82);

/// Shows the quest journal.
pub struct JournalPlugin;

impl Plugin for JournalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Journal>()
            .add_systems(Startup, spawn_window)
            .add_systems(Update, (record_journal, show_entries, show_when_open, close_on_click).chain())
            .add_systems(OnExit(ClientState::Playing), forget_journal);
    }
}

/// The player's quests, as the server last said.
#[derive(Resource, Default)]
struct Journal(Vec<QuestEntry>);

/// The window.
#[derive(Component)]
struct JournalWindow;

/// Where the quests are listed.
#[derive(Component)]
struct Entries;

/// The ✕ button.
#[derive(Component)]
struct CloseButton;

fn spawn_window(mut commands: Commands) {
    commands
        .spawn((
            JournalWindow,
            // Blocks clicks on the world behind it.
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP),
                right: px(RIGHT),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(12)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            // Above the shop window, which sits in the same place.
            GlobalZIndex(1),
            Visibility::Hidden,
        ))
        .with_children(|window| {
            window.spawn((
                Text::new("Quests"),
                TextFont { font_size: FontSize::Px(16.0), ..default() },
                TextColor(TITLE_COLOUR),
            ));
            window.spawn((Entries, Node { flex_direction: FlexDirection::Column, row_gap: px(10), ..default() }));
            spawn_close_button(window, CloseButton);
        });
}

fn record_journal(mut events: MessageReader<FromServer>, mut journal: ResMut<Journal>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Quests { quests } = message {
            journal.0.clone_from(quests);
        }
    }
}

/// Lists the quests: those in progress first, then finished ones.
fn show_entries(mut commands: Commands, journal: Res<Journal>, entries: Single<Entity, With<Entries>>) {
    if !journal.is_changed() {
        return;
    }
    let name_font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    let text_font = TextFont { font_size: FontSize::Px(13.0), ..default() };
    commands.entity(*entries).despawn_children().with_children(|entries| {
        if journal.0.is_empty() {
            entries.spawn((
                Text::new("You haven't started any quests yet. Talk to the people around town!"),
                text_font.clone(),
                TextColor(TEXT_COLOUR),
            ));
        }
        let in_progress = journal.0.iter().filter(|quest| !quest.complete);
        let finished = journal.0.iter().filter(|quest| quest.complete);
        for quest in in_progress.chain(finished) {
            let (name, colour) = if quest.complete {
                (format!("{} (complete)", quest.name), COMPLETE_COLOUR)
            } else {
                (quest.name.clone(), TITLE_COLOUR)
            };
            entries.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(3), ..default() }).with_children(
                |entry| {
                    entry.spawn((Text::new(name), name_font.clone(), TextColor(colour)));
                    entry.spawn((Text::new(quest.journal.clone()), text_font.clone(), TextColor(TEXT_COLOUR)));
                },
            );
        }
    });
}

fn show_when_open(panels: Res<OpenPanels>, mut window: Single<&mut Visibility, With<JournalWindow>>) {
    **window = if panels.journal { Visibility::Inherited } else { Visibility::Hidden };
}

/// The ✕ closes the journal.
fn close_on_click(
    close: Query<&Interaction, (With<CloseButton>, Changed<Interaction>)>,
    mut panels: ResMut<OpenPanels>,
) {
    if close.iter().any(|interaction| *interaction == Interaction::Pressed) {
        panels.journal = false;
    }
}

fn forget_journal(mut journal: ResMut<Journal>) {
    journal.0.clear();
}
