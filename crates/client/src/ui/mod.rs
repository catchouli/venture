//! The interface drawn over the game: chat, the dialogue window, the name
//! prompt, the target frame, the connection overlay, text fields and the cursor.

pub mod chat;
pub mod connection_overlay;
pub mod cursor;
pub mod dialogue_window;
pub mod name_prompt;
pub mod target_frame;
pub mod text_input;
pub mod widgets;

use bevy::prelude::*;

/// Shows the interface.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            text_input::TextInputPlugin,
            chat::ChatPlugin,
            dialogue_window::DialogueWindowPlugin,
            name_prompt::NamePromptPlugin,
            target_frame::TargetFramePlugin,
            connection_overlay::ConnectionOverlayPlugin,
            cursor::CursorPlugin,
        ));
    }
}
