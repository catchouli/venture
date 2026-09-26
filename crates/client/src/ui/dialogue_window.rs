//! The dialogue window: a conversation with an NPC, seen only by the player
//! having it.
//!
//! The server runs the conversation and sends it a page at a time
//! ([`ServerMessage::Dialogue`]): what's said, and the replies to choose
//! from. Clicking a reply sends it back ([`ClientMessage::ChooseDialogueOption`]),
//! and the server answers with the next page. When there are no replies the
//! conversation is over, and "Goodbye" closes the window. The ✕ closes it at
//! any point. (What NPCs say out loud, to everyone nearby, appears in chat
//! and above their heads instead.)

use bevy::prelude::*;
use venture_protocol::{ClientMessage, DialogueLine, EntityId, ServerMessage};

use crate::characters::nameplates::Nameplate;
use crate::dev::settings::DevSettings;
use crate::interaction::targeting::Targetable;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::chat::NPC_NAME_COLOUR;
use crate::ui::theme::{Piece, Themed};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER, spawn_close_button};

/// Width of the window, in pixels.
const WIDTH: f32 = 420.0;
/// Colour of the NPC's name in the title.
const TITLE_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of what's said.
const TEXT_COLOUR: Color = Color::srgb(0.95, 0.95, 0.92);
/// Colour of the replies.
const OPTION_COLOUR: Color = Color::srgb(0.95, 0.8, 0.45);
/// Colour of a reply under the mouse.
const OPTION_HOVER_COLOUR: Color = Color::srgb(1.0, 0.95, 0.7);

/// Shows conversations with NPCs.
pub struct DialogueWindowPlugin;

impl Plugin for DialogueWindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_window)
            .add_systems(
                Update,
                (show_pages, choose_replies, choose_replies_if_requested, highlight_replies, close_on_click),
            )
            .add_systems(OnExit(ClientState::Playing), close_window);
    }
}

/// The window.
#[derive(Component)]
struct DialogueWindow;

/// The title, showing who the player is talking to.
#[derive(Component)]
struct DialogueTitle;

/// Where the page's lines and replies go.
#[derive(Component)]
struct DialoguePage;

/// A reply to click, by its position in the list the server sent.
#[derive(Component)]
struct Reply(u32);

/// Closes a finished conversation.
#[derive(Component)]
struct GoodbyeButton;

/// The ✕ button, which ends the conversation.
#[derive(Component)]
struct CloseButton;

fn spawn_window(mut commands: Commands) {
    // A full-width row along the bottom of the screen, centring the window
    // below the player's character, which the camera keeps in the middle.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(12),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                DialogueWindow,
                // Blocks clicks on the world behind it.
                Interaction::default(),
                Node {
                    width: px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    padding: UiRect::all(px(crate::ui::widgets::WINDOW_PADDING)),
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
                    DialogueTitle,
                    Text::default(),
                    TextFont { font_size: FontSize::Px(16.0), ..default() },
                    TextColor(TITLE_COLOUR),
                    TextShadow { offset: Vec2::splat(1.0), ..default() },
                ));
                window.spawn(crate::ui::character_window::divider());
                window
                    .spawn((DialoguePage, Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }));
                spawn_close_button(window, CloseButton);
            });
        });
}

/// Opens the window with each page the server sends, and closes it when the
/// conversation ends.
fn show_pages(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    characters: Query<(&Targetable, &Nameplate)>,
    window: Single<&mut Visibility, With<DialogueWindow>>,
    mut title: Single<&mut Text, With<DialogueTitle>>,
    page: Single<Entity, With<DialoguePage>>,
) {
    let mut visibility = window.into_inner();
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::Dialogue { npc, lines, options } => {
                title.0 = name_of(*npc, &characters);
                commands.entity(*page).despawn_children().with_children(|page| fill_page(page, lines, options));
                *visibility = Visibility::Inherited;
            }
            ServerMessage::DialogueEnded => *visibility = Visibility::Hidden,
            _ => {}
        }
    }
}

/// The name of the character with this id, as shown above their head.
fn name_of(id: EntityId, characters: &Query<(&Targetable, &Nameplate)>) -> String {
    characters.iter().find(|(targetable, _)| targetable.id == id).map(|(_, name)| name.0.clone()).unwrap_or_default()
}

