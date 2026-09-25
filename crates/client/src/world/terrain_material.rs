//! The terrain's material: Bevy's standard PBR material, extended with a
//! shader (`shaders/terrain.wgsl`) that adds procedural surface detail and
//! bare rock on steep slopes.

use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// The complete terrain material.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

/// Slope (1 minus the normal's upward part) where rock starts to show.
const ROCK_START_SLOPE: f32 = 0.28;
/// Slope where the ground is entirely rock.
const ROCK_FULL_SLOPE: f32 = 0.45;
/// How strongly the surface bumps tilt the lighting.
const BUMP_STRENGTH: f32 = 0.12;

/// Registers the terrain material and embeds its shader in the executable.
pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}

/// The terrain shader's settings.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExtension {
    /// Packed as `(rock start slope, rock full slope, bump strength, unused)`,
    /// a single `vec4` so the layout suits WebGL2 as well.
    #[uniform(100)]
    params: Vec4,
}

impl Default for TerrainExtension {
    fn default() -> Self {
        Self { params: Vec4::new(ROCK_START_SLOPE, ROCK_FULL_SLOPE, BUMP_STRENGTH, 0.0) }
    }
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://venture_client/world/shaders/terrain.wgsl".into()
    }
}
