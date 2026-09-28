//! Walls and fences (see `docs/plan-walls.md`): built by the structures
//! generator (`venture_structures`) from the baked walls in the world's
//! manifest, which carry their paths and the ground's height along them, so
//! they don't wait for the terrain.
//!
//! - **Building:** the walls shown ([`ShownWalls`]) are the world's own,
//!   from its manifest, unless something else sets them (the editor shows
//!   the walls it's editing). Whenever they change, the walls that changed
//!   are built again in the background, in stretches of about 16 m.
//! - **Levels of detail:** a dry stone wall is every stone close up and one
//!   plain band far away, with Bevy's [`VisibilityRange`] crossfading
//!   between them; fences are light enough to be the same at any distance.
//! - **Materials:** the stones and wood wear PBR textures from the content
//!   (`textures/drystone/` and `fence_wood/`: colour, normal map, and
//!   ambient occlusion and roughness), multiplied by each piece's vertex
//!   colour. Bevy doesn't make mipmaps for JPEGs, so they're added once the
//!   pictures load. A hedge's body wears `textures/hedge/`; its leaves are
//!   [`HEDGE_SPECIES`]' own leaves (the trees' material, fading near the
//!   camera as trees do), added once the trees have grown.

use std::collections::HashMap;

use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use venture_structures::{BakedWall, MeshPart, Stretch, WallKind, build_wall};

use crate::common::mipmaps::add_mipmaps;
use crate::content::{CONTENT_SOURCE, ContentStore};
use crate::world::source::WorldInfo;
use crate::world::trees::TreeTemplates;

/// Each kind's texture (a folder in the content's `textures/`), and how
/// many metres it covers before it repeats.
const TEXTURES: [(WallKind, &str, f32); 3] =
    [(WallKind::DryStone, "drystone", 1.6), (WallKind::Fence, "fence_wood", 1.2), (WallKind::Hedge, "hedge", 2.2)];
/// The pictures of each texture, and whether each is sRGB.
const PICTURES: [(&str, bool); 3] = [("colour.jpg", true), ("normal.jpg", false), ("surface.jpg", false)];
/// The tree species whose leaves hedges wear.
pub const HEDGE_SPECIES: &str = "hawthorn";
/// Hedges' leaves are drawn up to this far from the camera, in metres.
const HEDGE_LEAVES_UNTIL: f32 = 110.0;
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
        app.init_resource::<Walls>().init_resource::<ShownWalls>().add_systems(
            Update,
            (load_materials, finish_textures, take_manifest_walls, start_building, finish_building, dress_hedges)
                .chain(),
        );
    }
}

/// The walls to show.
#[derive(Resource, Default)]
pub struct ShownWalls {
    /// The walls.
    pub walls: Vec<BakedWall>,
    /// Whether something else sets them (the editor), rather than their
    /// being the world's own, from its manifest.
    pub authored: bool,
}

/// A wall's stretches, built.
type Built = Vec<(WallKind, Stretch)>;

