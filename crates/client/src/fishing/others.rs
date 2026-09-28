//! Other players fishing: their line and float, where the server says
//! they've cast ([`ServerMessage::Angler`]), and them turned to face it.

use std::collections::HashMap;

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{EntityId, ServerMessage};

use crate::characters::model::{Gait, RodTip};
use crate::characters::player::LocalPlayer;
use crate::common::coords::{from_bevy, to_bevy};
use crate::fishing::{LineAssets, line_between, rod_tip};
use crate::interaction::targeting::Targetable;
use crate::network::{ClientState, FromServer};
use crate::world::query::water_level;
use crate::world::streaming::ChunkStore;

/// Shows other players' lines.
pub struct OthersPlugin;

impl Plugin for OthersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OtherLines>()
            .add_systems(Update, (follow_server, show_lines).chain().run_if(in_state(ClientState::Playing)))
            .add_systems(OnExit(ClientState::Playing), forget_lines);
    }
}

/// Each other player's line, by their id.
#[derive(Resource, Default)]
struct OtherLines(HashMap<EntityId, OtherLine>);

struct OtherLine {
    at: Point,
    float: Entity,
    line: Entity,
}

fn follow_server(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    assets: Res<LineAssets>,
    mut lines: ResMut<OtherLines>,
    mut anglers: Query<(&Targetable, &mut Gait), Without<LocalPlayer>>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Angler { id, at } = message else { continue };
        // A cast: a swing of the rod.
        if at.is_some()
            && let Some((_, mut gait)) = anglers.iter_mut().find(|(targetable, _)| targetable.id == *id)
        {
            gait.swing();
        }
        match (at, lines.0.get_mut(id)) {
            (Some(at), Some(line)) => line.at = *at,
            (Some(at), None) => {
                let (float, line) = assets.spawn(&mut commands);
                lines.0.insert(*id, OtherLine { at: *at, float, line });
            }
            (None, _) => {
                if let Some(line) = lines.0.remove(id) {
                    commands.entity(line.float).despawn();
                    commands.entity(line.line).despawn();
                }
            }
        }
    }
}

/// Places each line and float, turns their anglers to face them, and drops
/// the lines of players who've gone.
fn show_lines(
    mut commands: Commands,
    time: Res<Time>,
    store: Res<ChunkStore>,
    mut lines: ResMut<OtherLines>,
    mut anglers: Query<(&Targetable, &mut Transform), Without<LocalPlayer>>,
    mut parts: Query<(&mut Transform, &mut Visibility), (Without<Targetable>, Without<LocalPlayer>)>,
    tips: Query<&GlobalTransform, With<RodTip>>,
) {
    let bob = (time.elapsed_secs() * 2.4).sin() * 0.03;
    lines.0.retain(|id, line| {
        let Some((_, mut angler)) = anglers.iter_mut().find(|(targetable, _)| targetable.id == *id) else {
            commands.entity(line.float).despawn();
            commands.entity(line.line).despawn();
            return false;
        };
        let (x, y) = from_bevy(angler.translation);
        let (dx, dy) = (line.at.0 - x, line.at.1 - y);
        if dx.hypot(dy) > 0.1 {
            angler.rotation = Quat::from_rotation_y(dx.atan2(dy));
        }
        let surface = water_level(&store, line.at.0, line.at.1).unwrap_or(angler.translation.y);
        let float_at = to_bevy(line.at.0, line.at.1, surface + bob);
        if let Ok((mut transform, mut visibility)) = parts.get_mut(line.float) {
            *transform = Transform::from_translation(float_at);
            *visibility = Visibility::Inherited;
        }
        if let Ok((mut transform, mut visibility)) = parts.get_mut(line.line) {
            *transform = line_between(rod_tip(&tips, angler.translation, angler.rotation), float_at);
            *visibility = Visibility::Inherited;
        }
        true
    });
}

fn forget_lines(mut commands: Commands, mut lines: ResMut<OtherLines>) {
    for (_, line) in lines.0.drain() {
        commands.entity(line.float).despawn();
        commands.entity(line.line).despawn();
    }
}
