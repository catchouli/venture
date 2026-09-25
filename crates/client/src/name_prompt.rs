//! Asking a new player for their character's name, the first time they join.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, MAX_NAME_CHARS, ServerMessage, clean_name};

use crate::dev_settings::DevSettings;
use crate::network::{ClientState, FromServer, ToServer};
use crate::text_input::{FieldText, KeyboardFocus, TextField, TextInputSystems, TextSubmitted};

/// Asks new players for a name.
pub struct NamePromptPlugin;

impl Plugin for NamePromptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Suggestion>()
            .add_systems(PreUpdate, remember_suggestion.after(crate::network::NetworkSystems))
            .add_systems(OnEnter(ClientState::ChoosingName), open_prompt)
            .add_systems(OnExit(ClientState::ChoosingName), close_prompt)
            .add_systems(
                Update,
                (submit_name.after(TextInputSystems), show_rejections).run_if(in_state(ClientState::ChoosingName)),
            );
    }
}

/// The name the server suggested.
#[derive(Resource, Default)]
struct Suggestion(String);

/// The prompt window.
#[derive(Component)]
struct NamePrompt;

/// The field the name is typed in.
#[derive(Component)]
struct NameField;

/// Where problems with the name are shown.
#[derive(Component)]
struct NameError;

fn remember_suggestion(mut events: MessageReader<FromServer>, mut suggestion: ResMut<Suggestion>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::NameNeeded { suggestion: name } = message {
            suggestion.0.clone_from(name);
        }
    }
}

fn open_prompt(
    mut commands: Commands,
    suggestion: Res<Suggestion>,
    dev: Res<DevSettings>,
    mut focus: ResMut<KeyboardFocus>,
    mut to_server: MessageWriter<ToServer>,
) {
    // Automated runs (such as screenshots) take the suggestion without asking.
    if dev.automated {
        to_server.write(ToServer(ClientMessage::ChooseName { name: suggestion.0.clone() }));
        return;
    }
    let title = TextFont { font_size: FontSize::Px(26.0), ..default() };
    let body = TextFont { font_size: FontSize::Px(16.0), ..default() };
    let mut field = None;
    commands
        .spawn((
            NamePrompt,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(12),
                        padding: UiRect::all(px(28)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.07, 0.05, 0.92)),
                ))
                .with_children(|panel| {
                    panel.spawn((Text::new("Welcome, traveller"), title, TextColor(Color::srgb(0.95, 0.85, 0.55))));
                    panel.spawn((Text::new("What's your name?"), body.clone()));
                    let entity = panel
                        .spawn((
                            NameField,
                            TextField {
                                text: suggestion.0.clone(),
                                max_chars: MAX_NAME_CHARS,
                                placeholder: String::new(),
                            },
                            Node { width: px(260), padding: UiRect::axes(px(8), px(6)), ..default() },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                        ))
                        .with_child((FieldText, Text::default(), body.clone()))
                        .id();
                    field = Some(entity);
                    panel.spawn((
                        Text::new("Press Enter to begin"),
                        body.clone(),
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                    ));
                    panel.spawn((NameError, Text::default(), body, TextColor(Color::srgb(1.0, 0.45, 0.4))));
                });
        });
    focus.0 = field;
}

fn close_prompt(mut commands: Commands, prompts: Query<Entity, With<NamePrompt>>, mut focus: ResMut<KeyboardFocus>) {
    for prompt in &prompts {
        commands.entity(prompt).despawn();
    }
    focus.0 = None;
}

/// Checks the name as it's submitted, and sends it if it looks fine.
fn submit_name(
    mut submitted: MessageReader<TextSubmitted>,
    field: Single<Entity, With<NameField>>,
    mut error: Single<&mut Text, With<NameError>>,
    mut to_server: MessageWriter<ToServer>,
) {
    for submission in submitted.read().filter(|submission| submission.field == *field) {
        match clean_name(&submission.text) {
            Ok(name) => {
                error.0.clear();
                to_server.write(ToServer(ClientMessage::ChooseName { name }));
            }
            Err(reason) => error.0 = reason,
        }
    }
}

fn show_rejections(mut events: MessageReader<FromServer>, error: Option<Single<&mut Text, With<NameError>>>) {
    let Some(mut error) = error else { return };
    for FromServer(message) in events.read() {
        if let ServerMessage::NameRejected { reason } = message {
            error.0.clone_from(reason);
        }
    }
}
