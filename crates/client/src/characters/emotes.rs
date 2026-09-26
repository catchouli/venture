//! Emotes: gestures characters make for everyone nearby to see (such as
//! waving, with `/wave`), and other characters' jumps. The server says who did
//! what ([`ServerMessage::Emoted`], [`ServerMessage::Jumped`]), and their
//! character plays it. (The player's own jumps play at once: see
//! [`crate::characters::player`].)

use bevy::prelude::*;
use venture_protocol::{Emote, ServerMessage};

use crate::characters::model::Gait;
use crate::characters::player::LocalPlayer;
use crate::interaction::targeting::Targetable;
use crate::network::FromServer;

/// Plays emotes.
pub struct EmotesPlugin;

impl Plugin for EmotesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, play_emotes);
    }
}

fn play_emotes(
    mut events: MessageReader<FromServer>,
    mut me: Query<(&LocalPlayer, &mut Gait)>,
    mut others: Query<(&Targetable, &mut Gait), Without<LocalPlayer>>,
) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Jumped { id } = message {
            if let Some((_, mut gait)) = others.iter_mut().find(|(target, _)| target.id == *id) {
                gait.jump();
            }
            continue;
        }
        let ServerMessage::Emoted { id, emote } = message else { continue };
        let gait = match me.iter_mut().find(|(player, _)| player.id == *id) {
            Some((_, gait)) => Some(gait),
            None => others.iter_mut().find(|(target, _)| target.id == *id).map(|(_, gait)| gait),
        };
        let Some(mut gait) = gait else { continue };
        match emote {
            Emote::Wave => gait.wave(),
        }
    }
}
