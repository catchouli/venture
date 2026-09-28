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
use venture_content::trees::{Species, TreeDrawing, small_tree_name, tree_name};
use venture_protocol::{EntityId, ServerMessage, TreeId};
use venture_rpg::woodcutting::{FALLEN_LENGTHS, TreeSize, tree_level};
use venture_trees::impostor::{Impostor, impostor};
use venture_trees::photos::{LeafPhotos, Photos};
use venture_trees::{Detail, MeshPart, TreeParams, TreeTextures, grow, textures};
use venture_world::{CHUNK_SIZE, Chunk, Prop, PropKind};

use crate::common::coords::to_bevy;
use crate::common::mipmaps::{add_mipmaps, add_mipmaps_keeping_coverage};
use crate::content::ContentStore;
use crate::interaction::targeting::{Level, PickShape, Target, TargetKind, TargetLabel, Targetable};
use crate::network::FromServer;
use crate::world::query::ground_height;
use crate::world::source::WorldInfo;
use crate::world::streaming::{ChunkStore, MeshTask, ShownChunks};

/// Each species' variants: (age, seed). Young, mature and old (as
/// [`PropKind::Tree`]'s variant numbers them).
const VARIANTS: [(f32, u64); 3] = [(0.55, 11), (1.0, 12), (1.4, 13)];
/// Leaves are see-through below this coverage.
const ALPHA_CUTOFF: f32 = 0.5;
/// A standing tree can be picked this high up its trunk, in metres...
const TRUNK_PICK_HEIGHT: f32 = 5.0;
/// ...and this far round it.
const TRUNK_PICK_RADIUS: f32 = 0.8;
/// A fallen tree can be picked this far round its trunk, in metres.
const FALLEN_PICK_RADIUS: f32 = 0.9;
/// Trees can only be picked this near the player, in metres.
const PICK_TREES_WITHIN: f32 = 40.0;
/// How tall a felled tree's stump is, in metres.
const STUMP_HEIGHT: f32 = 0.55;
/// How wide young, mature and old trees' stumps (and fallen trunks) are, in metres of radius.
const STUMP_RADII: [f32; 3] = [0.22, 0.38, 0.55];
/// How far apart the ground is sampled under a fallen tree, in metres.
const TILT_SAMPLE_SPACING: f32 = 1.0;
/// How far to each side of its trunk a fallen tree's crown rests, in metres.
const CROWN_SPREAD: f32 = 1.5;
/// The most a fallen tree tilts up (over a rise) or down (down a slope), in radians.
const MOST_TILT_UP: f32 = 0.8;
const MOST_TILT_DOWN: f32 = 0.6;
/// How long a felled tree takes to tip over, in seconds.
const FALL_SECONDS: f32 = 2.2;
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
            .init_resource::<FelledTrees>()
            .add_systems(
                Update,
                (
                    start_growing,
                    finish_growing,
                    take_drawing,
                    plant_trees,
                    follow_felling,
                    show_felled,
                    dress_fallen,
                    tip_over,
                )
                    .chain(),
            );
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
    /// A stump: a cylinder a metre high and a metre in radius (scaled to size).
    stump: Option<Handle<Mesh>>,
    /// Counts up each time the templates (or how they're drawn) change, so chunks replant.
    generation: u32,
}

/// One variant of a species, ready to draw.
#[derive(Clone)]
pub struct Template {
    /// How tall it is, in metres.
    height: f32,
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
    /// How tall it is, in metres.
    height: f32,
    full: venture_trees::TreeMeshes,
    reduced: venture_trees::TreeMeshes,
    impostor: Impostor,
}

/// A standing tree (or bush) that can be selected and felled: its id.
#[derive(Component)]
pub struct StandingTree(pub TreeId);

/// What a planted tree looks like (for its stump, once felled).
#[derive(Component)]
struct TreeLook {
    bark: Handle<TreeMaterial>,
    variant: u8,
}

/// A part of a standing tree's body (hidden while it's felled).
#[derive(Component)]
struct TreeBody;

/// A felled tree's stump.
#[derive(Component)]
struct Stump;

/// A fallen tree: an entity of the server's (see `characters::remote`), drawn here.
#[derive(Component)]
pub struct Fallen {
    /// The tree it was.
    pub tree: TreeId,
    /// Which way it fell (radians round the vertical, as props turn).
    pub toward: f32,
}

/// A fallen tree has been drawn, this long (its trunk, in metres).
#[derive(Component)]
pub struct Dressed(pub f32);

/// The pivot at a fallen tree's foot, which tips it over.
#[derive(Component)]
struct FallenPivot {
    /// How high its trunk's middle lies once down (its radius), in metres.
    lying_radius: f32,
    /// How far it tips (radians from upright): a right angle on the flat,
    /// less where the ground rises under it, more where it falls away.
    tipped: f32,
}

