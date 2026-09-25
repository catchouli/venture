//! Simple single-line text fields (for chat and the name prompt), and which
//! one the keyboard is typing into.
//!
//! While a field has keyboard focus, game controls that use the keyboard
//! (running with WASD, turning the camera) are paused; check
//! [`KeyboardFocus::is_typing`].

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

/// How fast the text cursor blinks, in blinks per second.
const CURSOR_BLINKS_PER_SECOND: f32 = 2.0;

/// Handles typing into text fields.
pub struct TextInputPlugin;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyboardFocus>()
            .add_message::<TextSubmitted>()
            .add_systems(Update, (type_into_focused_field, show_field_text).chain().in_set(TextInputSystems));
    }
}

/// Systems that read typing. Anything that changes keyboard focus in
/// response to a key should run after these, so the same key press isn't
/// also typed.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextInputSystems;

/// A single-line text field. Give it a child entity with [`FieldText`] to show its contents.
#[derive(Component)]
pub struct TextField {
    /// What has been typed.
    pub text: String,
    /// The most characters it can hold.
    pub max_chars: usize,
    /// Shown, dimmed, when the field is empty and not focused.
    pub placeholder: String,
}

/// The text entity that displays a [`TextField`] (a child of the field).
#[derive(Component)]
pub struct FieldText;

/// Which text field (if any) the keyboard is typing into.
#[derive(Resource, Default)]
pub struct KeyboardFocus(pub Option<Entity>);

impl KeyboardFocus {
    /// Whether the player is typing, so keyboard game controls should be ignored.
    pub fn is_typing(&self) -> bool {
        self.0.is_some()
    }
}

/// Enter was pressed in a text field.
#[derive(Message)]
pub struct TextSubmitted {
    /// The field.
    pub field: Entity,
    /// Its text.
    pub text: String,
}

/// Applies key presses to the focused field: typed characters, Backspace,
/// Enter (submit) and Escape (stop typing).
fn type_into_focused_field(
    mut keys: MessageReader<KeyboardInput>,
    mut focus: ResMut<KeyboardFocus>,
    mut fields: Query<&mut TextField>,
    mut submitted: MessageWriter<TextSubmitted>,
) {
    let Some(focused) = focus.0 else {
        keys.clear();
        return;
    };
    let Ok(mut field) = fields.get_mut(focused) else {
        focus.0 = None;
        return;
    };
    for key in keys.read().filter(|key| key.state == ButtonState::Pressed) {
        match &key.logical_key {
            Key::Enter => {
                submitted.write(TextSubmitted { field: focused, text: field.text.clone() });
            }
            Key::Escape => focus.0 = None,
            Key::Backspace => {
                field.text.pop();
            }
            _ => {
                let Some(typed) = &key.text else { continue };
                for character in typed.chars().filter(|c| !c.is_control()) {
                    if field.text.chars().count() < field.max_chars {
                        field.text.push(character);
                    }
                }
            }
        }
    }
}

/// Shows each field's text, with a blinking cursor when focused, or its
/// placeholder when empty.
fn show_field_text(
    time: Res<Time>,
    focus: Res<KeyboardFocus>,
    fields: Query<(Entity, &TextField, &Children)>,
    mut texts: Query<(&mut Text, &mut TextColor), With<FieldText>>,
) {
    let cursor_on = ((time.elapsed_secs() * CURSOR_BLINKS_PER_SECOND * 2.0) as u32).is_multiple_of(2);
    for (entity, field, children) in &fields {
        let focused = focus.0 == Some(entity);
        let (shown, dimmed) = if focused {
            (format!("{}{}", field.text, if cursor_on { "|" } else { " " }), false)
        } else if field.text.is_empty() {
            (field.placeholder.clone(), true)
        } else {
            (field.text.clone(), false)
        };
        for child in children.iter() {
            if let Ok((mut text, mut colour)) = texts.get_mut(child) {
                if text.0 != shown {
                    text.0.clone_from(&shown);
                }
                colour.0 = if dimmed { Color::srgba(1.0, 1.0, 1.0, 0.45) } else { Color::WHITE };
            }
        }
    }
}
