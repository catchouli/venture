//! The sky and lighting, by the time of day ([`TimeOfDay`]; see
//! `docs/plan-day-night.md`).
//!
//! The skies come from the content (`skies/<name>/`, made from HDR
//! panoramas by `import_sky`, with their suns painted out; see
//! `docs/textures.md`): each a picture, and the light it casts (image-based
//! lighting: an irradiance cube for diffuse light and a prefiltered radiance
//! cube for reflections), baked ahead of time because WebGL2 can't filter
//! environment maps at run time. `skies/cycle.ron` says which sky shows at
//! which time of day, and how bright ([`SkyCycle`]).
//!
//! Each frame, by the time of day:
//! - the sky shows the two keyframe skies either side of the time, blended
//!   ([`crate::world::sky`]), with the sun and moon drawn where they are;
//! - the light the sky casts is the two skies' light, blended too (on the
//!   CPU, whenever the blend has moved on enough: see [`blend_light`]);
//! - the one shadow-casting light is the sun, coloured and dimmed by the air
//!   its light passes through (see [`crate::world::daylight`]); once it's
//!   below the horizon, the moon (a dim, cool light) takes over, the swap
//!   happening in twilight, when both are faint;
//! - the exposure opens up a few stops at night, so night is dark but
//!   playable; distant haze takes the skies' horizon colour, warmed by a low
//!   sun.
//!
//! Content without a cycle has one sky, `day`, dimmed at night. Until the
//! skies arrive (or if the content has none), there's a plain haze colour
//! and a flat ambient light instead.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use venture_content::sky::{SkyCycle, SkyKeyframe, SkyLight, SkyTurn, from_rgb9e5, rgb9e5};
use venture_simulation::SkyPosition;

use crate::characters::player::GameRules;
use crate::common::coords::to_bevy;
use crate::common::palette;
use crate::content::{CONTENT_SOURCE, ContentStore};
use crate::world::daylight::{exposure, night, sky_level, smoothstep, sun_colour, sun_strength};
use crate::world::sky::{SkyMaterial, SkySettings, sky_sphere};
use crate::world::time_of_day::TimeOfDay;

/// The sky used when the content has no cycle (a folder in `skies/`).
const DEFAULT_SKY: &str = "day";
/// How bright the sky is: its brightness scale's 1 in candelas per square
/// metre (for both the sky and the light it casts), at a brightness of 1.
const SKY_BRIGHTNESS: f32 = 1000.0;
/// How bright the sun is, in lux. (About 8:1 over the day sky's light on open
/// ground: bright enough for clear shadows.)
const SUN_ILLUMINANCE: f32 = 14_000.0;
/// How bright the moon is, in lux (with the night's exposure, a sixth or so
/// of the day's light: a film night rather than a real one).
const MOON_ILLUMINANCE: f32 = 450.0;
/// The moon's colour.
const MOON_COLOUR: Vec3 = Vec3::new(0.6, 0.7, 1.0);
/// The sun's disc: its radius (degrees), and its radiance (cd/m²) overhead.
const SUN_DISC: (f32, f32) = (0.8, 1.5e6);
/// The moon's disc: its radius (degrees, bigger than life), and its radiance.
const MOON_DISC: (f32, f32) = (1.3, 5000.0);
/// Where the pole star is: north, this many degrees up (the stars turn round it).
const POLE_HEIGHT: f32 = 50.0;
/// How much the sky's blend (or brightness) must change before its light is
/// blended again.
const LIGHT_STEP: f32 = 0.01;
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
            .add_systems(Update, (load_skies, finish_skies, dress_cameras, light_by_time, blend_light).chain());
    }
}

/// The sun (and, at night, the moon: it's the same light).
#[derive(Component)]
struct Sun;

/// The skies, as they load.
#[derive(Resource, Default)]
pub enum SkyState {
    /// Not asked for yet.
    #[default]
    Waiting,
    /// Their pictures are loading.
    Loading {
        /// Each sky, by name.
        skies: HashMap<String, LoadedSky>,
        /// Which shows when.
        cycle: SkyCycle,
        /// Whether the cycle is the content's own (rather than one sky, dimmed at night).
        from_content: bool,
    },
    /// Ready.
    Ready(Skies),
    /// The content has no skies (or they couldn't be loaded).
    None,
}

