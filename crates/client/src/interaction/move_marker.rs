//! The marker left where the player clicks to move: a glowing crest that
//! hovers over the spot, above a glowing ring on the ground. It vanishes
//! when the character arrives.
//!
//! Original placeholder art, drawn in code.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::characters::player::LocalPlayer;
use crate::common::coords::from_bevy;
use crate::common::procedural_image::{distance_to_segment, paint_image};

/// Width of the crest, in metres.
const CREST_SIZE: f32 = 1.0;
/// Height of the crest's centre above the ground, in metres.
const CREST_HEIGHT: f32 = 1.0;
/// Width of the ground ring, in metres.
const RING_SIZE: f32 = 1.3;
/// How long the marker takes to appear and to vanish, in seconds.
const TRANSITION_TIME: f32 = 0.18;
/// How much brighter than white the marker is drawn, which makes it bloom.
const GLOW_BRIGHTNESS: f32 = 2.5;
/// The marker's glow colour (linear RGB).
const GLOW: Vec3 = Vec3::new(0.15, 0.45, 1.0);

/// Places, animates and removes move markers.
pub struct MoveMarkerPlugin;

impl Plugin for MoveMarkerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarkerAssets>().add_systems(Update, (animate_markers, vanish_on_arrival));
    }
}

/// A move marker.
#[derive(Component, Default)]
pub struct MoveMarker {
    /// Seconds since it appeared.
    age: f32,
    /// Seconds since it started vanishing, once it has.
    vanishing: Option<f32>,
}

/// The upright part of the marker, which turns to face the camera.
#[derive(Component)]
struct Crest;

#[derive(Resource)]
pub struct MarkerAssets {
    crest_mesh: Handle<Mesh>,
    ring_mesh: Handle<Mesh>,
    crest_material: Handle<StandardMaterial>,
    ring_material: Handle<StandardMaterial>,
}

impl FromWorld for MarkerAssets {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();
        let crest_image = images.add(paint_image(256, RenderAssetUsages::RENDER_WORLD, crest_pixel));
        let ring_image = images.add(paint_image(256, RenderAssetUsages::RENDER_WORLD, ring_pixel));

        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let crest_mesh = meshes.add(Rectangle::new(CREST_SIZE, CREST_SIZE));
        let ring_mesh = meshes.add(Rectangle::new(RING_SIZE, RING_SIZE));

        // Unlit, so it looks self-lit. Blended rather than additive, so it
        // still shows up against bright ground in daylight.
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut glowing = |texture: Handle<Image>| {
            materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                // Brighter than white, so the camera's bloom makes it glow.
                base_color: Color::linear_rgb(GLOW_BRIGHTNESS, GLOW_BRIGHTNESS, GLOW_BRIGHTNESS),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                cull_mode: None,
                fog_enabled: false,
                ..default()
            })
        };
        Self { crest_mesh, ring_mesh, crest_material: glowing(crest_image), ring_material: glowing(ring_image) }
    }
}

/// Places a marker on the ground at `position`, replacing any existing marker.
pub fn place_marker(
    commands: &mut Commands,
    assets: &MarkerAssets,
    existing: impl IntoIterator<Item = Entity>,
    position: Vec3,
) {
    for marker in existing {
        commands.entity(marker).despawn();
    }
    commands
        .spawn((
            Name::new("Move marker"),
            MoveMarker::default(),
            Transform::from_translation(position).with_scale(Vec3::ZERO),
            Visibility::default(),
        ))
        .with_children(|marker| {
            marker.spawn((
                Crest,
                NotShadowCaster,
                Mesh3d(assets.crest_mesh.clone()),
                MeshMaterial3d(assets.crest_material.clone()),
                Transform::from_xyz(0.0, CREST_HEIGHT, 0.0),
            ));
            marker.spawn((
                NotShadowCaster,
                Mesh3d(assets.ring_mesh.clone()),
                MeshMaterial3d(assets.ring_material.clone()),
                Transform::from_xyz(0.0, 0.06, 0.0).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            ));
            // A soft blue light, so the marker lights up the ground around it.
            marker.spawn((
                PointLight {
                    color: Color::linear_rgb(GLOW.x, GLOW.y, GLOW.z),
                    intensity: 6_000.0,
                    range: 5.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, CREST_HEIGHT, 0.0),
            ));
        });
}

