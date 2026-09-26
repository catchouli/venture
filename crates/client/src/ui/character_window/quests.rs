//! The quests tab: each quest the player has started, and what their journal
//! says about where they've got to.
//!
//! The quests themselves are on the server, which sends the journal whenever
//! it changes ([`ServerMessage::Quests`]).

use bevy::prelude::*;
use venture_protocol::{QuestEntry, ServerMessage};

use crate::network::{ClientState, FromServer};
use crate::ui::character_window::{Pages, Tab};

/// Colour of the title, and of quests in progress.
const TITLE_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of finished quests' names.
const COMPLETE_COLOUR: Color = Color::srgb(0.6, 0.85, 0.55);
/// Colour of the journal text.
const TEXT_COLOUR: Color = Color::srgb(0.85, 0.85, 0.82);

/// Shows the quests tab.
pub struct QuestsPlugin;

impl Plugin for QuestsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Journal>()
            .add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (record_journal, show_entries).chain())
            .add_systems(OnExit(ClientState::Playing), forget_journal);
    }
}

/// The player's quests, as the server last said.
#[derive(Resource, Default)]
struct Journal(Vec<QuestEntry>);

/// Where the quests are listed.
#[derive(Component)]
struct Entries;

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands
        .entity(pages.of(Tab::Quests))
        .with_child((Entries, Node { flex_direction: FlexDirection::Column, row_gap: px(10), ..default() }));
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

fn forget_journal(mut journal: ResMut<Journal>) {
    journal.0.clear();
}
