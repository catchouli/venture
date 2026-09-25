//! The interface drawn over the game: chat, the dialogue window, the toolbar,
//! the bag, the quest journal, shops, character creation, the target frame, the
//! connection overlay, text fields, tooltips and the cursor.

pub mod character_creation;
pub mod chat;
pub mod connection_overlay;
pub mod cursor;
pub mod dialogue_window;
pub mod inventory;
pub mod journal;
pub mod shop;
pub mod target_frame;
pub mod text_input;
pub mod toolbar;
pub mod tooltip;
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
            inventory::InventoryPlugin,
            journal::JournalPlugin,
            toolbar::ToolbarPlugin,
            shop::ShopPlugin,
            tooltip::TooltipPlugin,
            character_creation::CharacterCreationPlugin,
            target_frame::TargetFramePlugin,
            connection_overlay::ConnectionOverlayPlugin,
            cursor::CursorPlugin,
        ));
    }
}