/// Adds a page's lines, then its replies (or "Goodbye" if there are none).
fn fill_page(page: &mut ChildSpawnerCommands, lines: &[DialogueLine], options: &[String]) {
    let font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    for line in lines {
        let mut text = page.spawn((Text::default(), font.clone(), TextColor(TEXT_COLOUR)));
        if let Some(speaker) = &line.speaker {
            text.with_child((TextSpan::new(format!("{speaker}: ")), font.clone(), TextColor(NPC_NAME_COLOUR)));
        }
        text.with_child((TextSpan::new(line.text.clone()), font.clone(), TextColor(TEXT_COLOUR)));
    }
    let mut add_button = |label: String, marker: Option<u32>| {
        let mut button = page.spawn((
            Button,
            Node { padding: UiRect::axes(px(10), px(5)), margin: UiRect::top(px(2)), ..default() },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
            Themed(Piece::Button),
        ));
        button.with_child((Text::new(label), font.clone(), TextColor(OPTION_COLOUR)));
        match marker {
            Some(index) => button.insert(Reply(index)),
            None => button.insert(GoodbyeButton),
        };
    };
    if options.is_empty() {
        add_button("Goodbye.".to_string(), None);
    }
    for (index, option) in options.iter().enumerate() {
        add_button(format!("{}. {option}", index + 1), Some(index as u32));
    }
}

/// Sends the reply the player clicked. The replies are removed straight
/// away, so a double click can't also pick something on the next page.
fn choose_replies(
    mut commands: Commands,
    replies: Query<(&Interaction, &Reply), Changed<Interaction>>,
    all_replies: Query<Entity, With<Reply>>,
    mut to_server: MessageWriter<ToServer>,
) {
    let Some((_, &Reply(index))) = replies.iter().find(|(interaction, _)| **interaction == Interaction::Pressed) else {
        return;
    };
    to_server.write(ToServer(ClientMessage::ChooseDialogueOption { index }));
    for reply in &all_replies {
        commands.entity(reply).despawn();
    }
}

/// For development: picks the `VENTURE_CHOOSE` replies, one per page.
fn choose_replies_if_requested(
    dev: Res<DevSettings>,
    mut events: MessageReader<FromServer>,
    mut chosen: Local<usize>,
    mut to_server: MessageWriter<ToServer>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Dialogue { options, .. } = message else { continue };
        let Some(&number) = dev.choose.get(*chosen).filter(|_| !options.is_empty()) else { continue };
        to_server.write(ToServer(ClientMessage::ChooseDialogueOption { index: number.saturating_sub(1) }));
        *chosen += 1;
    }
}

/// Lights up the reply (or "Goodbye") under the mouse.
fn highlight_replies(
    buttons: Query<(&Interaction, &Children), (Changed<Interaction>, Or<(With<Reply>, With<GoodbyeButton>)>)>,
    mut labels: Query<&mut TextColor>,
) {
    for (interaction, children) in &buttons {
        let colour = if *interaction == Interaction::None { OPTION_COLOUR } else { OPTION_HOVER_COLOUR };
        for child in children {
            if let Ok(mut label) = labels.get_mut(*child) {
                label.0 = colour;
            }
        }
    }
}

/// The ✕ ends the conversation; "Goodbye" closes a finished one.
fn close_on_click(
    close: Query<&Interaction, (With<CloseButton>, Changed<Interaction>)>,
    goodbye: Query<&Interaction, (With<GoodbyeButton>, Changed<Interaction>)>,
    mut window: Single<&mut Visibility, With<DialogueWindow>>,
    mut to_server: MessageWriter<ToServer>,
) {
    if close.iter().any(|interaction| *interaction == Interaction::Pressed) {
        to_server.write(ToServer(ClientMessage::EndDialogue));
        **window = Visibility::Hidden;
    }
    if goodbye.iter().any(|interaction| *interaction == Interaction::Pressed) {
        // The server has already finished this conversation.
        **window = Visibility::Hidden;
    }
}

fn close_window(mut window: Single<&mut Visibility, With<DialogueWindow>>) {
    **window = Visibility::Hidden;
}
