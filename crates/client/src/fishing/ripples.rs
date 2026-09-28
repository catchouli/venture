//! Rings spreading on the water: where a line lands, at a nibble (small),
//! and at a bite (big, and several), so they read at a glance.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{FishingState, ServerMessage};

use crate::common::coords::to_bevy;
use crate::fishing::Angling;
use crate::network::{ClientState, FromServer};
use crate::world::query::water_level;
use crate::world::streaming::ChunkStore;

/// How high above the water rings are drawn, in metres.
const LIFT: f32 = 0.02;

/// Shows ripples.
pub struct RipplesPlugin;

impl Plugin for RipplesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_ring)
            .add_systems(Update, (make_ripples, spread).chain().run_if(in_state(ClientState::Playing)));
    }
}

/// The ring's mesh (a metre across).
#[derive(Resource)]
struct Ring(Handle<Mesh>);

/// A spreading ring.
#[derive(Component)]
struct Ripple {
    /// Seconds since it started (negative: it hasn't yet).
    age: f32,
    /// How long it lasts, in seconds.
    life: f32,
    /// How wide it grows, in metres.
    size: f32,
    /// How strong it starts (its opacity).
    strength: f32,
}

fn make_ring(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // An annulus lies in the XY plane; the ripples turn it flat on the water.
    commands.insert_resource(Ring(meshes.add(Annulus::new(0.42, 0.5).mesh().resolution(40))));
}

fn make_ripples(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    angling: Res<Angling>,
    store: Res<ChunkStore>,
    ring: Res<Ring>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Fishing(state) = message else { continue };
        let (at, rings): (Point, &[(f32, f32, f32, f32)]) = match state {
            // (delay, life, size, strength)
            FishingState::Waiting { at, .. } => (*at, &[(0.0, 1.2, 1.6, 0.6), (0.2, 1.4, 2.4, 0.35)]),
            FishingState::Nibble => match &angling.line {
                Some(line) => (line.at, &[(0.0, 0.7, 0.7, 0.4)]),
                None => continue,
            },
            FishingState::Bite => match &angling.line {
                Some(line) => (line.at, &[(0.0, 1.0, 2.0, 0.8), (0.15, 1.2, 3.0, 0.6), (0.3, 1.4, 4.0, 0.4)]),
                None => continue,
            },
            _ => continue,
        };
        let Some(surface) = water_level(&store, at.0, at.1) else { continue };
        for &(delay, life, size, strength) in rings {
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(0.9, 0.95, 1.0, 0.0),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            commands.spawn((
                Ripple { age: -delay, life, size, strength },
                Mesh3d(ring.0.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(to_bevy(at.0, at.1, surface + LIFT))
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::ZERO),
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
            ));
        }
    }
}

/// Grows each ring and fades it out, then removes it.
fn spread(
    mut commands: Commands,
    time: Res<Time>,
    mut ripples: Query<(Entity, &mut Ripple, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, mut ripple, mut transform, material) in &mut ripples {
        ripple.age += time.delta_secs();
        if ripple.age < 0.0 {
            continue;
        }
        let t = ripple.age / ripple.life;
        if t >= 1.0 {
            materials.remove(&material.0);
            commands.entity(entity).despawn();
            continue;
        }
        // Fast at first, slowing as it spreads.
        let grown = 1.0 - (1.0 - t) * (1.0 - t);
        transform.scale = Vec3::splat(0.1 + ripple.size * grown);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color.set_alpha(ripple.strength * (1.0 - t));
        }
    }
}
