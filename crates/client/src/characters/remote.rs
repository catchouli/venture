//! Everyone else in the world, players and NPCs alike: shown where the server
//! says they are.
//!
//! Positions arrive in snapshots 20 times a second. To look smooth, others are
//! shown slightly in the past, interpolating between the two snapshots around
//! that moment (standard entity interpolation).
//!
//! NPCs are drawn in their content appearance. When the content changes,
//! their bodies are rebuilt in the new colours.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{EntityId, EntityKind, ServerMessage};

use crate::characters::model::{CharacterAssets, Gait, Look, appearance_look, rebuild_body, spawn_character};
use crate::characters::nameplates::Nameplate;
use crate::characters::player::LocalPlayer;
use crate::common::coords::{from_bevy, to_bevy};
use crate::content::ContentStore;
use crate::interaction::targeting::{TargetKind, Targetable};
use crate::network::{ClientState, FromServer};
use crate::world::query::ground_height;
use crate::world::streaming::ChunkStore;

/// How far in the past others are shown, in seconds (two server ticks).
const INTERPOLATION_DELAY: f32 = 0.1;
/// How many snapshots to remember per entity.
const MAX_SAMPLES: usize = 16;
/// How quickly other characters turn to face where they're going (per second).
const TURN_RATE: f32 = 12.0;

/// Shows everyone else.
pub struct RemotePlayersPlugin;

impl Plugin for RemotePlayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteEntities>()
            .init_resource::<NpcLooks>()
            .add_systems(Update, (follow_server_events, restyle_npcs_when_content_changes, interpolate).chain())
            .add_systems(OnEnter(ClientState::Disconnected), remove_everyone);
    }
}

/// The character entity for each other entity in the world.
#[derive(Resource, Default)]
struct RemoteEntities(HashMap<EntityId, Entity>);

/// Looks made from content appearances, by appearance id (so NPCs sharing an
/// appearance share materials).
#[derive(Resource, Default)]
struct NpcLooks(HashMap<String, Look>);

/// Someone else's character.
#[derive(Component)]
struct RemoteEntity {
    /// Positions from recent snapshots, with the (client) time each arrived, oldest first.
    samples: VecDeque<(f32, Point)>,
    /// The NPC's appearance id, if it's an NPC.
    appearance: Option<String>,
}

fn follow_server_events(
    mut commands: Commands,
    time: Res<Time>,
    mut events: MessageReader<FromServer>,
    assets: Res<CharacterAssets>,
    content: Res<ContentStore>,
    mut looks: ResMut<NpcLooks>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    store: Res<ChunkStore>,
    mut remotes: ResMut<RemoteEntities>,
    me: Option<Single<&LocalPlayer>>,
    mut others: Query<&mut RemoteEntity>,
) {
    let now = time.elapsed_secs();
    let my_id = me.map(|me| me.id);
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::EntityAppeared { id, name, kind, position: (x, y) } => {
                if remotes.0.contains_key(id) || Some(*id) == my_id {
                    continue;
                }
                let (look, target_kind, appearance) = match kind {
                    EntityKind::Player => (assets.player_look(id.0), TargetKind::Player, None),
                    EntityKind::Npc { appearance } => {
                        let look = npc_look(appearance, &content, &mut looks, &mut materials, &assets);
                        (look, TargetKind::Npc, Some(appearance.clone()))
                    }
                };
                let translation = to_bevy(*x, *y, ground_height(&store, *x, *y).unwrap_or(0.0));
                let entity = spawn_character(&mut commands, &assets, Transform::from_translation(translation), &look);
                commands.entity(entity).insert((
                    Name::new(name.clone()),
                    RemoteEntity { samples: VecDeque::from([(now, (*x, *y))]), appearance },
                    Nameplate(name.clone()),
                    Targetable { id: *id, kind: target_kind },
                ));
                remotes.0.insert(*id, entity);
            }
            ServerMessage::EntityLeft { id } => {
                if let Some(entity) = remotes.0.remove(id) {
                    commands.entity(entity).despawn();
                }
            }
            ServerMessage::Snapshot { entities, .. } => {
                for state in entities {
                    let Some(mut other) = remotes.0.get(&state.id).and_then(|&e| others.get_mut(e).ok()) else {
                        continue;
                    };
                    other.samples.push_back((now, (state.x, state.y)));
                    if other.samples.len() > MAX_SAMPLES {
                        other.samples.pop_front();
                    }
                }
            }
            _ => {}
        }
    }
}

