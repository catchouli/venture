//! Trees, grown by the tree generator (`venture_trees`) from the content's
//! species (see `docs/plan-trees.md`), and planted where the world's tree
//! props are.
//!
//! - **Templates:** when the content arrives (or changes), each species is
//!   grown in a few variants ([`VARIANTS`]), in the background, a task per
//!   species: textures (from its photographs), then each variant's full and
//!   reduced meshes and its impostor.
//! - **Levels of detail:** every tree is its full mesh close up, its reduced
//!   one further away, and its impostor (two crossed cards) in the
//!   distance. Bevy's [`VisibilityRange`] switches between them, crossfading
//!   with dither. Trees of one template share meshes and materials, so
//!   their draws batch.
//! - **Planting:** once a chunk's meshes are shown and the templates are
//!   ready, its tree props become trees, as children of the chunk: each
//!   names its species (in the world's list of them) and its variant.
//!
//! Leaves are alpha-tested and lit from both sides without their normals
//! flipping (`cull_mode: None` rather than `double_sided`), since their
//! normals point out from the crown whichever way a card faces.
//!
//! **Trees fade close to the camera,** smoothly, so they never block the
//! view (`shaders/tree.wgsl`; see [`TreeFade`]). How close, and where the
//! levels of detail change, come from the content (`trees/drawing.ron`).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use venture_content::trees::{Species, TreeDrawing};
use venture_trees::impostor::{Impostor, impostor};
use venture_trees::photos::{LeafPhotos, Photos};
use venture_trees::{Detail, MeshPart, TreeParams, TreeTextures, grow, textures};
use venture_world::{CHUNK_SIZE, Chunk, PropKind};

use crate::common::coords::to_bevy;
use crate::common::mipmaps::{add_mipmaps, add_mipmaps_keeping_coverage};
use crate::content::ContentStore;
use crate::world::source::WorldInfo;
use crate::world::streaming::{ChunkStore, MeshTask, ShownChunks};

/// Each species' variants: (age, seed). Young, mature and old (as
/// [`PropKind::Tree`]'s variant numbers them).
const VARIANTS: [(f32, u64); 3] = [(0.55, 11), (1.0, 12), (1.4, 13)];
/// Leaves are see-through below this coverage.
const ALPHA_CUTOFF: f32 = 0.5;
/// How far from its middle a trunk's flared base reaches, for sinking it on slopes, in metres.
const TRUNK_BASE_REACH: f32 = 1.2;
/// A tree's material: the standard one, fading where it's in the way.
pub type TreeMaterial = ExtendedMaterial<StandardMaterial, TreeFade>;

/// Where trees fade (see `shaders/tree.wgsl`).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TreeFade {
    #[uniform(100)]
    settings: FadeSettings,
}

/// The fade's settings (a `vec4`, so the layout suits WebGL2).
#[derive(ShaderType, Reflect, Debug, Clone, Default)]
struct FadeSettings {
    /// `(where the fade starts, where it ends, 1 if fading is on, unused)`, in metres from the camera.
    settings: Vec4,
}

impl MaterialExtension for TreeFade {
    fn fragment_shader() -> ShaderRef {
        "embedded://venture_client/world/shaders/tree.wgsl".into()
    }
}

/// Grows and plants trees.
pub struct TreesPlugin;

impl Plugin for TreesPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/tree.wgsl");
        app.add_plugins(MaterialPlugin::<TreeMaterial>::default())
            .init_resource::<TreeTemplates>()
            .add_systems(Update, (start_growing, finish_growing, take_drawing, plant_trees).chain());
    }
}

/// Every species' grown templates.
#[derive(Resource, Default)]
pub struct TreeTemplates {
    /// Which content they were grown from (its version), once grown.
    pub grown_from: Option<String>,
    /// Templates by species id.
    pub species: HashMap<String, Vec<Template>>,
    /// Growing, if under way: a task per species, and the content version.
    growing: Option<(String, Vec<Task<GrownSpecies>>)>,
    /// How trees are drawn (from the content).
    drawing: TreeDrawing,
    /// Counts up each time the templates (or how they're drawn) change, so chunks replant.
    generation: u32,
}