/// A fallen tree still tipping over: seconds since it started.
#[derive(Component)]
struct Falling {
    since: f32,
}

/// The trees felled (and not grown back yet), as the server says, with when
/// the client heard (seconds; long ago for those felled before joining).
#[derive(Resource, Default)]
pub struct FelledTrees(pub HashMap<TreeId, f32>);

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
            GrownVariant { height: full.height, full: full.meshes, reduced: reduced.meshes, impostor }
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
                    height: variant.height,
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
    templates.stump.get_or_insert_with(|| meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(10)));
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
    content: Res<ContentStore>,
    chunks: Query<(&GlobalTransform, Option<&Planted>, Option<&Children>), Without<MeshTask>>,
    parts: Query<(), With<TreePart>>,
) {
    let Some(world) = world else { return };
    if templates.species.is_empty() {
        return;
    }
    let facts = tree_facts(&content);
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
        plant_chunk(&mut commands, entity, chunk_place, chunk, &templates, &world.0.tree_species, &facts);
    }
}

/// What a species is called, and the woodworking level it takes to fell a
/// full-grown one (by species id), for selecting trees.
fn tree_facts(content: &ContentStore) -> HashMap<String, (String, u32)> {
    content
        .tree_species
        .iter()
        .filter_map(|species| {
            let entry = content.tree_woodcutting.iter().find(|entry| entry.species == species.id)?;
            Some((species.id.clone(), (species.name.clone(), entry.level)))
        })
        .collect()
}

/// A tree prop's template and its place in its chunk (transform), unless its
/// species isn't grown.
fn template_for<'a>(
    templates: &'a TreeTemplates,
    species_ids: &[String],
    chunk: &Chunk,
    prop: &Prop,
) -> Option<(&'a Template, Transform)> {
    let PropKind::Tree { species, variant } = prop.kind else { return None };
    let variants = species_ids
        .get(usize::from(species))
        .and_then(|id| templates.species.get(id))
        .filter(|variants| !variants.is_empty())?;
    let template = &variants[usize::from(variant).min(variants.len() - 1)];
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    let (local_x, local_y) = (prop.x - origin_x as f32, prop.y - origin_y as f32);
    // Sunk into the ground, so the trunk's base stays buried on a slope.
    let sink = ground_slope(chunk, local_x, local_y) * TRUNK_BASE_REACH + 0.1;
    let local = Transform::from_translation(to_bevy(local_x, local_y, prop.ground_height - sink))
        .with_rotation(Quat::from_rotation_y(prop.rotation))
        .with_scale(Vec3::splat(prop.scale));
    Some((template, local))
}

/// The ranges each level of detail shows over: full, reduced, impostor.
fn lod_ranges(drawing: &TreeDrawing) -> [VisibilityRange; 3] {
    let range = |(start, end): (f32, f32)| start..end.max(start);
    [
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
    ]
}

fn plant_chunk(
    commands: &mut Commands,
    entity: Entity,
    chunk_place: &GlobalTransform,
    chunk: &Chunk,
    templates: &TreeTemplates,
    species_ids: &[String],
    facts: &HashMap<String, (String, u32)>,
) {
    let ranges = lod_ranges(&templates.drawing);
    for (index, prop) in chunk.props.iter().enumerate() {
        let Some((template, local)) = template_for(templates, species_ids, chunk, prop) else { continue };
        let PropKind::Tree { species, variant } = prop.kind else { continue };
        // (Placed now, as the chunk's other children are: see `finish_mesh_tasks`.)
        let global = chunk_place.mul_transform(local);
        let root = commands
            .spawn((
                TreePart,
                TreeLook { bark: template.bark.clone(), variant },
                local,
                global,
                Visibility::default(),
                ChildOf(entity),
            ))
            .id();
        // Trees (and bushes: small young trees) of species that can be felled can be selected.
        let facts = species_ids.get(usize::from(species)).and_then(|id| facts.get(id));
        if let Some((species_name, species_level)) = facts {
            // Small (young) trees are easier to fell (and give less).
            let size = TreeSize::of_variant(variant);
            let name = if size == TreeSize::Young { small_tree_name(species_name) } else { tree_name(species_name) };
            let level = tree_level(*species_level, size);
            let id = TreeId { chunk_x: chunk.pos.x, chunk_y: chunk.pos.y, index: index as u16 };
            commands.entity(root).insert((
                StandingTree(id),
                Targetable { id: EntityId::NONE, kind: TargetKind::Tree },
                TargetLabel(name),
                Level(level),
                PickShape {
                    from: Vec3::ZERO,
                    to: Vec3::Y * TRUNK_PICK_HEIGHT / prop.scale,
                    radius: TRUNK_PICK_RADIUS,
                    within: PICK_TREES_WITHIN,
                },
            ));
        }
        let pieces = [
            (template.full.0.clone(), template.bark.clone(), 0),
            (template.full.1.clone(), template.leaves.clone(), 0),
            (template.reduced.0.clone(), template.bark.clone(), 1),
            (template.reduced.1.clone(), template.leaves.clone(), 1),
            (template.impostor.clone(), template.impostor_material.clone(), 2),
        ];
        for (mesh, material, level) in pieces {
            commands.spawn((
                TreeBody,
                Mesh3d(mesh),
                MeshMaterial3d(material),
                (Transform::IDENTITY, global),
                ranges[level].clone(),
                ChildOf(root),
            ));
        }
    }
}

