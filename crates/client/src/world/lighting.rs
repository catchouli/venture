//! The sky and lighting, by the time of day ([`TimeOfDay`]; see
//! `docs/plan-day-night.md`).
//!
//! The sky comes from the content (`skies/day/`, made from an HDR panorama by
//! `import_sky`; see `docs/textures.md`): a skybox, and the light it casts
//! (image-based lighting: an irradiance cube for diffuse light and a
//! prefiltered radiance cube for reflections), baked ahead of time because
//! WebGL2 can't filter environment maps at run time.
//!
//! Each frame, by where the sun is ([`venture_simulation::DayCycle`]):
//! - the one shadow-casting light is the sun, coloured and dimmed by the air
//!   its light passes through (see [`crate::world::daylight`]); once it's
//!   below the horizon, the moon (a dim, cool light) takes over, the swap
//!   happening in twilight, when both are faint;
//! - the sky (and the light it casts) dims through dusk to a dim night, and
//!   the exposure opens up a few stops, so night is dark but playable;
//! - distant haze takes the sky's horizon colour, warmed by a low sun.
//!
//! For now there's one sky, turned so its sun's glow follows the sun; the
//! plan is to blend between skies for dawn, day, dusk and night.
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

use crate::characters::player::GameRules;
use crate::common::coords::to_bevy;
use crate::common::palette;
use crate::content::{CONTENT_SOURCE, ContentStore};
use crate::world::daylight::{exposure, night, sky_level, smoothstep, sun_colour, sun_strength};
use crate::world::time_of_day::TimeOfDay;

/// The sky the world has (a folder in the content's `skies/`).
const SKY: &str = "day";
/// How bright the sky is: its brightness scale's 1 in candelas per square
/// metre (for both the skybox and the light it casts).
const SKY_BRIGHTNESS: f32 = 1000.0;
/// How bright the sun is, in lux. (About 8:1 over the day sky's light on open
/// ground once the sky is up: bright enough for clear shadows.)
const SUN_ILLUMINANCE: f32 = 14_000.0;
/// How bright the moon is, in lux (with the night's exposure, a sixth or so
/// of the day's light: a film night rather than a real one).
const MOON_ILLUMINANCE: f32 = 450.0;
/// The moon's colour.
const MOON_COLOUR: Vec3 = Vec3::new(0.6, 0.7, 1.0);
/// The flat ambient light without a sky.
const PLAIN_AMBIENT: f32 = 450.0;

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
            .add_systems(Update, (load_sky, finish_sky, dress_cameras, light_by_time).chain());
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
    /// The direction of the sun in its picture (Bevy's axes).
    sun: Vec3,
    /// Its horizon's colour, on its brightness scale.
    horizon: Vec3,
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
        DirectionalLight { illuminance: SUN_ILLUMINANCE, shadow_maps_enabled: true, ..default() },
        // (Placed by the time of day: see `light_by_time`.)
        Transform::default(),
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

/// Once the sky's picture has loaded: makes it a cubemap, and gets it ready
/// for cameras.
fn finish_sky(asset_server: Res<AssetServer>, mut images: ResMut<Assets<Image>>, mut state: ResMut<SkyState>) {
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

    let sky = Sky {
        skybox: Skybox { image: Some(picture.clone()), brightness: SKY_BRIGHTNESS, rotation: Quat::IDENTITY },
        light: EnvironmentMapLight {
            diffuse_map: diffuse.clone(),
            specular_map: specular.clone(),
            intensity: SKY_BRIGHTNESS,
            rotation: Quat::IDENTITY,
            affects_lightmapped_mesh_diffuse: true,
        },
        sun: Vec3::from(def.sun),
        horizon: Vec3::from(def.horizon),
    };
    info!("The sky is ready.");
    *state = SkyState::Ready(sky);
}

/// Puts the sky on the world's cameras (the ones with haze), including
/// cameras that appear later.
fn dress_cameras(
    mut commands: Commands,
    state: Res<SkyState>,
    cameras: Query<Entity, (With<Camera3d>, With<DistanceFog>, Without<Skybox>)>,
) {
    let SkyState::Ready(sky) = &*state else { return };
    for camera in &cameras {
        commands.entity(camera).insert((sky.skybox.clone(), sky.light.clone()));
    }
}

/// Lights the world for the time of day: the sun (or the moon), the sky and
/// its light, the exposure, and the haze.
fn light_by_time(
    clock: Res<TimeOfDay>,
    rules: Res<GameRules>,
    state: Res<SkyState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    sun: Single<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut cameras: Query<
        (&mut DistanceFog, Option<&mut Skybox>, Option<&mut EnvironmentMapLight>, &mut Exposure),
        With<Camera3d>,
    >,
) {
    let day = rules.0.day;
    let (sun_at, moon_at) = (day.sun(clock.0), day.moon(clock.0));
    let height = sun_at.height;
    let towards = |position: venture_simulation::SkyPosition| {
        let (east, south, up) = position.direction();
        to_bevy(east, south, up)
    };

    // One light: the sun, handing over to the moon in twilight (when both
    // are faint), so the shadows never jump.
    let (mut light, mut place) = sun.into_inner();
    let sunlight = sun_colour(height);
    let moonlight = MOON_ILLUMINANCE * smoothstep(-3f32.to_radians(), -9f32.to_radians(), height);
    let (direction, colour, illuminance) = if height > -3f32.to_radians() {
        (towards(sun_at), sunlight, SUN_ILLUMINANCE * sun_strength(height))
    } else {
        (towards(moon_at), MOON_COLOUR, moonlight)
    };
    place.look_to(-direction, Vec3::Y);
    light.color = Color::linear_rgb(colour.x, colour.y, colour.z);
    light.illuminance = illuminance;

    let level = sky_level(height);
    let ev100 = exposure(height);
    let scale = Exposure { ev100 }.exposure();
    // Haze is the horizon's colour, warmed by a low sun.
    let warmth = 0.5 * (1.0 - smoothstep(0.0, 20f32.to_radians(), height)) * (1.0 - night(height));
    let tint = Vec3::ONE.lerp(sunlight, warmth);
    let (horizon, turn) = match &*state {
        SkyState::Ready(sky) => {
            // Turn the sky so its sun's glow is where the sun is.
            let (east, south, _) = sun_at.direction();
            (sky.horizon, Quat::from_rotation_y(east.atan2(south) - sky.sun.x.atan2(sky.sun.z)))
        }
        _ => (Vec3::from(palette::HAZE.to_linear().to_f32_array_no_alpha()) / (SKY_BRIGHTNESS * scale), Quat::IDENTITY),
    };
    let haze = horizon * tint * SKY_BRIGHTNESS * level * scale;
    let haze = Color::linear_rgb(haze.x, haze.y, haze.z);
    if matches!(*state, SkyState::Ready(_)) {
        ambient.brightness = 0.0;
    } else {
        ambient.brightness = PLAIN_AMBIENT * level;
        clear.0 = haze;
    }

    for (mut fog, skybox, environment, mut camera_exposure) in &mut cameras {
        fog.color = haze;
        fog.directional_light_color = Color::srgba(1.0, 0.9, 0.7, 0.4 * (1.0 - night(height)));
        camera_exposure.ev100 = ev100;
        if let Some(mut skybox) = skybox {
            skybox.brightness = SKY_BRIGHTNESS * level;
            skybox.rotation = turn;
        }
        if let Some(mut environment) = environment {
            environment.intensity = SKY_BRIGHTNESS * level;
            environment.rotation = turn;
        }
    }
}
