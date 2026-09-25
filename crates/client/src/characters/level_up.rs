//! The level-up effect: a column of golden light rising around a character
//! when they go up a level, seen by everyone nearby.
//!
//! This is a placeholder made of simple shapes, until real effects are made
//! (see "Skills and the hotbar" in `docs/plan-combat.md`).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::characters::player::LocalPlayer;
use crate::interaction::targeting::Targetable;
use crate::network::FromServer;

/// How long the effect lasts, in seconds.
const DURATION: f32 = 1.6;
/// Colour of the light.
const COLOUR: Color = Color::srgb(1.0, 0.85, 0.35);
/// How strongly the light is added over what's behind it, at its brightest.
const PEAK_ALPHA: f32 = 0.6;
/// How tall the column of light grows, in metres.
const COLUMN_HEIGHT: f32 = 3.2;

/// Shows level-up effects.
pub struct LevelUpPlugin;

impl Plugin for LevelUpPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_meshes).add_systems(Update, (start_effects, animate_effects));
    }
}

/// The shapes the effect is made of.
#[derive(Resource)]
struct EffectMeshes {
    column: Handle<Mesh>,
    ring: Handle<Mesh>,
}

/// One part of an effect, fading out as it ages. It follows its character
/// (rather than being part of it, so rebuilding the character's body, as a
/// new look does, doesn't remove it).
#[derive(Component)]
struct LevelUpEffect {
    /// Whose level-up it is.
    character: Entity,
    /// Seconds since it started.
    age: f32,
    /// What the part does as it ages.
    part: EffectPart,
}

#[derive(Clone, Copy)]
enum EffectPart {
    /// Rises and grows taller.
    Column,
    /// Spreads out along the ground.
    Ring,
}

fn make_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(EffectMeshes {
        column: meshes.add(Cylinder::new(0.55, 1.0)),
        ring: meshes.add(Torus::new(0.45, 0.55)),
    });
}

/// Starts an effect on whoever went up a level.
fn start_effects(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    meshes: Res<EffectMeshes>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    others: Query<(Entity, &Targetable)>,
    me: Option<Single<(Entity, &LocalPlayer)>>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::PlayerChanged { id, celebrate: true, .. } = message else { continue };
        let mine = me.as_ref().filter(|me| me.1.id == *id).map(|me| me.0);
        let Some(character) = mine.or_else(|| others.iter().find(|(_, t)| t.id == *id).map(|(e, _)| e)) else {
            continue;
        };
        for (part, mesh) in [(EffectPart::Column, &meshes.column), (EffectPart::Ring, &meshes.ring)] {
            // Each part has its own material, so it can fade on its own.
            let material = materials.add(StandardMaterial {
                base_color: COLOUR.with_alpha(PEAK_ALPHA),
                emissive: LinearRgba::from(COLOUR) * 3.0,
                alpha_mode: AlphaMode::Add,
                unlit: true,
                ..default()
            });
            commands.spawn((
                LevelUpEffect { character, age: 0.0, part },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                // Light casts no shadow.
                NotShadowCaster,
            ));
        }
    }
}

/// Grows and fades each part, and removes it when it's done.
fn animate_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    characters: Query<&GlobalTransform, Without<LevelUpEffect>>,
    mut effects: Query<(Entity, &mut LevelUpEffect, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (entity, mut effect, mut transform, material) in &mut effects {
        effect.age += time.delta_secs();
        let progress = effect.age / DURATION;
        let Ok(character) = characters.get(effect.character) else {
            commands.entity(entity).despawn();
            continue;
        };
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let at = character.translation();
        // Quick to appear, then fading away.
        let strength = (progress * 8.0).min(1.0) * (1.0 - progress);
        *transform = match effect.part {
            EffectPart::Column => {
                let height = COLUMN_HEIGHT * (0.3 + 0.7 * progress.sqrt());
                Transform::from_translation(at + Vec3::Y * height / 2.0).with_scale(Vec3::new(1.0, height, 1.0))
            }
            EffectPart::Ring => {
                let spread = 1.0 + 2.5 * progress;
                Transform::from_translation(at + Vec3::Y * 0.05).with_scale(Vec3::new(spread, 1.0, spread))
            }
        };
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = COLOUR.with_alpha(PEAK_ALPHA * strength);
            material.emissive = LinearRgba::from(COLOUR) * (3.0 * strength);
        }
    }
}