/// Keeps track of which trees are felled, as the server says.
fn follow_felling(time: Res<Time>, mut events: MessageReader<FromServer>, mut felled: ResMut<FelledTrees>) {
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::TreesFelled { trees } => {
                felled.0 = trees.iter().map(|&tree| (tree, f32::NEG_INFINITY)).collect();
            }
            ServerMessage::TreeFelled { tree } => {
                felled.0.insert(*tree, time.elapsed_secs());
            }
            ServerMessage::TreeGrewBack { tree } => {
                felled.0.remove(tree);
            }
            _ => {}
        }
    }
}

/// Felled trees hide behind a stump; trees that grow back come back.
fn show_felled(
    mut commands: Commands,
    felled: Res<FelledTrees>,
    templates: Res<TreeTemplates>,
    trees: Query<(Entity, &StandingTree, &TreeLook, &GlobalTransform, &Children, Ref<StandingTree>, Has<Targetable>)>,
    mut bodies: Query<&mut Visibility, With<TreeBody>>,
    stumps: Query<(), With<Stump>>,
    mut target: ResMut<Target>,
) {
    let Some(stump_mesh) = templates.stump.clone() else { return };
    for (root, &StandingTree(id), look, place, children, added, selectable) in &trees {
        if !felled.is_changed() && !added.is_added() {
            continue;
        }
        let is_felled = felled.0.contains_key(&id);
        // A felled tree can't be selected (and stops being, if it was).
        if is_felled {
            commands.entity(root).remove::<Targetable>();
            if target.0 == Some(root) {
                target.0 = None;
            }
        } else if !selectable {
            commands.entity(root).insert(Targetable { id: EntityId::NONE, kind: TargetKind::Tree });
        }
        for child in children {
            if let Ok(mut visibility) = bodies.get_mut(*child) {
                visibility.set_if_neq(if is_felled { Visibility::Hidden } else { Visibility::Inherited });
            }
        }
        let stump = children.iter().find(|child| stumps.contains(*child));
        match (is_felled, stump) {
            (true, None) => {
                let radius = STUMP_RADII[usize::from(look.variant.min(2))];
                let local = Transform::from_xyz(0.0, STUMP_HEIGHT / 2.0, 0.0).with_scale(Vec3::new(
                    radius,
                    STUMP_HEIGHT,
                    radius,
                ));
                commands.spawn((
                    Stump,
                    Mesh3d(stump_mesh.clone()),
                    MeshMaterial3d(look.bark.clone()),
                    (local, place.mul_transform(local)),
                    ChildOf(root),
                ));
            }
            (false, Some(stump)) => commands.entity(stump).despawn(),
            _ => {}
        }
    }
}

