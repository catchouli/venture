//! Textures for the world's structures (walls, fences, hedges, buildings):
//! each a folder of the content's `textures/` (colour, normal map, and
//! ambient occlusion and roughness: see `docs/textures.md`), loaded once
//! however many materials use it. Bevy doesn't make mipmaps for JPEGs, so
//! they're added (with a sampler that repeats) as each picture loads.

use std::collections::HashMap;

use bevy::asset::LoadState;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;

use crate::common::mipmaps::add_mipmaps;
use crate::content::CONTENT_SOURCE;

/// The pictures of each texture, and whether each is sRGB.
const PICTURES: [(&str, bool); 3] = [("colour.jpg", true), ("normal.jpg", false), ("surface.jpg", false)];

/// Loads structures' textures and gives them mipmaps.
pub struct SurfacesPlugin;

impl Plugin for SurfacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceTextures>().add_systems(Update, finish_textures);
    }
}

/// The structures' textures: each asked for once.
#[derive(Resource, Default)]
pub struct SurfaceTextures {
    loaded: HashMap<String, [Handle<Image>; 3]>,
    /// Pictures still to be given mipmaps.
    pending: Vec<Handle<Image>>,
}

impl SurfaceTextures {
    /// A texture's pictures (colour, normal map, surface), by its folder in
    /// the content's `textures/`: asked for the first time it's wanted.
    pub fn get(&mut self, name: &str, asset_server: &AssetServer) -> [Handle<Image>; 3] {
        if let Some(pictures) = self.loaded.get(name) {
            return pictures.clone();
        }
        let pictures = PICTURES.map(|(picture, srgb)| {
            asset_server
                .load_builder()
                .with_settings(move |settings: &mut ImageLoaderSettings| settings.is_srgb = srgb)
                .load::<Image>(format!("{CONTENT_SOURCE}://textures/{name}/{picture}"))
        });
        self.pending.extend(pictures.iter().cloned());
        self.loaded.insert(name.to_string(), pictures.clone());
        pictures
    }
}

/// The usual rough material for a texture, repeating every `metres`:
/// multiplied by its meshes' vertex colours, with its normal map, and its
/// ambient occlusion (red) and roughness (green; blue is 0, no metal).
pub fn textured([colour, normal, surface]: [Handle<Image>; 3], metres: f32) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(colour),
        normal_map_texture: Some(normal),
        occlusion_texture: Some(surface.clone()),
        metallic_roughness_texture: Some(surface),
        perceptual_roughness: 1.0,
        metallic: 0.0,
        reflectance: 0.3,
        uv_transform: Affine2::from_scale(Vec2::splat(1.0 / metres)),
        ..default()
    }
}

/// Gives the pictures mipmaps and a sampler that repeats, as each loads.
fn finish_textures(
    asset_server: Res<AssetServer>,
    mut textures: ResMut<SurfaceTextures>,
    mut images: ResMut<Assets<Image>>,
) {
    if textures.pending.is_empty() {
        return;
    }
    textures.pending.retain(|handle| {
        if matches!(asset_server.load_state(handle), LoadState::Failed(_)) {
            warn!("A texture for the world's structures couldn't be loaded, so they go without it.");
            return false;
        }
        if !asset_server.is_loaded_with_dependencies(handle) {
            return true;
        }
        if let Some(mut image) = images.get_mut(handle) {
            add_mipmaps(&mut image);
            image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                mag_filter: ImageFilterMode::Linear,
                min_filter: ImageFilterMode::Linear,
                mipmap_filter: ImageFilterMode::Linear,
                anisotropy_clamp: 16,
                ..default()
            });
        }
        false
    });
}
