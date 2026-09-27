//! Drawing the sky: two photographed skies blended, with the sun and the moon
//! drawn where they are ([`shaders/sky.wgsl`]), on the inside of a big
//! sphere that stays round the camera. It replaces Bevy's skybox, which
//! shows one cubemap. [`crate::world::lighting`] loads the skies and sets
//! what it shows each frame ([`SkyMaterial::settings`]).

use bevy::asset::embedded_asset;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::transform::TransformSystems;

/// How far the sky's sphere is (inside the camera's far plane, a kilometre).
const RADIUS: f32 = 900.0;

/// Draws the sky.
pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/sky.wgsl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(PostUpdate, stay_round_the_camera.before(TransformSystems::Propagate));
    }
}

/// The sky's look: two skies (cubemaps), and how they and the sun and moon show.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct SkyMaterial {
    /// How it all shows (set each frame).
    #[uniform(0)]
    pub settings: SkySettings,
    /// The first sky.
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub first: Handle<Image>,
    /// The second.
    #[texture(3, dimension = "cube")]
    #[sampler(4)]
    pub second: Handle<Image>,
}

/// How the sky shows (every field a `vec4` or `mat4`, so the layout suits WebGL2).
#[derive(ShaderType, Clone, Debug, Default)]
pub struct SkySettings {
    /// Turns world directions into the first sky's own.
    pub first_turn: Mat4,
    /// And the second's.
    pub second_turn: Mat4,
    /// How bright each sky is (x the first, y the second), in cd/m² for
    /// their brightness scale's 1, weighted by the blend.
    pub strengths: Vec4,
    /// Towards the sun (xyz), and the cosine of its disc's radius (w).
    pub sun: Vec4,
    /// The sun disc's radiance in cd/m² (rgb), and its glow's strength (w).
    pub sun_colour: Vec4,
    /// Towards the moon (xyz), and the cosine of its disc's radius (w).
    pub moon: Vec4,
    /// The moon disc's radiance in cd/m² (rgb).
    pub moon_colour: Vec4,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://venture_client/world/shaders/sky.wgsl".into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// The sky's sphere.
#[derive(Component)]
pub struct SkySphere;

/// The sky's sphere, facing inwards, with its material.
pub fn sky_sphere(meshes: &mut Assets<Mesh>, material: Handle<SkyMaterial>) -> impl Bundle {
    let mesh = Sphere::new(RADIUS).mesh().uv(64, 32).with_inverted_winding().expect("a sphere has triangles");
    (
        SkySphere,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        NotShadowCaster,
        NotShadowReceiver,
        Transform::default(),
    )
}

/// Keeps the sky's sphere centred on the world's camera (the one with haze).
fn stay_round_the_camera(
    camera: Option<Single<&Transform, (With<Camera3d>, With<DistanceFog>, Without<SkySphere>)>>,
    mut spheres: Query<&mut Transform, With<SkySphere>>,
) {
    let Some(camera) = camera else { return };
    for mut sphere in &mut spheres {
        sphere.translation = camera.translation;
    }
}
