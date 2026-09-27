//! The sky and lighting.
//!
//! The sky comes from the content (`skies/day/`, made from an HDR panorama by
//! `import_sky`; see `docs/textures.md`): a skybox, and the light it casts
//! (image-based lighting: an irradiance cube for diffuse light and a
//! prefiltered radiance cube for reflections), baked ahead of time because
//! WebGL2 can't filter environment maps at run time. The sky is turned so
//! its sun is where the game's sun is (the south-west, mid-afternoon), the
//! sun rises to the sky's own sun's height, and distant haze takes the
//! colour of its horizon.
//!
//! Until the sky arrives (or if the content has none), there's a plain haze
//! colour and a flat ambient light instead.

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::light::{CascadeShadowConfigBuilder, Skybox};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use venture_content::sky::SkyDef;

use crate::common::palette;
use crate::content::{CONTENT_SOURCE, ContentStore};

/// The sky the world has (a folder in the content's `skies/`).
const SKY: &str = "day";
/// How bright the sky is: its brightness scale's 1 in candelas per square
/// metre (for both the skybox and the light it casts).
const SKY_BRIGHTNESS: f32 = 1000.0;
/// The flat ambient light without a sky.
const PLAIN_AMBIENT: f32 = 450.0;
/// Where the sun is, round the horizon: towards the south-west (x east, z south).
const SUN_BEARING: Vec2 = Vec2::new(-1.0, 1.0);

/// Lights the scene.
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(palette::HAZE))
            // Soft, warm fill light, so shadowed slopes aren't black (until there's a sky).
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.9, 0.9, 0.8),
                brightness: PLAIN_AMBIENT,
                ..default()
            })
            .init_resource::<SkyState>()
            .add_systems(Startup, spawn_sun)
            .add_systems(Update, (load_sky, finish_sky, dress_cameras).chain());
    }
}

/// The sun.
#[derive(Component)]
struct Sun;

/// The sky, as it loads.
#[derive(Resource, Default)]
pub enum SkyState {
    /// Not asked for yet.
    #[default]
    Waiting,
    /// Its picture is loading; its light is ready.
    Loading {
        /// The skybox picture, loading.
        picture: Handle<Image>,
        /// Its sun and horizon.
        def: SkyDef,
        /// Its irradiance cube.
        diffuse: Handle<Image>,
        /// Its radiance cube.
        specular: Handle<Image>,
    },
    /// Ready to put on cameras.
    Ready(Sky),
    /// The content has no sky (or it couldn't be loaded).
    None,
}

impl SkyState {
    /// Whether the sky is up, or won't be (for the loading screen).
    pub fn settled(&self) -> bool {
        matches!(self, SkyState::Ready(_) | SkyState::None)
    }
}

/// A loaded sky, as cameras wear it.
#[derive(Clone)]
pub struct Sky {
    skybox: Skybox,
    light: EnvironmentMapLight,
    haze: Color,
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
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

/// Once the content has the sky: builds its light, and asks for its picture.
fn load_sky(
    content: Res<ContentStore>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<SkyState>,
) {
    if !matches!(*state, SkyState::Waiting) || content.version.is_none() {
        return;
    }
    let Some(files) = content.skies.get(SKY) else {
        *state = SkyState::None;
        return;
    };
    let (Some(def), Some(light)) = (&files.def, &files.light) else {
        *state = SkyState::None;
        return;
    };
    let diffuse = images.add(cubemap(light.irradiance_size, 1, &light.irradiance));
    let specular = images.add(cubemap(light.radiance_size, light.radiance_mips, &light.radiance));
    let picture = asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = true)
        .load::<Image>(format!("{CONTENT_SOURCE}://skies/{SKY}/sky.jpg"));
    *state = SkyState::Loading { picture, def: def.clone(), diffuse, specular };
}

/// A cubemap of RGB9E5 texels (each face in turn, with all its mipmaps).
fn cubemap(size: u32, mips: u32, texels: &[u32]) -> Image {
    let data = texels.iter().flat_map(|texel| texel.to_le_bytes()).collect();
    let extent = Extent3d { width: size, height: size, depth_or_array_layers: 6 };
    let mut image =
        Image::new_uninit(extent, TextureDimension::D2, TextureFormat::Rgb9e5Ufloat, RenderAssetUsages::RENDER_WORLD);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mips;
    image.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    image.sampler = ImageSampler::linear();
    image
}

/// Once the sky's picture has loaded: makes it a cubemap, turns the sky and
/// the sun to match the game's, and gets it ready for cameras.
fn finish_sky(
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<SkyState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun: Single<&mut Transform, With<Sun>>,
) {
    let SkyState::Loading { picture, def, diffuse, specular } = &*state else { return };
    if asset_server.load_state(picture).is_failed() {
        warn!("The sky's picture couldn't be loaded, so the sky stays plain.");
        *state = SkyState::None;
        return;
    }
    if !asset_server.is_loaded_with_dependencies(picture) {
        return;
    }
    let Some(mut image) = images.get_mut(picture) else { return };
    if let Err(error) = image.reinterpret_stacked_2d_as_array(6) {
        warn!("The sky's picture isn't six faces stacked ({error:?}), so the sky stays plain.");
        *state = SkyState::None;
        return;
    }
    image.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });

    // Turn the sky round so its sun is on the game's bearing, and put the
    // sun where the sky shows it.
    let (x, y, z) = def.sun;
    let turn = Quat::from_rotation_y(SUN_BEARING.x.atan2(SUN_BEARING.y) - x.atan2(z));
    let towards_sun = turn * Vec3::new(x, y, z);
    sun.look_to(-towards_sun, Vec3::Y);
    ambient.brightness = 0.0;

    let (r, g, b) = def.horizon;
    let exposure = Exposure::default().exposure();
    let scale = SKY_BRIGHTNESS * exposure;
    let haze = Color::linear_rgb(r * scale, g * scale, b * scale);
    let sky = Sky {
        skybox: Skybox { image: Some(picture.clone()), brightness: SKY_BRIGHTNESS, rotation: turn },
        light: EnvironmentMapLight {
            diffuse_map: diffuse.clone(),
            specular_map: specular.clone(),
            intensity: SKY_BRIGHTNESS,
            rotation: turn,
            affects_lightmapped_mesh_diffuse: true,
        },
        haze,
    };
    info!("The sky is ready.");
    *state = SkyState::Ready(sky);
}

/// Puts the sky on the world's cameras (the ones with haze), including
/// cameras that appear later.
fn dress_cameras(
    mut commands: Commands,
    state: Res<SkyState>,
    mut cameras: Query<(Entity, &mut DistanceFog), (With<Camera3d>, Without<Skybox>)>,
) {
    let SkyState::Ready(sky) = &*state else { return };
    for (camera, mut fog) in &mut cameras {
        fog.color = sky.haze;
        commands.entity(camera).insert((sky.skybox.clone(), sky.light.clone()));
    }
}