/// One variant of a species, ready to draw.
#[derive(Clone)]
pub struct Template {
    full: (Handle<Mesh>, Handle<Mesh>),
    reduced: (Handle<Mesh>, Handle<Mesh>),
    impostor: Handle<Mesh>,
    bark: Handle<TreeMaterial>,
    leaves: Handle<TreeMaterial>,
    impostor_material: Handle<TreeMaterial>,
}

/// A species grown in the background (plain data, made into assets after).
struct GrownSpecies {
    id: String,
    textures: TreeTextures,
    variants: Vec<GrownVariant>,
}

struct GrownVariant {
    full: venture_trees::TreeMeshes,
    reduced: venture_trees::TreeMeshes,
    impostor: Impostor,
}

/// A chunk's trees are planted, from this generation of templates.
#[derive(Component)]
struct Planted(u32);

/// A tree's part (so replanting can find them).
#[derive(Component)]
struct TreePart;

/// Grows a species' textures and variants.
fn grow_species(species: Species, photos: Photos) -> GrownSpecies {
    let textures = textures::paint(&species, &photos);
    let variants = VARIANTS
        .iter()
        .map(|&(age, seed)| {
            let params = TreeParams { age, seed, ..TreeParams::default() };
            let full = grow(&species, &params, Detail::Full, &textures);
            let reduced = grow(&species, &params, Detail::Reduced, &textures);
            let impostor = impostor(&full, &textures);
            GrownVariant { full: full.meshes, reduced: reduced.meshes, impostor }
        })
        .collect();
    GrownSpecies { id: species.id, textures, variants }
}

/// A species' photographs, from the downloaded content.
fn photos_for(species: &Species, files: &HashMap<String, Arc<Vec<u8>>>) -> Photos {
    let decode = |file: &str| {
        let bytes = files.get(file)?;
        image::load_from_memory(bytes).map(|image| image.to_rgba8()).ok()
    };
    let bark = species.bark.texture.as_deref().and_then(decode);
    let leaves =
        species.leaves.photo.as_ref().and_then(|photo| Some(LeafPhotos::find(decode(&photo.file)?, photo.tips)));
    Photos { bark, leaves }
}

/// Starts growing the templates when the content's species arrive or change.
fn start_growing(content: Res<ContentStore>, mut templates: ResMut<TreeTemplates>) {
    let Some(version) = &content.version else { return };
    let current = templates.growing.as_ref().map(|(version, _)| version).or(templates.grown_from.as_ref());
    if current == Some(version) || content.tree_species.is_empty() {
        return;
    }
    let pool = AsyncComputeTaskPool::get();
    let tasks = content
        .tree_species
        .iter()
        .map(|species| {
            let (species, photos) = (species.clone(), photos_for(species, &content.tree_photos));
            pool.spawn(async move { grow_species(species, photos) })
        })
        .collect();
    templates.growing = Some((version.clone(), tasks));
}

