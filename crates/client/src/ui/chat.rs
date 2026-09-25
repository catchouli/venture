//! The chat box, in the bottom-left corner: recent messages above a line to
//! type in.
//!
//! - Press Enter, or click the input line, to start typing.
//! - Enter sends the message (unless it's empty) and stops typing.
//! - Escape, or clicking the world, stops typing without sending.
//!
//! Sending only *asks* the server to say something ([`ToServer`]). Messages
//! appear in the box when the server sends them back ([`FromServer`]), for
//! the sender and everyone else alike, already checked and cleaned.
//!
//! A message starting with `/` is a command instead, naming an
//! [action](crate::interaction::actions) (`/follow`, `/help`, ...). The
//! client's own messages (such as the `/help` list) are [`SystemMessage`]s.

use std::collections::VecDeque;

use bevy::prelude::*;
use venture_protocol::{ClientMessage, MAX_CHAT_CHARS, ServerMessage, clean_chat_text};

use crate::dev::settings::DevSettings;
use crate::interaction::actions::{Action, PerformAction};
use crate::interaction::targeting::{TargetKind, Targetable};
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::text_input::{FieldText, KeyboardFocus, TextField, TextInputSystems, TextSubmitted};
use crate::world::streaming::StreamingProgress;

/// How many messages are shown.
const VISIBLE_LINES: usize = 8;
/// How many messages are remembered.
const HISTORY: usize = 100;
/// Width of the chat box, in pixels.
const WIDTH: f32 = 440.0;

/// Colour of players' names.
const NAME_COLOUR: Color = Color::srgb(0.95, 0.8, 0.45);
/// Colour of NPCs' names.
pub const NPC_NAME_COLOUR: Color = Color::srgb(0.6, 0.9, 0.55);
/// Colour of what players say.
const MESSAGE_COLOUR: Color = Color::srgb(0.95, 0.95, 0.92);
/// Colour of messages from the server itself.
const NOTICE_COLOUR: Color = Color::srgb(0.6, 0.85, 0.95);

/// Shows chat and lets the player take part.
pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>().add_message::<SystemMessage>().add_systems(Startup, spawn_chat_box).add_systems(
            Update,
            (
                record_messages,
                show_messages.run_if(resource_changed::<ChatLog>),
                // Starting to type is checked before sending, so the Enter that sends a
                // message doesn't also start typing again straight away.
                (start_typing, send_typed_message)
                    .chain()
                    .after(TextInputSystems)
                    .run_if(in_state(ClientState::Playing)),
                say_once_if_requested.run_if(in_state(ClientState::Playing)),
                show_only_while_playing,
            ),
        );
    }
}

/// A message from the client itself, shown in chat like the server's notices.
#[derive(Message, Clone, Debug)]
pub struct SystemMessage(pub String);

/// The messages received, oldest first.
#[derive(Resource, Default)]
struct ChatLog(VecDeque<ChatLine>);

/// One message.
enum ChatLine {
    /// A player or NPC said something.
    Said { name: String, text: String, npc: bool },
    /// A message from the server.
    Notice(String),
}

/// The whole chat box.
#[derive(Component)]
struct ChatBox;

/// The column the messages are shown in.
#[derive(Component)]
struct ChatLines;

/// The line to type in.
#[derive(Component)]
struct ChatInput;

