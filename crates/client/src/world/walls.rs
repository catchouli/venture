//! Walls and fences (see `docs/plan-walls.md`): built by the structures
//! generator (`venture_structures`) from the baked walls in the world's
//! manifest, which carry their paths and the ground's height along them, so
//! they don't wait for the terrain.
//!
//! - **Building:** when the world arrives (or changes), every wall is built
//!   in the background, in stretches of about 16 m.
//! - **Levels of detail:** a dry stone wall is every stone close up and one
//!   plain band far away, with Bevy's [`VisibilityRange`] crossfading
//!   between them; fences are light enough to be the same at any distance.
//! - **Materials:** the stones and wood wear PBR textures from the content
//!   (`textures/drystone/` and `fence_wood/`: colour, normal map, and
//!   ambient occlusion and roughness), multiplied by each piece's vertex
//!   colour. Bevy doesn't make mipmaps for JPEGs, so they're added once the
//!   pictures load.

use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use venture_structures::{MeshPart, Stretch, WallKind, build_wall};

use crate::common::mipmaps::add_mipmaps;
use crate::content::{CONTENT_SOURCE, ContentStore};
use crate::world::source::WorldInfo;

/// Each kind's texture (a folder in the content's `textures/`), and how
/// many metres it covers before it repeats.
const TEXTURES: [(WallKind, &str, f32); 2] =
    [(WallKind::DryStone, "drystone", 1.6), (WallKind::Fence, "fence_wood", 1.2)];
/// The pictures of each texture, and whether each is sRGB.
const PICTURES: [(&str, bool); 3] = [("colour.jpg", true), ("normal.jpg", false), ("surface.jpg", false)];
/// Walls are every stone up to this far from the camera, in metres...
const FULL_UNTIL: f32 = 60.0;
/// ...crossfading to (and from) their far look over this distance...
const CROSSFADE: f32 = 10.0;
/// ...and aren't drawn at all beyond this.
const DRAWN_UNTIL: f32 = 450.0;

/// Builds and shows walls.
pub struct WallsPlugin;

impl Plugin for WallsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Walls>()
            .add_systems(Update, (load_materials, finish_textures, start_building, finish_building).chain());
    }
}

/// The walls: their materials, and what they were built from.
#[derive(Resource, Default)]
pub struct Walls {
    /// Each kind's material (in the order of [`TEXTURES`]), once asked for.
    materials: Option<[Handle<StandardMaterial>; 2]>,
    /// Each kind's pictures, until they've been given mipmaps.
    pictures: Vec<Handle<Image>>,
    /// Building, if under way.
    building: Option<Task<Vec<(WallKind, Stretch)>>>,
}

/// A stretch of wall (the parent of its meshes).
#[derive(Component)]
pub struct WallStretch;

/// Asks for the walls' textures, once the content (which has them) is here,
/// and makes their materials.
fn load_materials(
    content: Res<ContentStore>,
    asset_server: Res<AssetServer>,
    mut walls: ResMut<Walls>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if walls.materials.is_some() || content.version.is_none() {
        return;
    }
    let mut pictures = Vec::new();
    let made = TEXTURES.map(|(_, texture, metres)| {
        let [colour, normal, surface] = PICTURES.map(|(picture, srgb)| {
            let handle = asset_server
                .load_builder()
                .with_settings(move |settings: &mut ImageLoaderSettings| settings.is_srgb = srgb)
                .load::<Image>(format!("{CONTENT_SOURCE}://textures/{texture}/{picture}"));
            pictures.push(handle.clone());
            handle
        });
        materials.add(StandardMaterial {
            base_color_texture: Some(colour),
            normal_map_texture: Some(normal),
            // Ambient occlusion in red, roughness in green (and no metal: blue is 0).
            occlusion_texture: Some(surface.clone()),
            metallic_roughness_texture: Some(surface),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.3,
            uv_transform: Affine2::from_scale(Vec2::splat(1.0 / metres)),
            ..default()
        })
    });
    walls.materials = Some(made);
    walls.pictures = pictures;
}