/// Once every species has grown, makes them into meshes and materials.
fn finish_growing(
    mut templates: ResMut<TreeTemplates>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TreeMaterial>>,
) {
    let Some((_, tasks)) = &templates.growing else { return };
    if !tasks.iter().all(Task::is_finished) {
        return;
    }
    let Some((version, tasks)) = templates.growing.take() else { return };
    let mut by_species = HashMap::new();
    for mut task in tasks {
        let Some(grown) = check_ready(&mut task) else { continue };
        let bark_image = images.add(texture(&grown.textures.bark, true, None));
        let leaves_image = images.add(texture(&grown.textures.leaves, false, Some(ALPHA_CUTOFF)));
        let bark = materials.add(fading(StandardMaterial {
            base_color_texture: Some(bark_image),
            perceptual_roughness: 0.95,
            reflectance: 0.25,
            // (So it can fade: see `shaders/tree.wgsl`.)
            alpha_mode: AlphaMode::AlphaToCoverage,
            ..default()
        }));
        let leaves = materials.add(fading(leaf_material(leaves_image)));
        let variants = grown
            .variants
            .iter()
            .map(|variant| {
                let impostor_image = images.add(texture(&variant.impostor.texture, false, Some(ALPHA_CUTOFF)));
                Template {
                    full: (meshes.add(mesh(&variant.full.bark)), meshes.add(mesh(&variant.full.leaves))),
                    reduced: (meshes.add(mesh(&variant.reduced.bark)), meshes.add(mesh(&variant.reduced.leaves))),
                    impostor: meshes.add(mesh(&variant.impostor.mesh)),
                    bark: bark.clone(),
                    leaves: leaves.clone(),
                    impostor_material: materials.add(fading(leaf_material(impostor_image))),
                }
            })
            .collect();
        by_species.insert(grown.id, variants);
    }
    info!("Trees ready: {} species, {} variants each.", by_species.len(), VARIANTS.len());
    templates.species = by_species;
    templates.grown_from = Some(version);
    templates.generation += 1;
}

/// A material that fades close to the camera (see [`take_drawing`]).
fn fading(base: StandardMaterial) -> TreeMaterial {
    TreeMaterial { base, extension: TreeFade::default() }
}

/// Takes how trees are drawn from the content: the fade into every tree
/// material, and (by replanting) the levels of detail.
fn take_drawing(
    content: Res<ContentStore>,
    mut templates: ResMut<TreeTemplates>,
    mut materials: ResMut<Assets<TreeMaterial>>,
) {
    let drawing = &content.tree_drawing;
    if *drawing == templates.drawing && !templates.is_changed() {
        return;
    }
    if *drawing != templates.drawing {
        templates.drawing = drawing.clone();
        templates.generation += 1;
    }
    let (near, far) = drawing.camera_fade;
    let settings = FadeSettings { settings: Vec4::new(near, far.max(near + 0.01), 1.0, 0.0) };
    let handles: Vec<Handle<TreeMaterial>> = templates
        .species
        .values()
        .flatten()
        .flat_map(|template| [template.bark.clone(), template.leaves.clone(), template.impostor_material.clone()])
        .collect();
    for handle in handles {
        if let Some(mut material) = materials.get_mut(&handle) {
            material.extension.settings = settings.clone();
        }
    }
}

/// Leaves' material: alpha-tested, lit on both sides with their own normals.
fn leaf_material(image: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(image),
        // (Soft, antialiased edges, and fading: see `shaders/tree.wgsl`.)
        alpha_mode: AlphaMode::AlphaToCoverage,
        cull_mode: None,
        perceptual_roughness: 0.8,
        reflectance: 0.2,
        diffuse_transmission: 0.3,
        ..default()
    }
}

/// A picture as a texture with mipmaps: repeating (bark), or clamped with
/// its coverage kept down the mipmaps (leaves and impostors).
fn texture(picture: &image::RgbaImage, repeats: bool, cutoff: Option<f32>) -> Image {
    let size = Extent3d { width: picture.width(), height: picture.height(), depth_or_array_layers: 1 };
    let mut image = Image::new(
        size,
        TextureDimension::D2,
        picture.as_raw().clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    match cutoff {
        Some(cutoff) => add_mipmaps_keeping_coverage(&mut image, cutoff),
        None => add_mipmaps(&mut image),
    };
    let address = if repeats { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    });
    image
}

/// A generator mesh as a Bevy mesh.
fn mesh(part: &MeshPart) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, part.uvs.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, part.colours.clone())
        .with_inserted_indices(Indices::U32(part.indices.clone()))
}