fn spawn_chat_box(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    commands
        .spawn((
            ChatBox,
            Node {
                position_type: PositionType::Absolute,
                left: px(10),
                bottom: px(10),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
        ))
        .with_children(|chat| {
            chat.spawn((
                ChatLines,
                Node { flex_direction: FlexDirection::Column, padding: UiRect::all(px(6)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            ));
            chat.spawn((
                ChatInput,
                Button,
                TextField {
                    text: String::new(),
                    max_chars: MAX_CHAT_CHARS,
                    placeholder: "Press Enter to chat".to_string(),
                },
                Node { padding: UiRect::axes(px(6), px(4)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .with_child((FieldText, Text::default(), font));
        });
}

/// Adds chat and notices from the server, and the client's own messages, to the log.
fn record_messages(
    mut events: MessageReader<FromServer>,
    mut system: MessageReader<SystemMessage>,
    mut log: ResMut<ChatLog>,
    speakers: Query<&Targetable>,
) {
    for FromServer(message) in events.read() {
        let line = match message {
            ServerMessage::Chat { from, name, text } => {
                let npc = speakers.iter().any(|speaker| speaker.id == *from && speaker.kind == TargetKind::Npc);
                ChatLine::Said { name: name.clone(), text: text.clone(), npc }
            }
            ServerMessage::Notice { text } => ChatLine::Notice(text.clone()),
            _ => continue,
        };
        log.push(line);
    }
    for SystemMessage(text) in system.read() {
        log.push(ChatLine::Notice(text.clone()));
    }
}

impl ChatLog {
    fn push(&mut self, line: ChatLine) {
        self.0.push_back(line);
        if self.0.len() > HISTORY {
            self.0.pop_front();
        }
    }
}

/// Shows the most recent messages.
fn show_messages(mut commands: Commands, log: Res<ChatLog>, lines: Single<Entity, With<ChatLines>>) {
    let font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    let shadow = TextShadow { offset: Vec2::splat(1.0), ..default() };
    commands.entity(*lines).despawn_children().with_children(|lines| {
        for line in log.0.iter().skip(log.0.len().saturating_sub(VISIBLE_LINES)) {
            match line {
                ChatLine::Said { name, text, npc } => {
                    let name_colour = if *npc { NPC_NAME_COLOUR } else { NAME_COLOUR };
                    lines
                        .spawn((Text::new(format!("{name}: ")), font.clone(), TextColor(name_colour), shadow))
                        .with_child((TextSpan::new(text.clone()), font.clone(), TextColor(MESSAGE_COLOUR), shadow));
                }
                ChatLine::Notice(text) => {
                    lines.spawn((Text::new(text.clone()), font.clone(), TextColor(NOTICE_COLOUR), shadow));
                }
            }
        }
    });
}

/// Enter, or a click on the input line, starts typing.
fn start_typing(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<KeyboardFocus>,
    input: Single<(Entity, &Interaction), With<ChatInput>>,
) {
    let (input, interaction) = *input;
    let clicked = *interaction == Interaction::Pressed;
    let pressed_enter = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    if focus.0.is_none() && (clicked || pressed_enter) {
        focus.0 = Some(input);
    }
}

/// Sends the typed message when Enter is pressed, then stops typing.
fn send_typed_message(
    mut submitted: MessageReader<TextSubmitted>,
    mut focus: ResMut<KeyboardFocus>,
    mut input: Single<(Entity, &mut TextField), With<ChatInput>>,
    mut sender: ChatSender,
) {
    let (entity, ref mut field) = *input;
    for submission in submitted.read().filter(|submission| submission.field == entity) {
        sender.submit(&submission.text);
        field.text.clear();
        focus.0 = None;
    }
}

/// For development: says (or runs) the `VENTURE_SAY` messages once the
/// world around the player has loaded.
fn say_once_if_requested(
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    mut said: Local<bool>,
    mut sender: ChatSender,
) {
    if *said || !progress.measured || progress.waiting > 0 {
        return;
    }
    for text in &dev.say {
        sender.submit(text);
    }
    *said = true;
}

/// Everything needed to act on what the player typed.
#[derive(bevy::ecs::system::SystemParam)]
struct ChatSender<'w> {
    to_server: MessageWriter<'w, ToServer>,
    actions: MessageWriter<'w, PerformAction>,
    system: MessageWriter<'w, SystemMessage>,
}

impl ChatSender<'_> {
    /// Says the text in chat, or carries out the command if it starts with `/`.
    fn submit(&mut self, text: &str) {
        let Some(text) = clean_chat_text(text) else { return };
        let Some(command) = text.strip_prefix('/') else {
            self.to_server.write(ToServer(ClientMessage::Chat { text }));
            return;
        };
        let name = command.split_whitespace().next().unwrap_or_default();
        match Action::named(name) {
            Some(action) => {
                self.actions.write(PerformAction(action));
            }
            None => {
                let problem = if name.is_empty() {
                    "Type /help to see the commands.".to_string()
                } else {
                    format!("There's no /{name} command. Type /help to see the commands.")
                };
                self.system.write(SystemMessage(problem));
            }
        }
    }
}

fn show_only_while_playing(state: Res<State<ClientState>>, mut chat: Single<&mut Visibility, With<ChatBox>>) {
    **chat = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
}