/// The walls: their materials, and what's built.
#[derive(Resource, Default)]
pub struct Walls {
    /// Each kind's material, once asked for.
    materials: Option<HashMap<WallKind, Handle<StandardMaterial>>>,
    /// Hedges' leaves waiting for the trees to grow (for their material):
    /// the stretch they're on, and their mesh.
    bare_hedges: Vec<(Entity, Handle<Mesh>)>,
    /// Each kind's pictures, until they've been given mipmaps.
    pictures: Vec<Handle<Image>>,
    /// Each wall shown, by its key: what it was built from, and its stretches.
    built: HashMap<String, (BakedWall, Vec<Entity>)>,
    /// Building, if under way: every wall's key (to know which have gone),
    /// and the ones built again.
    building: Option<Task<(Vec<String>, Vec<(String, BakedWall, Built)>)>>,
    /// Whether the walls shown changed while others were being built.
    dirty: bool,
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
    let mut made: HashMap<WallKind, Handle<StandardMaterial>> = HashMap::new();
    for (kind, texture, metres) in TEXTURES {
        let [colour, normal, surface] = PICTURES.map(|(picture, srgb)| {
            let handle = asset_server
                .load_builder()
                .with_settings(move |settings: &mut ImageLoaderSettings| settings.is_srgb = srgb)
                .load::<Image>(format!("{CONTENT_SOURCE}://textures/{texture}/{picture}"));
            pictures.push(handle.clone());
            handle
        });
        let material = materials.add(match kind {
            // Leaves: rough all over, with little sheen, and some light through
            // them. (Its roughness picture reads too smooth for leaves, and its
            // normal map adds more fine noise than shape.)
            WallKind::Hedge => StandardMaterial {
                base_color_texture: Some(colour),
                occlusion_texture: Some(surface),
                perceptual_roughness: 0.95,
                metallic: 0.0,
                reflectance: 0.08,
                diffuse_transmission: 0.15,
                uv_transform: Affine2::from_scale(Vec2::splat(1.0 / metres)),
                ..default()
            },
            _ => StandardMaterial {
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
            },
        });
        made.insert(kind, material);
    }
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

/// Shows the world's own walls, unless something else sets them.
fn take_manifest_walls(world: Option<Res<WorldInfo>>, mut shown: ResMut<ShownWalls>) {
    if let Some(world) = world
        && world.is_changed()
        && !shown.authored
    {
        shown.walls = world.0.walls.clone();
    }
}

/// Each wall's key: its id (made unique, should two share one).
fn keys(walls: &[BakedWall]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::with_capacity(walls.len());
    for wall in walls {
        let key = (1..)
            .map(|n| if n == 1 { wall.id.clone() } else { format!("{}#{n}", wall.id) })
            .find(|key| !keys.contains(key))
            .unwrap_or_default();
        keys.push(key);
    }
    keys
}

/// Builds the walls that changed in the background (one build at a time).
fn start_building(shown: Res<ShownWalls>, mut walls: ResMut<Walls>) {
    if shown.is_changed() {
        walls.dirty = true;
    }
    if !walls.dirty || walls.building.is_some() {
        return;
    }
    walls.dirty = false;
    let keys = keys(&shown.walls);
    let changed: Vec<(String, BakedWall)> = keys
        .iter()
        .zip(&shown.walls)
        .filter(|(key, wall)| walls.built.get(*key).is_none_or(|(built, _)| built != *wall))
        .map(|(key, wall)| (key.clone(), wall.clone()))
        .collect();
    if changed.is_empty() && walls.built.keys().all(|key| keys.contains(key)) {
        return;
    }
    walls.building = Some(AsyncComputeTaskPool::get().spawn(async move {
        let started = bevy::platform::time::Instant::now();
        let built: Vec<(String, BakedWall, Built)> = changed
            .into_iter()
            .map(|(key, wall)| {
                let stretches = build_wall(&wall).into_iter().map(|stretch| (wall.kind, stretch)).collect();
                (key, wall, stretches)
            })
            .collect();
        let triangles: usize =
            built.iter().flat_map(|(_, _, stretches)| stretches).map(|(_, stretch)| stretch.full.triangles()).sum();
        let took = started.elapsed().as_secs_f64() * 1000.0;
        debug!("{} walls built: {triangles} triangles, in {took:.0} ms.", built.len());
        (keys, built)
    }));
}

/// Shows the walls once they're built (and their materials exist), and
/// takes down the ones they replace or that have gone.
fn finish_building(mut commands: Commands, mut walls: ResMut<Walls>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(materials) = walls.materials.clone() else { return };
    let Some(task) = walls.building.as_mut() else { return };
    let Some((keys, built)) = check_ready(task) else { return };
    walls.building = None;
    let gone: Vec<String> = walls
        .built
        .keys()
        .filter(|key| !keys.contains(key) || built.iter().any(|(rebuilt, ..)| rebuilt == *key))
        .cloned()
        .collect();
    for key in gone {
        if let Some((_, entities)) = walls.built.remove(&key) {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
    }
    let material =
        |kind: WallKind| materials.get(&kind).or_else(|| materials.values().next()).cloned().unwrap_or_default();
    let range = |from: f32, to: f32| VisibilityRange {
        start_margin: from..if from > 0.0 { from + CROSSFADE } else { 0.0 },
        end_margin: to..to + CROSSFADE,
        use_aabb: false,
    };
    let (near, far, everywhere) = (range(0.0, FULL_UNTIL), range(FULL_UNTIL, DRAWN_UNTIL), range(0.0, DRAWN_UNTIL));
    for (key, wall, stretches) in built {
        let mut entities = Vec::with_capacity(stretches.len());
        for (kind, stretch) in stretches {
            let material = material(kind);
            let [x, height, y] = stretch.origin;
            let parent = commands
                .spawn((WallStretch, Transform::from_xyz(x, height, y), Visibility::default()))
                .with_children(|parent| {
                    // Fences have no far look: they're the same at any distance.
                    let full_range = if stretch.far.is_empty() { everywhere.clone() } else { near.clone() };
                    for (part, range) in [(&stretch.full, full_range), (&stretch.far, far.clone())] {
                        if part.is_empty() {
                            continue;
                        }
                        parent.spawn((Mesh3d(meshes.add(to_mesh(part))), MeshMaterial3d(material.clone()), range));
                    }
                })
                .id();
            if !stretch.leaves.is_empty() {
                walls.bare_hedges.push((parent, meshes.add(to_mesh(&stretch.leaves))));
            }
            entities.push(parent);
        }
        walls.built.insert(key, (wall, entities));
    }
}

/// Gives hedges their leaves, once the trees have grown (their material is
/// the hedge species' leaves).
fn dress_hedges(
    mut commands: Commands,
    mut walls: ResMut<Walls>,
    templates: Res<TreeTemplates>,
    stretches: Query<(), With<WallStretch>>,
) {
    if walls.bare_hedges.is_empty() {
        return;
    }
    let Some(leaves) = templates
        .species
        .get(HEDGE_SPECIES)
        .or_else(|| templates.species.values().next())
        .and_then(|variants| variants.first())
        .map(|template| template.leaf_material().clone())
    else {
        return;
    };
    let range = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: HEDGE_LEAVES_UNTIL..HEDGE_LEAVES_UNTIL + CROSSFADE,
        use_aabb: false,
    };
    for (stretch, mesh) in walls.bare_hedges.drain(..) {
        if stretches.contains(stretch) {
            commands.entity(stretch).with_child((Mesh3d(mesh), MeshMaterial3d(leaves.clone()), range.clone()));
        }
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