impl SkyState {
    /// Whether the skies are up, or won't be (for the loading screen).
    pub fn settled(&self) -> bool {
        matches!(self, SkyState::Ready(_) | SkyState::None)
    }
}

/// A sky, loaded.
pub struct LoadedSky {
    /// Its picture (a cubemap, once loaded).
    picture: Handle<Image>,
    /// The direction of the sun in its picture (Bevy's axes), where its glow is.
    sun: Vec3,
    /// Its horizon's colour, on its brightness scale.
    horizon: Vec3,
    /// Its light, decoded: irradiance texels, then radiance texels.
    light: Vec<Vec3>,
}

/// The loaded skies, and what shows them.
pub struct Skies {
    skies: HashMap<String, LoadedSky>,
    cycle: SkyCycle,
    from_content: bool,
    /// The sky's material.
    material: Handle<SkyMaterial>,
    /// The light the sky casts (blended): its irradiance and radiance cubes.
    light: EnvironmentMapLight,
    /// Width of the irradiance cube's faces (the skies' light is the
    /// irradiance cube's texels, then the radiance cube's).
    irradiance_size: u32,
    /// The blend the light was last made for: the two skies, how far from
    /// the first to the second, and their brightnesses.
    light_made_for: Option<(String, String, f32, f32, f32)>,
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

/// Once the content is here: decodes each sky's light, and asks for its picture.
fn load_skies(content: Res<ContentStore>, asset_server: Res<AssetServer>, mut state: ResMut<SkyState>) {
    if !matches!(*state, SkyState::Waiting) || content.version.is_none() {
        return;
    }
    let (cycle, from_content) = match &content.sky_cycle {
        Some(cycle) => (cycle.clone(), true),
        None => {
            let keyframe =
                SkyKeyframe { hour: 0.0, sky: DEFAULT_SKY.to_string(), brightness: 1.0, turn: SkyTurn::FollowSun };
            (SkyCycle { keyframes: vec![keyframe] }, false)
        }
    };
    let mut skies = HashMap::new();
    for keyframe in &cycle.keyframes {
        if skies.contains_key(&keyframe.sky) {
            continue;
        }
        let Some((def, light)) =
            content.skies.get(&keyframe.sky).and_then(|files| Some((files.def.as_ref()?, files.light.as_ref()?)))
        else {
            warn!("There's no sky called \"{}\", so the sky stays plain.", keyframe.sky);
            *state = SkyState::None;
            return;
        };
        let picture = asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = true)
            .load::<Image>(format!("{CONTENT_SOURCE}://skies/{}/sky.jpg", keyframe.sky));
        let decoded = light.irradiance.iter().chain(&light.radiance).map(|&texel| Vec3::from(from_rgb9e5(texel)));
        let loaded =
            LoadedSky { picture, sun: Vec3::from(def.sun), horizon: Vec3::from(def.horizon), light: decoded.collect() };
        skies.insert(keyframe.sky.clone(), loaded);
    }
    *state = SkyState::Loading { skies, cycle, from_content };
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

/// Once every sky's picture has loaded: makes them cubemaps, and puts up
/// the sky.
fn finish_skies(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    content: Res<ContentStore>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut state: ResMut<SkyState>,
) {
    let SkyState::Loading { skies, .. } = &*state else { return };
    if skies.values().any(|sky| asset_server.load_state(&sky.picture).is_failed()) {
        warn!("A sky's picture couldn't be loaded, so the sky stays plain.");
        *state = SkyState::None;
        return;
    }
    if !skies.values().all(|sky| asset_server.is_loaded_with_dependencies(&sky.picture)) {
        return;
    }
    for sky in skies.values() {
        let Some(mut image) = images.get_mut(&sky.picture) else { return };
        if image.texture_descriptor.size.depth_or_array_layers == 1
            && let Err(error) = image.reinterpret_stacked_2d_as_array(6)
        {
            warn!("A sky's picture isn't six faces stacked ({error:?}), so the sky stays plain.");
            *state = SkyState::None;
            return;
        }
        image.texture_view_descriptor =
            Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    }
    let SkyState::Loading { skies, cycle, from_content } = std::mem::take(&mut *state) else { return };

    // The light's cubes, filled in by `blend_light` (they all share a layout).
    let first = &cycle.keyframes[0].sky;
    let Some(layout) = content.skies.get(first).and_then(|files| files.light.as_ref()) else { return };
    let (irradiance_size, radiance_size, radiance_mips) =
        (layout.irradiance_size, layout.radiance_size, layout.radiance_mips);
    let SkyLight { irradiance, radiance, .. } = layout;
    let light = EnvironmentMapLight {
        diffuse_map: images.add(cubemap(irradiance_size, 1, irradiance)),
        specular_map: images.add(cubemap(radiance_size, radiance_mips, radiance)),
        intensity: SKY_BRIGHTNESS,
        rotation: Quat::IDENTITY,
        affects_lightmapped_mesh_diffuse: true,
    };
    let picture = skies[first].picture.clone();
    let material =
        materials.add(SkyMaterial { settings: SkySettings::default(), first: picture.clone(), second: picture });
    commands.spawn(sky_sphere(&mut meshes, material.clone()));
    info!("The sky is ready ({} skies).", skies.len());
    *state =
        SkyState::Ready(Skies { skies, cycle, from_content, material, light, irradiance_size, light_made_for: None });
}

/// Gives the world's cameras (the ones with haze) an exposure to set, and
/// the sky's light once it's ready, including cameras that appear later.
fn dress_cameras(
    mut commands: Commands,
    state: Res<SkyState>,
    unexposed: Query<Entity, (With<Camera3d>, With<DistanceFog>, Without<Exposure>)>,
    unlit: Query<Entity, (With<Camera3d>, With<DistanceFog>, Without<EnvironmentMapLight>)>,
) {
    for camera in &unexposed {
        commands.entity(camera).insert(Exposure::default());
    }
    let SkyState::Ready(skies) = &*state else { return };
    for camera in &unlit {
        commands.entity(camera).insert(skies.light.clone());
    }
}

/// How a keyframe's sky is turned at a time of day, given where the sun is:
/// from the sky's own directions into the world's.
fn turn_of(keyframe: &SkyKeyframe, sky: &LoadedSky, sun: SkyPosition, hour: f32) -> Quat {
    match keyframe.turn {
        SkyTurn::FollowSun => {
            // So its sun's glow is on the sun's bearing.
            let (east, south, _) = sun.direction();
            Quat::from_rotation_y(east.atan2(south) - sky.sun.x.atan2(sky.sun.z))
        }
        SkyTurn::Stars => {
            let pole = to_bevy(0.0, -POLE_HEIGHT.to_radians().cos(), POLE_HEIGHT.to_radians().sin());
            Quat::from_axis_angle(pole, -hour / 24.0 * std::f32::consts::TAU)
        }
        SkyTurn::Fixed(degrees) => Quat::from_rotation_y(-degrees.to_radians()),
    }
}

/// Lights the world for the time of day: the sun (or the moon), the sky, the
/// exposure, and the haze.
fn light_by_time(
    clock: Res<TimeOfDay>,
    rules: Res<GameRules>,
    state: Res<SkyState>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    sun: Single<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut cameras: Query<(&mut DistanceFog, Option<&mut EnvironmentMapLight>, &mut Exposure), With<Camera3d>>,
) {
    let day = rules.0.day;
    let hour = clock.0;
    let (sun_at, moon_at) = (day.sun(hour), day.moon(hour));
    let height = sun_at.height;
    let towards = |position: SkyPosition| {
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

    let ev100 = exposure(height);
    let scale = Exposure { ev100 }.exposure();
    // Haze is the horizon's colour, warmed by a low sun.
    let warmth = 0.5 * (1.0 - smoothstep(0.0, 20f32.to_radians(), height)) * (1.0 - night(height));
    let tint = Vec3::ONE.lerp(sunlight, warmth);

    let mut rotation = Quat::IDENTITY;
    let horizon = match &*state {
        SkyState::Ready(skies) => {
            let (from, to, along) = skies.cycle.at(hour);
            // (With one sky, dimmed by the sun's height; a cycle says how bright each is.)
            let dimming = if skies.from_content { 1.0 } else { sky_level(height) };
            let (first, second) = (&skies.skies[&from.sky], &skies.skies[&to.sky]);
            let (first_turn, second_turn) = (turn_of(from, first, sun_at, hour), turn_of(to, second, sun_at, hour));
            let strengths = Vec2::new((1.0 - along) * from.brightness, along * to.brightness) * dimming;
            rotation = if along < 0.5 { first_turn } else { second_turn };
            if let Some(mut material) = materials.get_mut(&skies.material) {
                let sun_disc = sunlight * sun_strength(height) * SUN_DISC.1;
                let moon_disc = MOON_COLOUR.lerp(Vec3::ONE, 0.6) * MOON_DISC.1 * night(height);
                material.settings = SkySettings {
                    first_turn: Mat4::from_quat(first_turn.inverse()),
                    second_turn: Mat4::from_quat(second_turn.inverse()),
                    strengths: (strengths * SKY_BRIGHTNESS).extend(0.0).extend(0.0),
                    sun: towards(sun_at).extend(SUN_DISC.0.to_radians().cos()),
                    sun_colour: sun_disc.extend(1.0),
                    moon: towards(moon_at).extend(MOON_DISC.0.to_radians().cos()),
                    moon_colour: moon_disc.extend(0.0),
                };
                material.first = first.picture.clone();
                material.second = second.picture.clone();
            }
            first.horizon * strengths.x + second.horizon * strengths.y
        }
        _ => Vec3::from(palette::HAZE.to_linear().to_f32_array_no_alpha()) / (SKY_BRIGHTNESS * scale),
    };
    let haze = horizon * tint * SKY_BRIGHTNESS * scale;
    let haze = Color::linear_rgb(haze.x, haze.y, haze.z);
    if matches!(*state, SkyState::Ready(_)) {
        ambient.brightness = 0.0;
    } else {
        ambient.brightness = PLAIN_AMBIENT * sky_level(height);
        clear.0 = haze;
    }

    for (mut fog, environment, mut camera_exposure) in &mut cameras {
        fog.color = haze;
        fog.directional_light_color = Color::srgba(1.0, 0.9, 0.7, 0.4 * (1.0 - night(height)));
        camera_exposure.ev100 = ev100;
        if let Some(mut environment) = environment {
            environment.rotation = rotation;
        }
    }
}

/// Blends the light the two skies cast into the light cubes (on the CPU:
/// WebGL2 can't do it on the GPU), whenever the blend has moved on enough
/// since last time.
fn blend_light(
    clock: Res<TimeOfDay>,
    rules: Res<GameRules>,
    mut state: ResMut<SkyState>,
    mut images: ResMut<Assets<Image>>,
) {
    let SkyState::Ready(skies) = &mut *state else { return };
    let height = rules.0.day.sun(clock.0).height;
    let (from, to, along) = skies.cycle.at(clock.0);
    let dimming = if skies.from_content { 1.0 } else { sky_level(height) };
    let wanted = (from.sky.clone(), to.sky.clone(), along, from.brightness * dimming, to.brightness * dimming);
    let close_enough = skies.light_made_for.as_ref().is_some_and(|made| {
        made.0 == wanted.0
            && made.1 == wanted.1
            && (made.2 - wanted.2).abs() < LIGHT_STEP
            && (made.3 - wanted.3).abs() < LIGHT_STEP * made.3.max(0.01)
            && (made.4 - wanted.4).abs() < LIGHT_STEP * made.4.max(0.01)
    });
    if close_enough {
        return;
    }
    let (a, b) = (&skies.skies[&from.sky].light, &skies.skies[&to.sky].light);
    let (weight_a, weight_b) = ((1.0 - along) * wanted.3, along * wanted.4);
    let texels: Vec<u32> = a.iter().zip(b).map(|(&a, &b)| rgb9e5((a * weight_a + b * weight_b).to_array())).collect();
    let irradiance_texels = (skies.irradiance_size * skies.irradiance_size * 6) as usize;
    let bytes = |texels: &[u32]| texels.iter().flat_map(|texel| texel.to_le_bytes()).collect::<Vec<u8>>();
    if let Some(mut image) = images.get_mut(&skies.light.diffuse_map) {
        image.data = Some(bytes(&texels[..irradiance_texels]));
    }
    if let Some(mut image) = images.get_mut(&skies.light.specular_map) {
        image.data = Some(bytes(&texels[irradiance_texels..]));
    }
    skies.light_made_for = Some(wanted);
}