/// Gives the textures mipmaps and a sampler that repeats, as each loads.
fn finish_textures(asset_server: Res<AssetServer>, mut walls: ResMut<Walls>, mut images: ResMut<Assets<Image>>) {
    walls.pictures.retain(|handle| {
        if matches!(asset_server.load_state(handle), LoadState::Failed(_)) {
            warn!("A wall texture couldn't be loaded, so walls go without it.");
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

/// Builds every wall in the background when the world arrives or changes
/// (and takes down the old ones).
fn start_building(
    mut commands: Commands,
    world: Option<Res<WorldInfo>>,
    mut walls: ResMut<Walls>,
    old: Query<Entity, With<WallStretch>>,
) {
    let Some(world) = world else { return };
    if !world.is_changed() {
        return;
    }
    for stretch in &old {
        commands.entity(stretch).despawn();
    }
    let baked = world.0.walls.clone();
    if baked.is_empty() {
        walls.building = None;
        return;
    }
    walls.building = Some(AsyncComputeTaskPool::get().spawn(async move {
        let started = bevy::platform::time::Instant::now();
        let stretches: Vec<(WallKind, Stretch)> =
            baked.iter().flat_map(|wall| build_wall(wall).into_iter().map(|stretch| (wall.kind, stretch))).collect();
        let triangles: usize = stretches.iter().map(|(_, stretch)| stretch.full.triangles()).sum();
        let took = started.elapsed().as_secs_f64() * 1000.0;
        info!("{} walls built: {} stretches, {triangles} triangles, in {took:.0} ms.", baked.len(), stretches.len());
        stretches
    }));
}

/// Shows the walls once they're built (and their materials exist).
fn finish_building(mut commands: Commands, mut walls: ResMut<Walls>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(materials) = walls.materials.clone() else { return };
    let Some(task) = walls.building.as_mut() else { return };
    let Some(stretches) = check_ready(task) else { return };
    walls.building = None;
    let material = |kind: WallKind| {
        let index = TEXTURES.iter().position(|(of, ..)| *of == kind).unwrap_or(0);
        materials[index].clone()
    };
    let near =
        VisibilityRange { start_margin: 0.0..0.0, end_margin: FULL_UNTIL..FULL_UNTIL + CROSSFADE, use_aabb: false };
    let far = VisibilityRange {
        start_margin: FULL_UNTIL..FULL_UNTIL + CROSSFADE,
        end_margin: DRAWN_UNTIL..DRAWN_UNTIL + CROSSFADE,
        use_aabb: false,
    };
    let everywhere =
        VisibilityRange { start_margin: 0.0..0.0, end_margin: DRAWN_UNTIL..DRAWN_UNTIL + CROSSFADE, use_aabb: false };
    for (kind, stretch) in stretches {
        let material = material(kind);
        let [x, height, y] = stretch.origin;
        commands.spawn((WallStretch, Transform::from_xyz(x, height, y), Visibility::default())).with_children(
            |parent| {
                // Fences have no far look: they're the same at any distance.
                let full_range = if stretch.far.is_empty() { everywhere.clone() } else { near.clone() };
                for (part, range) in [(&stretch.full, full_range), (&stretch.far, far.clone())] {
                    if part.is_empty() {
                        continue;
                    }
                    parent.spawn((Mesh3d(meshes.add(to_mesh(part))), MeshMaterial3d(material.clone()), range));
                }
            },
        );
    }
}

/// A Bevy mesh from a generated one, with tangents for the normal maps.
fn to_mesh(part: &MeshPart) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, part.uvs.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, part.colours.clone())
        .with_inserted_indices(Indices::U32(part.indices.clone()));
    if let Err(error) = mesh.generate_tangents() {
        warn!("A wall's tangents couldn't be worked out ({error}), so its normal map is off.");
    }
    mesh
}
