//! Characters: the placeholder character model, the player's own character,
//! other players, and the names above them.

pub mod model;
pub mod nameplates;
pub mod player;
pub mod remote;

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
        ));
    }
}
