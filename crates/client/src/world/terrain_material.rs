//! The terrain's material: Bevy's standard PBR material, extended with a
//! shader (`shaders/terrain.wgsl`) that textures the ground.
//!
//! Grass, dirt and rock wear PBR textures from the content pack
//! (`textures/grass/`, `dirt/` and `rock/`: colour, normal map, roughness
//! and ambient occlusion; see `docs/textures.md`), and steep slopes are
//! rock. Other ground (sand, snow, ash), and everything until the textures
//! have loaded, gets procedural detail instead.
//!
//! The pictures are loaded, given mipmaps (Bevy doesn't make them for JPEGs),
//! and packed into three texture arrays (colours, normal maps, surfaces), one
//! layer per texture: three bindings however many textures there are, which
//! keeps the shader within WebGL2's limit on textures.

use bevy::asset::{LoadState, RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::shader::ShaderRef;

use crate::common::mipmaps::{add_mipmaps, average_colour};
use crate::content::{CONTENT_SOURCE, ContentStore};

/// The complete terrain material.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

/// Slope (1 minus the normal's upward part) where rock starts to show.
const ROCK_START_SLOPE: f32 = 0.28;
/// Slope where the ground is entirely rock.
const ROCK_FULL_SLOPE: f32 = 0.45;
/// How strongly the procedural surface bumps tilt the lighting.
const BUMP_STRENGTH: f32 = 0.12;
/// The ground textures (folders in the content's `textures/`), in the order
/// of [`crate::world::terrain_mesh::GroundTexture`], and how many metres
/// each covers before it repeats.
const GROUND_TEXTURES: [(&str, f32); 3] = [("grass", 2.5), ("dirt", 3.0), ("rock", 4.0)];
/// The pictures of each texture, and whether each is sRGB.
const PICTURES: [(&str, bool); 3] = [("colour.jpg", true), ("normal.jpg", false), ("surface.jpg", false)];

/// Registers the terrain material and embeds its shader in the executable.
pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .init_resource::<GroundTextures>()
            .add_systems(Update, (load_ground_textures, finish_ground_textures).chain());
    }
}

/// The terrain shader's settings and textures.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExtension {
    #[uniform(100)]
    settings: TerrainSettings,
    /// The textures' colours (sRGB), one layer each.
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    colours: Option<Handle<Image>>,
    /// Their normal maps (OpenGL style).
    #[texture(103, dimension = "2d_array")]
    #[sampler(104)]
    normals: Option<Handle<Image>>,
    /// Their surfaces: ambient occlusion (red) and roughness (green).
    #[texture(105, dimension = "2d_array")]
    #[sampler(106)]
    surfaces: Option<Handle<Image>>,
}

/// The shader's settings (every field a `vec4`, so the layout suits WebGL2).
#[derive(ShaderType, Reflect, Debug, Clone)]
struct TerrainSettings {
    /// `(rock start slope, rock full slope, bump strength, 1 once the textures have loaded)`.
    params: Vec4,
    /// Each texture's average colour (linear), and how many metres it covers.
    textures: [Vec4; 3],
}

impl TerrainExtension {
    /// The terrain's settings, with stand-in textures until the real ones load.
    ///
    /// (The stand-ins are arrays of as many layers as the real ones: WebGL2
    /// can't be told a texture is an array, and guesses from its layers, so
    /// Bevy's one-layer fallback texture would be taken for a plain one.)
    pub fn new(images: &mut Assets<Image>) -> Self {
        let texture = |(_, metres): (&str, f32)| Vec4::new(0.5, 0.5, 0.5, metres);
        let mut stand_in = |format| {
            let size = Extent3d { width: 1, height: 1, depth_or_array_layers: GROUND_TEXTURES.len() as u32 };
            let data = vec![255; 4 * GROUND_TEXTURES.len()];
            let mut image = Image::new(size, TextureDimension::D2, data, format, RenderAssetUsages::RENDER_WORLD);
            image.texture_view_descriptor =
                Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..default() });
            Some(images.add(image))
        };
        Self {
            settings: TerrainSettings {
                params: Vec4::new(ROCK_START_SLOPE, ROCK_FULL_SLOPE, BUMP_STRENGTH, 0.0),
                textures: GROUND_TEXTURES.map(texture),
            },
            colours: stand_in(TextureFormat::Rgba8UnormSrgb),
            normals: stand_in(TextureFormat::Rgba8Unorm),
            surfaces: stand_in(TextureFormat::Rgba8Unorm),
        }
    }
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://venture_client/world/shaders/terrain.wgsl".into()
    }
}

