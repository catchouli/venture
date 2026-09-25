//! Characters: the placeholder character model, the player's own character,
//! other players, the names and speech bubbles above them, the level-up effect and emotes.

pub mod emotes;
pub mod level_up;
pub mod model;
pub mod nameplates;
pub mod player;
pub mod remote;
pub mod speech;

use bevy::prelude::*;

/// Shows and animates characters.
pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            model::CharacterPlugin,
            player::PlayerPlugin,
            remote::RemotePlayersPlugin,
            nameplates::NameplatesPlugin,
            speech::SpeechPlugin,
            level_up::LevelUpPlugin,
            emotes::EmotesPlugin,
        ));
    }
}