/// Draws fallen trees (entities of the server's: see
/// `characters::remote`) as their tree, lying the way it fell: tipping over
/// first, if it's only just been felled.
fn dress_fallen(
    mut commands: Commands,
    time: Res<Time>,
    templates: Res<TreeTemplates>,
    store: Res<ChunkStore>,
    world: Option<Res<WorldInfo>>,
    felled: Res<FelledTrees>,
    mut fallen: Query<(Entity, &Fallen, &mut Transform), Without<Dressed>>,
) {
    let Some(world) = world else { return };
    if templates.species.is_empty() {
        return;
    }
    let ranges = lod_ranges(&templates.drawing);
    for (entity, fallen_tree, mut transform) in &mut fallen {
        let tree = fallen_tree.tree;
        let Some(chunk) = store.0.get(&venture_world::ChunkPos::new(tree.chunk_x, tree.chunk_y)) else { continue };
        let Some(prop) = chunk.props.get(usize::from(tree.index)) else { continue };
        let Some((template, local)) = template_for(&templates, &world.0.tree_species, chunk, prop) else { continue };
        let PropKind::Tree { variant, .. } = prop.kind else { continue };
        let lying_radius = STUMP_RADII[usize::from(variant.min(2))] * prop.scale;
        let length = FALLEN_LENGTHS[usize::from(variant.min(2))] * prop.scale;
        commands.entity(entity).insert(Dressed(length));
        // The root turns to the way it fell; a pivot at its foot tips it over.
        transform.rotation = Quat::from_rotation_y(fallen_tree.toward);
        let base = (prop.x, prop.y);
        let tilt = resting_tilt(&store, base, fallen_tree.toward, template.height * prop.scale);
        let just_felled = felled.0.get(&tree).is_some_and(|&at| time.elapsed_secs() - at < 1.0);
        let pivot = commands
            .spawn((
                Transform::from_scale(local.scale),
                Visibility::default(),
                ChildOf(entity),
                FallenPivot { lying_radius, tipped: std::f32::consts::FRAC_PI_2 - tilt },
            ))
            .id();
        let pieces = [
            (template.full.0.clone(), template.bark.clone(), ranges[0].clone()),
            (template.full.1.clone(), template.leaves.clone(), ranges[0].clone()),
            // (No impostor lying down: the reduced tree goes on to the end.)
            (
                template.reduced.0.clone(),
                template.bark.clone(),
                VisibilityRange { end_margin: ranges[2].end_margin.clone(), ..ranges[1].clone() },
            ),
            (
                template.reduced.1.clone(),
                template.leaves.clone(),
                VisibilityRange { end_margin: ranges[2].end_margin.clone(), ..ranges[1].clone() },
            ),
        ];
        for (mesh, material, range) in pieces {
            commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::IDENTITY, range, ChildOf(pivot)));
        }
        commands.entity(entity).insert(PickShape {
            from: Vec3::Y * lying_radius,
            to: Vec3::new(0.0, lying_radius + length * tilt.sin(), length * tilt.cos()),
            radius: FALLEN_PICK_RADIUS,
            within: PICK_TREES_WITHIN,
        });
        commands.entity(pivot).insert(Falling { since: if just_felled { 0.0 } else { FALL_SECONDS } });
    }
}

/// Tips fallen trees over: slowly at first, faster as they go (as a falling
/// tree does), landing with a small bounce.
fn tip_over(
    mut commands: Commands,
    time: Res<Time>,
    mut pivots: Query<(Entity, &mut Falling, &FallenPivot, &mut Transform)>,
) {
    for (entity, mut falling, pivot, mut transform) in &mut pivots {
        falling.since += time.delta_secs();
        let t = (falling.since / FALL_SECONDS).min(1.0);
        // Accelerating (squared), then a small bounce as it lands.
        let bounce = if t > 0.85 { (1.0 - t) / 0.15 * (1.0 - (1.0 - t) / 0.15) * 0.08 } else { 0.0 };
        let share = (t * t - bounce).clamp(0.0, 1.0);
        transform.rotation = Quat::from_rotation_x(pivot.tipped * share);
        transform.translation.y = pivot.lying_radius * share;
        if t >= 1.0 {
            commands.entity(entity).remove::<Falling>();
        }
    }
}

/// How far a fallen tree tilts up from the flat (radians; negative tilts
/// down) to rest on its foot and the highest ground under it, rather than
/// through a hillside: the steepest the ground rises (or least it falls)
/// along where it lies, `length` metres from `base` towards `toward`, and a
/// little to each side under its crown.
fn resting_tilt(store: &ChunkStore, base: (f32, f32), toward: f32, length: f32) -> f32 {
    let Some(foot) = ground_height(store, base.0, base.1) else { return 0.0 };
    let (dx, dy) = (toward.sin(), toward.cos());
    let steps = (length / TILT_SAMPLE_SPACING).ceil().max(1.0) as u32;
    let mut steepest = f32::NEG_INFINITY;
    for step in 1..=steps {
        let along = length * step as f32 / steps as f32;
        // Under the crown (the outer part), a little to each side too.
        let sides: &[f32] = if along > length * 0.4 { &[-CROWN_SPREAD, 0.0, CROWN_SPREAD] } else { &[0.0] };
        for &side in sides {
            let (x, y) = (base.0 + dx * along + dy * side, base.1 + dy * along - dx * side);
            if let Some(height) = ground_height(store, x, y) {
                steepest = steepest.max((height - foot).atan2(along));
            }
        }
    }
    if steepest.is_finite() { steepest.clamp(-MOST_TILT_DOWN, MOST_TILT_UP) } else { 0.0 }
}

/// How steep the ground is at a point in a chunk (metres of height a metre across).
fn ground_slope(chunk: &Chunk, local_x: f32, local_y: f32) -> f32 {
    let last = CHUNK_SIZE as usize - 1;
    let heights = chunk.cell_corner_heights((local_x as usize).min(last), (local_y as usize).min(last));
    let highest = heights.iter().copied().fold(f32::MIN, f32::max);
    let lowest = heights.iter().copied().fold(f32::MAX, f32::min);
    highest - lowest
}