/// Plants trees in chunks that are shown and haven't been planted with the
/// latest templates.
fn plant_trees(
    mut commands: Commands,
    templates: Res<TreeTemplates>,
    shown: Res<ShownChunks>,
    store: Res<ChunkStore>,
    world: Option<Res<WorldInfo>>,
    chunks: Query<(&GlobalTransform, Option<&Planted>, Option<&Children>), Without<MeshTask>>,
    parts: Query<(), With<TreePart>>,
) {
    let Some(world) = world else { return };
    if templates.species.is_empty() {
        return;
    }
    for (pos, &entity) in &shown.0 {
        let Ok((chunk_place, planted, children)) = chunks.get(entity) else { continue };
        if planted.is_some_and(|planted| planted.0 == templates.generation) {
            continue;
        }
        let Some(chunk) = store.0.get(pos) else { continue };
        // Trees from older templates go.
        for child in children.into_iter().flatten().filter(|child| parts.contains(**child)) {
            commands.entity(*child).despawn();
        }
        commands.entity(entity).insert(Planted(templates.generation));
        plant_chunk(&mut commands, entity, chunk_place, chunk, &templates, &world.0.tree_species);
    }
}

fn plant_chunk(
    commands: &mut Commands,
    entity: Entity,
    chunk_place: &GlobalTransform,
    chunk: &Chunk,
    templates: &TreeTemplates,
    species_ids: &[String],
) {
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    let drawing = &templates.drawing;
    let range = |(start, end): (f32, f32)| start..end.max(start);
    let ranges = [
        VisibilityRange { start_margin: 0.0..0.0, end_margin: range(drawing.full_until), use_aabb: false },
        VisibilityRange {
            start_margin: range(drawing.full_until),
            end_margin: range(drawing.reduced_until),
            use_aabb: false,
        },
        VisibilityRange {
            start_margin: range(drawing.reduced_until),
            end_margin: range(drawing.impostors_until),
            use_aabb: false,
        },
    ];
    for prop in &chunk.props {
        let PropKind::Tree { species, variant } = prop.kind else { continue };
        let (local_x, local_y) = (prop.x - origin_x as f32, prop.y - origin_y as f32);
        let Some(variants) = species_ids
            .get(usize::from(species))
            .and_then(|id| templates.species.get(id))
            .filter(|variants| !variants.is_empty())
        else {
            continue;
        };
        let template = &variants[usize::from(variant).min(variants.len() - 1)];
        let scale = prop.scale;
        // Sunk into the ground, so the trunk's base stays buried on a slope.
        let sink = ground_slope(chunk, local_x, local_y) * TRUNK_BASE_REACH + 0.1;
        let local = Transform::from_translation(to_bevy(local_x, local_y, prop.ground_height - sink))
            .with_rotation(Quat::from_rotation_y(prop.rotation))
            .with_scale(Vec3::splat(scale));
        // (Placed now, as the chunk's other children are: see `finish_mesh_tasks`.)
        let placed = (local, chunk_place.mul_transform(local));
        let pieces = [
            (template.full.0.clone(), template.bark.clone(), 0),
            (template.full.1.clone(), template.leaves.clone(), 0),
            (template.reduced.0.clone(), template.bark.clone(), 1),
            (template.reduced.1.clone(), template.leaves.clone(), 1),
            (template.impostor.clone(), template.impostor_material.clone(), 2),
        ];
        let mut chunk_entity = commands.entity(entity);
        for (mesh, material, level) in pieces {
            chunk_entity.with_child((TreePart, Mesh3d(mesh), MeshMaterial3d(material), placed, ranges[level].clone()));
        }
    }
}

/// How steep the ground is at a point in a chunk (metres of height a metre across).
fn ground_slope(chunk: &Chunk, local_x: f32, local_y: f32) -> f32 {
    let last = CHUNK_SIZE as usize - 1;
    let heights = chunk.cell_corner_heights((local_x as usize).min(last), (local_y as usize).min(last));
    let highest = heights.iter().copied().fold(f32::MIN, f32::max);
    let lowest = heights.iter().copied().fold(f32::MAX, f32::min);
    highest - lowest
}
