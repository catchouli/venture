//! Sound: playing the content's sounds for what the server reports. An ability
//! plays its own sound when used; hits, misses, critical hits, deaths and the
//! player's level-ups play the game's sounds (`sounds.ron`), and so does
//! the player's fishing (casting, nibbles, bites and catches). Sounds further
//! from the player's character are quieter.
//!
//! Automated runs (screenshots) are silent.

use bevy::audio::Volume;
use bevy::prelude::*;
use venture_protocol::{AttackResult, EntityId, FishingState, ServerMessage};

use crate::characters::player::LocalPlayer;
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::interaction::targeting::Targetable;
use crate::network::{ClientState, FromServer};

/// Sounds this far away (in metres) or further are as quiet as they get.
const HEARING_DISTANCE: f32 = 40.0;
/// The quietest a sound gets, from 0 to 1.
const QUIETEST: f32 = 0.15;
/// How loud sounds are overall, from 0 to 1.
const VOLUME: f32 = 0.6;

/// Plays sounds.
pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (play_sounds, play_when_due).run_if(in_state(ClientState::Playing)));
    }
}

/// How long after a swing starts the axe comes down, in seconds (see
/// `characters::model`'s swing: raised over the first third, then down).
const CHOP_DELAY: f32 = 0.22;
/// How loud a swing that doesn't bite is, compared with one that does.
const GLANCE_VOLUME: f32 = 0.5;

/// A sound to play in a moment.
#[derive(Component)]
struct Due {
    in_seconds: f32,
    sound: Handle<AudioSource>,
    volume: Volume,
}

/// Plays sounds once they're due.
fn play_when_due(mut commands: Commands, time: Res<Time>, mut due: Query<(Entity, &mut Due)>) {
    for (entity, mut sound) in &mut due {
        sound.in_seconds -= time.delta_secs();
        if sound.in_seconds <= 0.0 {
            commands.entity(entity).despawn();
            commands
                .spawn((AudioPlayer::new(sound.sound.clone()), PlaybackSettings::DESPAWN.with_volume(sound.volume)));
        }
    }
}

fn play_sounds(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    settings: Res<DevSettings>,
    content: Res<ContentStore>,
    me: Option<Single<(&LocalPlayer, &GlobalTransform)>>,
    others: Query<(&Targetable, &GlobalTransform)>,
) {
    if settings.automated {
        events.clear();
        return;
    }
    let Some(me) = me else { return };
    let (me, here) = (me.0.id, me.1.translation());
    // How loud something that happened to `id` sounds.
    let loudness = |id: EntityId| {
        let there = if id == me {
            here
        } else {
            let Some((_, place)) = others.iter().find(|(targetable, _)| targetable.id == id) else { return QUIETEST };
            place.translation()
        };
        (1.0 - here.distance(there) / HEARING_DISTANCE).clamp(QUIETEST, 1.0)
    };
    let sounds = &content.game_sounds;
    for FromServer(message) in events.read() {
        let (file, id) = match message {
            ServerMessage::AbilityUsed { user, ability, .. } => {
                (content.abilities.get(ability).and_then(|def| def.sound.as_ref()), *user)
            }
            ServerMessage::Attacked { target, result, .. } => {
                let file = match result {
                    AttackResult::Miss => &sounds.miss,
                    AttackResult::Hit(_) => &sounds.hit,
                    AttackResult::Critical(_) => &sounds.critical,
                };
                (file.as_ref(), *target)
            }
            ServerMessage::Died { id } => (sounds.death.as_ref(), *id),
            ServerMessage::Chopped { id, bit, .. } => {
                // In time with the swing: when the axe comes down (a swing that
                // doesn't bite glances, more quietly).
                let Some(sound) = sounds.chop.as_ref().and_then(|file| content.sounds.get(file)) else { continue };
                let quieter = if *bit { 1.0 } else { GLANCE_VOLUME };
                let volume = Volume::Linear(VOLUME * loudness(*id) * quieter);
                commands.spawn(Due { in_seconds: CHOP_DELAY, sound: sound.clone(), volume });
                continue;
            }
            ServerMessage::PlayerChanged { id, celebrate: true, .. } if *id == me => (sounds.level_up.as_ref(), *id),
            ServerMessage::Fishing(state) => {
                let file = match state {
                    FishingState::Waiting { .. } => &sounds.cast,
                    FishingState::Nibble => &sounds.nibble,
                    FishingState::Bite => &sounds.bite,
                    FishingState::Caught { .. } => &sounds.catch,
                    _ => continue,
                };
                (file.as_ref(), me)
            }
            _ => continue,
        };
        let Some(sound) = file.and_then(|file| content.sounds.get(file)) else { continue };
        let volume = Volume::Linear(VOLUME * loudness(id));
        commands.spawn((AudioPlayer::new(sound.clone()), PlaybackSettings::DESPAWN.with_volume(volume)));
    }
}
