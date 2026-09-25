//! The sky and lighting.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

use crate::common::palette;

/// Lights the scene.
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(palette::HAZE))
            // Soft, warm fill light, so shadowed slopes aren't black.
            .insert_resource(GlobalAmbientLight { color: Color::srgb(0.9, 0.9, 0.8), brightness: 450.0, ..default() })
            .add_systems(Startup, spawn_sun);
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.85),
            illuminance: light_consts::lux::AMBIENT_DAYLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        // Mid-afternoon sun from the south-west, low enough for long shadows.
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -2.4, -0.7, 0.0)),
        CascadeShadowConfigBuilder { maximum_distance: 200.0, first_cascade_far_bound: 25.0, ..default() }.build(),
    ));
}