/// The look for an NPC appearance: made from the content (once per
/// appearance), or the plain player look if the content hasn't arrived yet.
fn npc_look(
    appearance: &str,
    content: &ContentStore,
    looks: &mut NpcLooks,
    materials: &mut Assets<StandardMaterial>,
    assets: &CharacterAssets,
) -> Look {
    if let Some(look) = looks.0.get(appearance) {
        return look.clone();
    }
    let Some(definition) = content.appearances.get(appearance) else { return assets.player_look(0) };
    let look = appearance_look(materials, definition);
    looks.0.insert(appearance.to_string(), look.clone());
    look
}

/// Rebuilds NPC bodies in their (new) appearance whenever the content changes.
fn restyle_npcs_when_content_changes(
    mut commands: Commands,
    content: Res<ContentStore>,
    assets: Res<CharacterAssets>,
    mut looks: ResMut<NpcLooks>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    npcs: Query<(Entity, &RemoteEntity)>,
) {
    if !content.is_changed() || content.is_added() {
        return;
    }
    looks.0.clear();
    for (entity, npc) in &npcs {
        let Some(appearance) = &npc.appearance else { continue };
        let look = npc_look(appearance, &content, &mut looks, &mut materials, &assets);
        rebuild_body(&mut commands, entity, &assets, &look);
    }
}

/// Places everyone else where they were [`INTERPOLATION_DELAY`] ago.
fn interpolate(time: Res<Time>, store: Res<ChunkStore>, mut others: Query<(&RemoteEntity, &mut Transform, &mut Gait)>) {
    let dt = time.delta_secs();
    let shown_time = time.elapsed_secs() - INTERPOLATION_DELAY;
    for (other, mut transform, mut gait) in &mut others {
        let (x, y) = position_at(&other.samples, shown_time);
        let previous = from_bevy(transform.translation);
        let (moved_x, moved_y) = (x - previous.0, y - previous.1);
        let moved = moved_x.hypot(moved_y);

        let height = ground_height(&store, x, y).unwrap_or(transform.translation.y);
        transform.translation = to_bevy(x, y, height);
        if moved > 1e-4 {
            let facing = Quat::from_rotation_y(moved_x.atan2(moved_y));
            transform.rotation = transform.rotation.slerp(facing, (TURN_RATE * dt).min(1.0));
        }
        gait.speed = if dt > 0.0 { moved / dt } else { 0.0 };
    }
}

/// The position at `time`, interpolated between the samples around it (or
/// the nearest end if `time` is outside them).
fn position_at(samples: &VecDeque<(f32, Point)>, time: f32) -> Point {
    let Some(&(_, last)) = samples.back() else { return (0.0, 0.0) };
    for pair in samples.iter().collect::<Vec<_>>().windows(2) {
        let (&(t0, p0), &(t1, p1)) = (pair[0], pair[1]);
        if time >= t0 && time <= t1 {
            let f = if t1 > t0 { (time - t0) / (t1 - t0) } else { 1.0 };
            return (p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f);
        }
    }
    if samples.front().is_some_and(|&(t0, _)| time < t0) { samples[0].1 } else { last }
}

fn remove_everyone(mut commands: Commands, mut remotes: ResMut<RemoteEntities>) {
    for (_, entity) in remotes.0.drain() {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_interpolated_between_samples() {
        let samples = VecDeque::from([(0.0, (0.0, 0.0)), (1.0, (10.0, 0.0))]);
        assert_eq!(position_at(&samples, 0.5), (5.0, 0.0));
        assert_eq!(position_at(&samples, -1.0), (0.0, 0.0));
        assert_eq!(position_at(&samples, 2.0), (10.0, 0.0));
    }
}
