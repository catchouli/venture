//! Water's material: Bevy's standard PBR material, extended with a shader
//! (`shaders/water.wgsl`) that ripples the surface with moving waves, so it
//! reflects the sky and catches the sun, and makes it more reflective (and
//! less see-through) at low angles.

use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::common::palette;

/// The complete water material.
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

/// Registers the water material and embeds its shader in the executable.
pub struct WaterMaterialPlugin;

impl Plugin for WaterMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/water.wgsl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
    }
}

/// The water shader (it has no settings of its own: the waves are in it).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct WaterExtension {}

impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://venture_client/world/shaders/water.wgsl".into()
    }
}

/// The water material: its colour and opacity looking straight down
/// (`palette::WATER`), and smooth, so the waves' reflections are sharp.
pub fn water_material() -> WaterMaterial {
    WaterMaterial {
        base: StandardMaterial {
            base_color: palette::WATER,
            // (Premultiplied: the shader adds reflections at full strength
            // over the see-through water.)
            alpha_mode: AlphaMode::Premultiplied,
            perceptual_roughness: 0.08,
            // Water reflects about 2% looking straight down.
            reflectance: 0.35,
            ..default()
        },
        extension: WaterExtension {},
    }
}
