//! The interface drawn over the game: chat, the dialogue window, the toolbar,
//! the hotbar, the player frame (health and mana), the party frames and
//! invitation, the character window (party, stats, abilities, bag,
//! equipment, quests and emotes),
//! shops, character creation, the target frame, the connection overlay, text
//! fields, tooltips and the cursor.

pub mod character_creation;
pub mod character_window;
pub mod chat;
pub mod cursor;
pub mod dialogue_window;
pub mod hotbar;
pub mod layout;
pub mod loading_screen;
pub mod party;
pub mod shop;
pub mod target_frame;
pub mod text_input;
pub mod theme;
pub mod toolbar;
pub mod tooltip;
pub mod vitals;
pub mod widgets;

use bevy::prelude::*;

/// Shows the interface.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            text_input::TextInputPlugin,
            theme::ThemePlugin,
            layout::LayoutPlugin,
            chat::ChatPlugin,
            dialogue_window::DialogueWindowPlugin,
            character_window::CharacterWindowPlugin,
            toolbar::ToolbarPlugin,
            hotbar::HotbarPlugin,
            shop::ShopPlugin,
            tooltip::TooltipPlugin,
            (vitals::VitalsPlugin, party::PartyPlugin),
            character_creation::CharacterCreationPlugin,
            target_frame::TargetFramePlugin,
            loading_screen::LoadingScreenPlugin,
            cursor::CursorPlugin,
        ));
    }
}
