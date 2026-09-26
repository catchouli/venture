//! The player frame, in the top-left corner: the player's name, level,
//! health, mana and experience. The server sends health and mana whenever
//! they change ([`ServerMessage::Vitals`]); the level and experience come
//! from the character sheet. How the frame looks is its layout
//! (`ui/layouts/player_frame.ron`); this gives it the data it shows.

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::characters::nameplates::Nameplate;
use crate::characters::player::LocalPlayer;
use crate::network::{ClientState, FromServer};
use crate::ui::character_window::Sheet;
use crate::ui::layout::{UiData, UiRoot};

/// The frame's layout.
const FRAME: &str = "player_frame";

/// Shows the player frame.
pub struct VitalsPlugin;

impl Plugin for VitalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Vitals>()
            .add_systems(Update, (record_vitals, show_frame).chain())
            .add_systems(OnExit(ClientState::Playing), forget_vitals);
    }
}

/// The player's health and mana, as the server last said.
#[derive(Resource, Default)]
pub struct Vitals(pub Option<PlayerVitals>);

/// Health and mana, now and at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerVitals {
    /// Health left.
    pub health: u32,
    /// Most health.
    pub max_health: u32,
    /// Mana left.
    pub mana: u32,
    /// Most mana.
    pub max_mana: u32,
}

fn record_vitals(mut events: MessageReader<FromServer>, mut vitals: ResMut<Vitals>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Vitals { health, max_health, mana, max_mana } = message {
            vitals.0 =
                Some(PlayerVitals { health: *health, max_health: *max_health, mana: *mana, max_mana: *max_mana });
        }
    }
}

/// Gives the player frame its data, and shows it while playing.
fn show_frame(
    state: Res<State<ClientState>>,
    vitals: Res<Vitals>,
    sheet: Res<Sheet>,
    player: Option<Single<&Nameplate, With<LocalPlayer>>>,
    mut data: ResMut<UiData>,
    mut roots: Query<(&UiRoot, &mut Visibility)>,
) {
    let playing = *state.get() == ClientState::Playing;
    for (root, mut visibility) in &mut roots {
        let wanted = if playing { Visibility::Inherited } else { Visibility::Hidden };
        if root.0 == FRAME && *visibility != wanted {
            *visibility = wanted;
        }
    }
    if let Some(player) = player {
        UiData::set(&mut data, "name", &player.0);
    }
    if let Some(sheet) = &sheet.0 {
        UiData::set(&mut data, "level", sheet.level);
        UiData::set(&mut data, "experience", sheet.experience);
        UiData::set(&mut data, "experience_max", sheet.experience_to_next);
    }
    if let Some(vitals) = vitals.0 {
        UiData::set(&mut data, "health", vitals.health);
        UiData::set(&mut data, "health_max", vitals.max_health);
        UiData::set(&mut data, "mana", vitals.mana);
        UiData::set(&mut data, "mana_max", vitals.max_mana);
    }
}

fn forget_vitals(mut vitals: ResMut<Vitals>) {
    vitals.0 = None;
}
