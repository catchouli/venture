//! The hotbar: which abilities and actions are on keys 1 to 0. It's only a
//! layout (nothing on it changes the game until it's used, through the
//! usual requests), saved with the character.
//!
//! New characters (and characters made before the hotbar) start with Attack,
//! then the abilities they know. Players rearrange it by dragging; the client
//! sends the whole bar, and only abilities they know are kept. A newly learned
//! ability goes in the first empty slot, and abilities a character no longer knows
//! (after an admin reset, say) come off.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_protocol::{HOTBAR_SLOTS, HotbarAction, HotbarEntry, ServerMessage};

use crate::ServerSystems;
use crate::character::Character;
use crate::entities::Player;
use crate::link::Link;
use crate::requests::HotbarRequest;

/// Keeps hotbars.
pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, rearrange.in_set(ServerSystems::Gameplay))
            .add_systems(FixedUpdate, follow_abilities.in_set(ServerSystems::Updates));
    }
}

/// A player's hotbar: exactly [`HOTBAR_SLOTS`] slots, from key 1.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Hotbar(Vec<Option<HotbarEntry>>);

impl Hotbar {
    /// A saved hotbar, or the starting one if there's none.
    pub fn from_saved(saved: Option<Vec<Option<HotbarEntry>>>, character: &Character) -> Self {
        match saved {
            Some(slots) => Self::fitted(slots, character),
            None => Self::starting(character),
        }
    }

    /// Attack, then the abilities the character knows.
    pub fn starting(character: &Character) -> Self {
        let attack = HotbarEntry::Action(HotbarAction::Attack);
        let abilities = character.abilities.iter().map(|ability| HotbarEntry::Ability(ability.clone()));
        Self::fitted(std::iter::once(attack).chain(abilities).map(Some).collect(), character)
    }

    /// Exactly [`HOTBAR_SLOTS`] slots, with abilities the character doesn't know taken off.
    fn fitted(mut slots: Vec<Option<HotbarEntry>>, character: &Character) -> Self {
        for slot in &mut slots {
            if let Some(HotbarEntry::Ability(ability)) = slot
                && !character.abilities.contains(ability)
            {
                *slot = None;
            }
        }
        slots.resize(HOTBAR_SLOTS, None);
        Self(slots)
    }

    /// What to save (and send).
    pub fn slots(&self) -> &[Option<HotbarEntry>] {
        &self.0
    }

    /// The message telling the player their hotbar.
    pub fn message(&self) -> ServerMessage {
        ServerMessage::Hotbar { slots: self.0.clone() }
    }
}

/// Applies players' rearrangements. The client already shows the new layout,
/// so it's only told if something had to be taken off.
fn rearrange(
    mut requests: MessageReader<HotbarRequest>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &Character, &mut Hotbar)>,
) {
    for request in requests.read() {
        let Ok((player, character, mut hotbar)) = players.get_mut(request.player) else { continue };
        let fitted = Hotbar::fitted(request.slots.clone(), character);
        let asked: Vec<_> = request.slots.iter().cloned().chain(std::iter::repeat(None)).take(HOTBAR_SLOTS).collect();
        if fitted.0 != asked {
            link.send(player.connection, &fitted.message());
        }
        *hotbar = fitted;
    }
}

/// Keeps hotbars in step with the abilities characters know: new abilities go in
/// the first empty slot, forgotten ones come off. The player is sent the
/// new layout.
fn follow_abilities(
    mut link: ResMut<Link>,
    mut players: Query<(Entity, &Player, &Character, &mut Hotbar), Changed<Character>>,
    everyone: Query<(), With<Hotbar>>,
    mut known: Local<HashMap<Entity, Vec<String>>>,
) {
    for (entity, player, character, mut hotbar) in &mut players {
        // The first time a player is seen is when they join: nothing is new.
        let Some(before) = known.insert(entity, character.abilities.clone()) else { continue };
        let mut updated = Hotbar::fitted(hotbar.0.clone(), character);
        for ability in character.abilities.iter().filter(|ability| !before.contains(ability)) {
            let on_bar = updated.0.contains(&Some(HotbarEntry::Ability(ability.clone())));
            if let Some(empty) = updated.0.iter_mut().find(|slot| slot.is_none()).filter(|_| !on_bar) {
                *empty = Some(HotbarEntry::Ability(ability.clone()));
            }
        }
        if updated != *hotbar {
            link.send(player.connection, &updated.message());
            *hotbar = updated;
        }
    }
    // Forget players who have left.
    if known.len() > everyone.iter().len() {
        known.retain(|entity, _| everyone.contains(*entity));
    }
}