/// Grows markers in, bobs and pulses them, turns crests to face the camera,
/// and shrinks away vanishing markers.
fn animate_markers(
    mut commands: Commands,
    time: Res<Time>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut markers: Query<(Entity, &mut MoveMarker, &mut Transform, &Children), Without<Crest>>,
    mut crests: Query<&mut Transform, With<Crest>>,
) {
    let dt = time.delta_secs();
    let (camera_x, camera_y) = from_bevy(camera.translation());
    for (entity, mut marker, mut transform, children) in &mut markers {
        marker.age += dt;
        if let Some(vanishing) = marker.vanishing.as_mut() {
            *vanishing += dt;
        }
        let appear = (marker.age / TRANSITION_TIME).min(1.0);
        let vanish = marker.vanishing.map_or(1.0, |t| 1.0 - t / TRANSITION_TIME);
        if vanish <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let pulse = 1.0 + 0.04 * (marker.age * TAU * 1.2).sin();
        transform.scale = Vec3::splat(ease_out(appear) * vanish * pulse);

        let (x, y) = from_bevy(transform.translation);
        let facing = (camera_x - x).atan2(camera_y - y);
        for child in children.iter() {
            if let Ok(mut crest) = crests.get_mut(child) {
                crest.translation.y = CREST_HEIGHT + 0.05 * (marker.age * PI).sin();
                crest.rotation = Quat::from_rotation_y(facing);
            }
        }
    }
}

/// Starts vanishing the marker once the player has stopped moving.
fn vanish_on_arrival(player: Option<Single<&LocalPlayer>>, mut markers: Query<&mut MoveMarker>) {
    if player.is_some_and(|player| player.movement.is_moving()) {
        return;
    }
    for mut marker in &mut markers {
        marker.vanishing.get_or_insert(0.0);
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// Turns a glow intensity into a colour: saturated blue, going white where
/// it's brightest, and see-through where it's faint.
fn glow_colour(intensity: f32) -> [f32; 4] {
    let white = ((intensity - 0.9) * 2.0).clamp(0.0, 0.8);
    let colour = GLOW.lerp(Vec3::ONE, white);
    [colour.x, colour.y, colour.z, (intensity * 1.2).clamp(0.0, 1.0)]
}

/// A soft glowing line: 1 on the line, fading out over `width`.
fn glow_line(distance: f32, width: f32) -> f32 {
    (-(distance / width).powi(2)).exp()
}

/// The crest: a ring with four compass points, an inner ring and a glowing lens.
fn crest_pixel(u: f32, v: f32) -> [f32; 4] {
    let p = Vec2::new(u, v);
    let r = p.length();
    let mut glow = glow_line(r - 0.74, 0.045) + 0.8 * glow_line(r - 0.5, 0.03);

    // Four compass points, sharp towards the outside.
    for direction in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
        let along = p.dot(direction);
        let across = p.perp_dot(direction).abs();
        if (0.58..0.98).contains(&along) {
            let half_width = 0.13 * (0.98 - along) / 0.4;
            glow += glow_line((across - half_width).max(0.0), 0.02);
        }
    }

    // A tall, lens-shaped eye in the middle, with a bright core.
    let lens = ((u / 0.11).powi(2) + (v / 0.3).powi(2)).sqrt();
    glow += 0.9 * glow_line(lens - 1.0, 0.18)
        + 1.2 * glow_line(distance_to_segment(p, Vec2::new(0.0, -0.18), Vec2::new(0.0, 0.18)), 0.035);

    // A faint halo over the whole disc.
    glow += 0.35 * (1.0 - r / 0.95).max(0.0);
    glow_colour(glow)
}

/// The ground ring: a bright ring with a faint glow inside.
fn ring_pixel(u: f32, v: f32) -> [f32; 4] {
    let r = Vec2::new(u, v).length();
    glow_colour(glow_line(r - 0.72, 0.08) + 0.25 * (1.0 - r / 0.72).max(0.0) * f32::from(r < 0.72))
}