/// The ground textures' pictures, once asked for.
#[derive(Resource, Default)]
struct GroundTextures {
    /// For each picture (colour, normal map, surface), each texture's.
    loading: Option<[[Handle<Image>; 3]; 3]>,
    /// Whether they're in the terrain material (or couldn't be).
    done: bool,
}

/// Asks for the ground textures once the content (which has them) is here.
fn load_ground_textures(
    content: Res<ContentStore>,
    asset_server: Res<AssetServer>,
    mut textures: ResMut<GroundTextures>,
) {
    if textures.loading.is_some() || content.version.is_none() {
        return;
    }
    let load = |texture: &str, (picture, srgb): (&str, bool)| {
        asset_server
            .load_builder()
            .with_settings(move |settings: &mut ImageLoaderSettings| settings.is_srgb = srgb)
            .load::<Image>(format!("{CONTENT_SOURCE}://textures/{texture}/{picture}"))
    };
    textures.loading = Some(PICTURES.map(|picture| GROUND_TEXTURES.map(|(texture, _)| load(texture, picture))));
}

/// A sampler that repeats, blends between mipmaps, and stays sharp at low angles.
fn tiling_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    }
}

/// Once every picture has loaded: gives them mipmaps, packs them into
/// arrays, and puts those in the terrain material. (If any fails, the
/// terrain keeps its procedural detail.)
fn finish_ground_textures(
    asset_server: Res<AssetServer>,
    mut textures: ResMut<GroundTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Some(handles) = textures.loading.clone() else { return };
    if textures.done {
        return;
    }
    let all = || handles.iter().flatten();
    if all().any(|handle| matches!(asset_server.load_state(handle), LoadState::Failed(_))) {
        warn!("The ground textures couldn't all be loaded, so the ground keeps its plain look.");
        textures.done = true;
        return;
    }
    if !all().all(|handle| asset_server.is_loaded_with_dependencies(handle)) {
        return;
    }
    textures.done = true;
    let started = bevy::platform::time::Instant::now();
    let mut arrays = Vec::new();
    for layers in &handles {
        let mut pictures = Vec::new();
        for handle in layers {
            let Some(mut image) = images.remove(handle) else { return };
            add_mipmaps(&mut image);
            pictures.push(image);
        }
        let Some(array) = texture_array(&pictures) else {
            warn!(
                "Each ground texture's pictures must be the same size as the other textures' (colour, normal, surface)."
            );
            return;
        };
        arrays.push((images.add(array), pictures));
    }
    // Each texture's average colour: its colour picture's smallest mipmap.
    let averages: Vec<Vec4> = arrays[0]
        .1
        .iter()
        .zip(GROUND_TEXTURES)
        .map(|(picture, (_, metres))| {
            let [r, g, b, _] = average_colour(picture).unwrap_or([128, 128, 128, 255]);
            let linear = Color::srgb_u8(r, g, b).to_linear();
            Vec4::new(linear.red, linear.green, linear.blue, metres)
        })
        .collect();
    let took = started.elapsed().as_secs_f64() * 1000.0;
    info!("The ground textures are ready ({took:.0} ms to make mipmaps and arrays).");
    for (_, material) in materials.iter_mut() {
        let extension = &mut material.extension;
        extension.settings.params.w = 1.0;
        extension.settings.textures = [averages[0], averages[1], averages[2]];
        extension.colours = Some(arrays[0].0.clone());
        extension.normals = Some(arrays[1].0.clone());
        extension.surfaces = Some(arrays[2].0.clone());
    }
}

/// Pictures of one size and format (each with the same number of mipmaps),
/// as the layers of one array texture.
fn texture_array(pictures: &[Image]) -> Option<Image> {
    let first = pictures.first()?;
    let descriptor = &first.texture_descriptor;
    let same = |picture: &Image| {
        let other = &picture.texture_descriptor;
        other.size == descriptor.size
            && other.format == descriptor.format
            && other.mip_level_count == descriptor.mip_level_count
    };
    if !pictures.iter().all(same) {
        return None;
    }
    // Layer by layer, each with all its mipmaps (Bevy's usual order).
    let data: Vec<u8> = pictures.iter().flat_map(|picture| picture.data.clone().unwrap_or_default()).collect();
    let size = Extent3d { depth_or_array_layers: pictures.len() as u32, ..descriptor.size };
    // (`new_uninit`: `Image::new` expects only the first mipmap.)
    let mut array = Image::new_uninit(size, TextureDimension::D2, descriptor.format, RenderAssetUsages::RENDER_WORLD);
    array.data = Some(data);
    array.texture_descriptor.mip_level_count = descriptor.mip_level_count;
    array.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..default() });
    array.sampler = ImageSampler::Descriptor(tiling_sampler());
    Some(array)
}
