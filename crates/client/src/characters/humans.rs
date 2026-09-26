//! People as real models: a body, hair and outfit parts on one shared
//! skeleton, animated from what the character is doing. The models and
//! animations are content (`characters/`, see [`CharacterModels`]); until
//! they've arrived, and if the content has none, people keep their simple
//! placeholder bodies (see [`crate::characters::model`]).
//!
//! A person is any character with a [`HumanLook`]: the players, the
//! character creation preview, and NPCs. Dressing one goes like this:
//! 1. Its placeholder body goes, and a glTF scene is spawned for its body,
//!    hair, and each outfit part.
//! 2. Once they've spawned, each part's and the hair's meshes are pointed
//!    at the body's bones (so one skeleton moves everything), and the parts'
//!    own skeletons go. Held things (a sword, a shield) are hung on their
//!    bone instead.
//! 3. The body's mesh loses the triangles clothes cover (so they don't poke
//!    through), the skin and hair are coloured, and the body's skeleton is
//!    given the animations.
//!
//! Changing a person's look dresses them again.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::camera::visibility::RenderLayers;
use bevy::gltf::Gltf;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstance, WorldInstanceSpawner};
use venture_content::AppearanceDef;
use venture_content::characters::{BodyRegion, CharacterModels, PartModel};
use venture_protocol::{BodyType, ItemLook, LookStyle, PlayerLook};

use crate::characters::model::{CharacterAssets, Gait, PlaceholderPart};
use crate::content::{CONTENT_SOURCE, ContentStore};

/// Players' hair colour (until they can choose one).
const PLAYER_HAIR: Color = Color::srgb(0.32, 0.21, 0.12);
/// How long animations take to blend into each other, in seconds.
const BLEND_SECONDS: f32 = 0.2;
/// Moving slower than this (metres per second) walks rather than runs.
const WALK_BELOW: f32 = 2.5;
/// The speeds the walk and run animations were made at (metres per second),
/// to play them faster or slower to match.
const WALK_ANIMATION_SPEED: f32 = 1.6;
const RUN_ANIMATION_SPEED: f32 = 4.2;

/// Dresses and animates people.
pub struct HumansPlugin;

impl Plugin for HumansPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HumanModels>().init_resource::<MaskedMeshes>().add_systems(
            Update,
            (load_models, dress_humans, assemble_humans, remove_placeholders, animate_humans).chain(),
        );
    }
}

/// How a person looks: what the models make them from.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct HumanLook {
    /// Their build.
    pub body: BodyType,
    /// Their hairstyle, if not their body's usual one.
    pub hair: Option<String>,
    /// Their outfit parts (clothes, armour, and things held).
    pub parts: Vec<String>,
    /// Whether their skin is the paler one.
    pub light_skin: bool,
    /// A colour their hair is tinted, if any.
    pub hair_colour: Option<Color>,
    /// Simple shapes for held things that may have no model (a staff):
    /// drawn in the hand, unless an outfit part is held there instead.
    pub shapes: Vec<ItemLook>,
}

impl HumanLook {
    /// A player's look: their build, and the outfit parts of what they wear.
    pub fn of_player(look: &PlayerLook) -> Self {
        // (Brown hair, until players can choose.)
        let hair_colour = Some(PLAYER_HAIR);
        let shapes = look
            .worn
            .iter()
            .filter(|item| {
                matches!(item.style, LookStyle::Sword | LookStyle::Staff | LookStyle::Club | LookStyle::Buckler)
            })
            .copied()
            .collect();
        Self { body: look.body, hair: None, parts: look.outfit.clone(), light_skin: true, hair_colour, shapes }
    }

    /// An NPC's look, from their appearance: its parts (or the ones for its
    /// outfit style), hairstyle, skin (paler for light skin colours) and
    /// hair colour.
    pub fn of_appearance(appearance: Option<&AppearanceDef>, models: Option<&CharacterModels>) -> Self {
        let Some(appearance) = appearance else {
            return Self {
                body: BodyType::Masculine,
                hair: None,
                parts: Vec::new(),
                light_skin: true,
                hair_colour: None,
                shapes: Vec::new(),
            };
        };
        let parts = if appearance.parts.is_empty() {
            let style = format!("{:?}", appearance.outfit);
            models.and_then(|models| models.outfits.get(&style)).cloned().unwrap_or_default()
        } else {
            appearance.parts.clone()
        };
        let (r, g, b) = appearance.skin;
        let (hr, hg, hb) = appearance.hair;
        Self {
            body: appearance.body,
            hair: appearance.hairstyle.clone(),
            parts,
            light_skin: 0.3 * r + 0.55 * g + 0.15 * b > 0.6,
            hair_colour: Some(Color::srgb(hr, hg, hb)),
            shapes: Vec::new(),
        }
    }
}

/// A person dressed in models (so their placeholder body goes).
#[derive(Component)]
pub struct Dressed;

/// A person whose models are still spawning.
#[derive(Component)]
struct Assembling;

/// One of a person's models: the scene's root, under the person.
#[derive(Component, Clone)]
struct HumanScene(SceneKind);

#[derive(Clone)]
enum SceneKind {
    Body,
    Hair,
    /// Clothes or armour, worn on the skeleton.
    Worn,
    /// Something held, hung on a bone.
    Held(venture_content::characters::Held),
}

/// A dressed person's skeleton, for animating.
#[derive(Component)]
pub struct HumanRig {
    /// The entity with the animation player (the body's armature).
    armature: Entity,
    /// The body scene's root (moved up and down for jumps).
    body_root: Entity,
    /// Whether they're holding a weapon (for which attack animation to play).
    armed: bool,
    /// What's playing, and since when (seconds of game time).
    playing: Option<(&'static str, f32)>,
    /// The swing (or wave) already started, so it isn't started again.
    started_swing: bool,
    started_wave: bool,
    /// Whether they were resting (to stand up when they stop).
    was_resting: bool,
}

/// The character models, loading or loaded.
#[derive(Resource, Default)]
pub struct HumanModels {
    /// What the content says, if it has models.
    models: Option<CharacterModels>,
    /// Each model file, by name.
    files: HashMap<String, Handle<Gltf>>,
    /// Textures used as they are (paler skins), by name.
    textures: HashMap<String, Handle<Image>>,
    /// The animations, once loaded: the graph, and each animation's node
    /// and length (seconds) by the game's name for it.
    animations: Option<(Handle<AnimationGraph>, HashMap<String, (AnimationNodeIndex, f32)>)>,
    /// The content version these are for.
    version: Option<String>,
}

impl HumanModels {
    /// A model file's glTF, if it's loaded.
    fn gltf<'a>(&self, gltfs: &'a Assets<Gltf>, file: &str) -> Option<&'a Gltf> {
        gltfs.get(self.files.get(file)?)
    }
}

/// Bodies with the covered parts taken out, by body mesh and what's covered.
#[derive(Resource, Default)]
struct MaskedMeshes(HashMap<(AssetId<Mesh>, Vec<BodyRegion>), Handle<Mesh>>);

fn content_path(file: &str) -> String {
    format!("{CONTENT_SOURCE}://characters/{file}")
}

/// Loads the models the content names (again, when they change), and
/// redresses everyone when they do.
fn load_models(
    mut commands: Commands,
    content: Res<ContentStore>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut models: ResMut<HumanModels>,
    dressed: Query<Entity, With<Dressed>>,
) {
    if content.is_changed() && content.version.is_some() && content.version != models.version {
        for path in &content.changed_models {
            asset_server.reload(format!("{CONTENT_SOURCE}://{path}"));
        }
        let defs = content.character_models.clone();
        let mut files = HashMap::new();
        let mut textures = HashMap::new();
        if let Some(defs) = &defs {
            let bodies =
                [BodyType::Masculine, BodyType::Feminine, BodyType::Androgynous].map(|body| defs.bodies.of(body));
            let parts = defs.parts.values().flat_map(|part| [&part.masculine, &part.feminine]).flatten();
            let names = bodies.iter().map(|body| &body.model).chain(defs.hair.values()).chain(parts);
            for file in names.chain([&defs.animations]) {
                files.insert(file.clone(), asset_server.load(content_path(file)));
            }
            for skin in bodies.iter().filter_map(|body| body.light_skin.as_ref()) {
                textures.insert(skin.clone(), asset_server.load(content_path(skin)));
            }
        }
        *models = HumanModels { models: defs, files, textures, animations: None, version: content.version.clone() };
        for person in &dressed {
            commands.entity(person).remove::<Dressed>();
        }
    }
    // The animation graph, once the animations have loaded.
    let Some(defs) = models.models.clone() else { return };
    if models.animations.is_some() {
        return;
    }
    let Some(gltf) = models.gltf(&gltfs, &defs.animations) else { return };
    let named: Vec<(String, Handle<AnimationClip>)> = defs
        .clips
        .iter()
        .filter_map(|(ours, theirs)| Some((ours.clone(), gltf.named_animations.get(theirs.as_str())?.clone())))
        .collect();
    if named.iter().any(|(_, clip)| clips.get(clip).is_none()) {
        return;
    }
    let (graph, nodes) = AnimationGraph::from_clips(named.iter().map(|(_, clip)| clip.clone()));
    let by_name = named
        .iter()
        .zip(nodes)
        .map(|((name, clip), node)| (name.clone(), (node, clips.get(clip).map_or(1.0, AnimationClip::duration))))
        .collect();
    models.animations = Some((graphs.add(graph), by_name));
}

/// Whether everything a look needs has loaded.
fn look_ready(look: &HumanLook, models: &HumanModels, defs: &CharacterModels, asset_server: &AssetServer) -> bool {
    models.animations.is_some()
        && files_for(look, defs)
            .iter()
            .all(|file| models.files.get(file).is_some_and(|handle| asset_server.is_loaded_with_dependencies(handle)))
}

/// The model files a look is made of: body, hair, then parts.
fn files_for(look: &HumanLook, defs: &CharacterModels) -> Vec<String> {
    let body = defs.bodies.of(look.body);
    let mut files = vec![body.model.clone()];
    let hair = look.hair.as_ref().filter(|hair| defs.hair.contains_key(*hair)).unwrap_or(&body.hair);
    files.extend(defs.hair.get(hair).cloned());
    files.extend(worn_parts(look, defs).iter().filter_map(|(_, part)| part.model_for(body.fits).cloned()));
    files
}

/// The parts a look wears: its own, then plain clothes where nothing covers.
fn worn_parts<'a>(look: &HumanLook, defs: &'a CharacterModels) -> Vec<(&'a String, &'a PartModel)> {
    let mut parts: Vec<_> = look.parts.iter().filter_map(|name| defs.parts.get_key_value(name)).collect();
    let covered: HashSet<BodyRegion> = parts.iter().flat_map(|(_, part)| part.covers.iter().copied()).collect();
    for (name, part) in defs.plain_clothes.iter().filter_map(|name| defs.parts.get_key_value(name)) {
        if !part.covers.iter().any(|region| covered.contains(region)) {
            parts.push((name, part));
        }
    }
    parts
}

/// Dresses people whose look is new or changed (once their models have
/// loaded): spawns a scene for each model under them.
fn dress_humans(
    mut commands: Commands,
    models: Res<HumanModels>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    people: Query<(Entity, &HumanLook, Option<&Children>), Or<(Changed<HumanLook>, Without<Dressed>)>>,
    scenes: Query<(), With<HumanScene>>,
) {
    let Some(defs) = &models.models else { return };
    for (person, look, children) in &people {
        // (Until everything's loaded, they keep what they have.)
        if !look_ready(look, &models, defs, &asset_server) {
            continue;
        }
        // Out with the old models.
        for child in children.into_iter().flatten().filter(|child| scenes.contains(**child)) {
            commands.entity(*child).despawn();
        }
        let body = defs.bodies.of(look.body);
        let scene = |file: &str| models.gltf(&gltfs, file).and_then(|gltf| gltf.scenes.first().cloned());
        let mut spawn = |file: &str, kind: SceneKind| {
            if let Some(handle) = scene(file) {
                commands.spawn((WorldAssetRoot(handle), HumanScene(kind), ChildOf(person)));
            }
        };
        spawn(&body.model, SceneKind::Body);
        let hair = look.hair.as_ref().filter(|hair| defs.hair.contains_key(*hair)).unwrap_or(&body.hair);
        let hides_hair = worn_parts(look, defs).iter().any(|(_, part)| part.hides_hair);
        if let Some(file) = defs.hair.get(hair).filter(|_| !hides_hair) {
            spawn(file, SceneKind::Hair);
        }
        for (_, part) in worn_parts(look, defs) {
            let Some(file) = part.model_for(body.fits) else { continue };
            let kind = part.held.clone().map_or(SceneKind::Worn, SceneKind::Held);
            spawn(file, kind);
        }
        commands.entity(person).insert((Dressed, Assembling)).remove::<HumanRig>();
    }
}

/// Everything assembling a person needs.
#[derive(bevy::ecs::system::SystemParam)]
struct Assembly<'w, 's> {
    commands: Commands<'w, 's>,
    models: Res<'w, HumanModels>,
    spawner: Res<'w, WorldInstanceSpawner>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    masked: ResMut<'w, MaskedMeshes>,
    children: Query<'w, 's, &'static Children>,
    names: Query<'w, 's, &'static Name>,
    instances: Query<'w, 's, &'static WorldInstance>,
    scenes: Query<'w, 's, &'static HumanScene>,
    skinned: Query<'w, 's, (&'static mut SkinnedMesh, &'static Mesh3d)>,
    mesh_materials: Query<'w, 's, &'static MeshMaterial3d<StandardMaterial>>,
    layers: Query<'w, 's, &'static RenderLayers>,
    shapes: Res<'w, CharacterAssets>,
}

/// Finishes dressing people whose models have all spawned.
fn assemble_humans(mut assembly: Assembly, people: Query<(Entity, &HumanLook), With<Assembling>>) {
    let Some(defs) = assembly.models.models.clone() else { return };
    for (person, look) in &people {
        let scene_roots: Vec<Entity> = assembly
            .children
            .get(person)
            .map(|children| children.iter().filter(|child| assembly.scenes.contains(*child)).collect())
            .unwrap_or_default();
        let ready = !scene_roots.is_empty()
            && scene_roots.iter().all(|root| {
                assembly.instances.get(*root).is_ok_and(|instance| assembly.spawner.instance_is_ready(**instance))
            });
        if ready {
            assembly.assemble(person, look, &defs, &scene_roots);
        }
    }
}

impl Assembly<'_, '_> {
    /// Every entity under `root` (itself included), with its name.
    fn named_descendants(&self, root: Entity) -> Vec<(Entity, String)> {
        let mut found = Vec::new();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(name) = self.names.get(entity) {
                found.push((entity, name.as_str().to_string()));
            }
            if let Ok(children) = self.children.get(entity) {
                stack.extend(children.iter());
            }
        }
        found
    }

    /// Every entity under `root` (itself included).
    fn descendants(&self, root: Entity) -> Vec<Entity> {
        let mut found = vec![root];
        let mut index = 0;
        while index < found.len() {
            if let Ok(children) = self.children.get(found[index]) {
                found.extend(children.iter());
            }
            index += 1;
        }
        found
    }

    fn assemble(&mut self, person: Entity, look: &HumanLook, defs: &CharacterModels, scene_roots: &[Entity]) {
        let Some(body_root) =
            scene_roots.iter().copied().find(|root| matches!(self.scenes.get(*root), Ok(HumanScene(SceneKind::Body))))
        else {
            return;
        };
        let bones: HashMap<String, Entity> =
            self.named_descendants(body_root).into_iter().map(|(entity, name)| (name, entity)).collect();
        let Some(&armature) = bones.get("Armature") else { return };
        let covered: Vec<BodyRegion> = {
            let mut regions: Vec<BodyRegion> =
                worn_parts(look, defs).iter().flat_map(|(_, part)| part.covers.iter().copied()).collect();
            regions.sort();
            regions.dedup();
            regions
        };
        let mut armed = false;
        let mut shielded = false;

        for &root in scene_roots {
            let Ok(HumanScene(kind)) = self.scenes.get(root).cloned() else { continue };
            match kind {
                SceneKind::Body => self.prepare_body(root, look, defs, &covered),
                SceneKind::Hair | SceneKind::Worn => {
                    self.bind_to(root, &bones);
                    if matches!(kind, SceneKind::Hair)
                        && let Some(colour) = look.hair_colour
                    {
                        self.tint(root, colour);
                    }
                }
                SceneKind::Held(held) => {
                    armed |= held.bone == "hand_r";
                    shielded |= matches!(held.bone.as_str(), "lowerarm_l" | "hand_l");
                    if let Some(&bone) = bones.get(&held.bone) {
                        let (x, y, z) = held.rotation;
                        let rotation = Quat::from_euler(EulerRot::XYZ, x.to_radians(), y.to_radians(), z.to_radians());
                        let (px, py, pz) = held.position;
                        let transform =
                            Transform::from_xyz(px, py, pz).with_rotation(rotation).with_scale(Vec3::splat(held.scale));
                        self.commands.entity(root).insert((transform, ChildOf(bone)));
                    }
                }
            }
        }

        // Simple shapes for held things without a model, where nothing else is held.
        let shapes: Vec<ItemLook> = look
            .shapes
            .iter()
            .filter(|shape| if shape.style == LookStyle::Buckler { !shielded } else { !armed })
            .copied()
            .collect();
        armed |= shapes.iter().any(|shape| shape.style != LookStyle::Buckler);
        let (hand, forearm) = (bones.get("hand_r").copied(), bones.get("lowerarm_l").copied());
        let assets = &*self.shapes;
        assets.attach_shapes(&mut self.commands, &mut self.materials, &shapes, hand, forearm);

        // The skeleton plays the animations.
        let armature_names = self.named_descendants(armature);
        let paths = paths_from(armature, &armature_names, &self.children, &self.names);
        for (entity, path) in paths {
            let names: Vec<Name> = path.into_iter().map(Name::new).collect();
            self.commands.entity(entity).insert((AnimationTargetId::from_names(names.iter()), AnimatedBy(armature)));
        }
        if let Some((graph, _)) = &self.models.animations {
            self.commands.entity(armature).insert((
                AnimationPlayer::default(),
                AnimationGraphHandle(graph.clone()),
                AnimationTransitions::new(),
            ));
        }
        // Things drawn only by some cameras (the character creation preview)
        // pass that on to their models.
        if let Ok(layers) = self.layers.get(person).cloned() {
            for &root in scene_roots {
                // (Parts' own skeletons are already on their way out.)
                for entity in self.descendants(root) {
                    self.commands.entity(entity).try_insert(layers.clone());
                }
            }
        }
        self.commands.entity(person).remove::<Assembling>().insert(HumanRig {
            armature,
            body_root,
            armed,
            playing: None,
            started_swing: false,
            started_wave: false,
            was_resting: false,
        });
    }

    /// Points a part's (or hair's) meshes at the body's bones, and removes
    /// the part's own skeleton.
    fn bind_to(&mut self, root: Entity, bones: &HashMap<String, Entity>) {
        let own = self.named_descendants(root);
        for (entity, _) in &own {
            let Ok((mut skinned, _)) = self.skinned.get_mut(*entity) else { continue };
            let joints: Vec<Entity> = skinned
                .joints
                .iter()
                .map(|joint| {
                    self.names.get(*joint).ok().and_then(|name| bones.get(name.as_str()).copied()).unwrap_or(*joint)
                })
                .collect();
            skinned.joints = joints;
        }
        if let Some((skeleton, _)) = own.iter().find(|(_, name)| name == "root") {
            self.commands.entity(*skeleton).despawn();
        }
    }

    /// Hides the body under clothes, and gives it the right skin.
    fn prepare_body(&mut self, root: Entity, look: &HumanLook, defs: &CharacterModels, covered: &[BodyRegion]) {
        let light = defs.bodies.of(look.body).light_skin.as_ref().filter(|_| look.light_skin);
        let light = light.and_then(|file| self.models.textures.get(file).cloned());
        for entity in self.descendants(root) {
            let Ok((skinned, mesh)) = self.skinned.get(entity) else { continue };
            // The body itself is the mesh with the most vertices (the others are eyes and eyebrows).
            let Some(source) = self.meshes.get(&mesh.0) else { continue };
            if source.count_vertices() < 1000 {
                continue;
            }
            if !covered.is_empty() {
                let key = (mesh.0.id(), covered.to_vec());
                let masked = match self.masked.0.get(&key) {
                    Some(masked) => masked.clone(),
                    None => {
                        let joint_names: Vec<String> = skinned
                            .joints
                            .iter()
                            .map(|joint| self.names.get(*joint).map(|name| name.to_string()).unwrap_or_default())
                            .collect();
                        let made = mask(source, &joint_names, covered);
                        let masked = self.meshes.add(made);
                        self.masked.0.insert(key, masked.clone());
                        masked
                    }
                };
                self.commands.entity(entity).insert(Mesh3d(masked));
            }
            if let (Some(texture), Ok(material)) = (light.clone(), self.mesh_materials.get(entity))
                && let Some(mut skin) = self.materials.get(&material.0).cloned()
            {
                skin.base_color_texture = Some(texture);
                let skin = self.materials.add(skin);
                self.commands.entity(entity).insert(MeshMaterial3d(skin));
            }
        }
    }

    /// Tints a model's materials (hair colour).
    fn tint(&mut self, root: Entity, colour: Color) {
        for entity in self.descendants(root) {
            let Ok(material) = self.mesh_materials.get(entity) else { continue };
            let Some(mut tinted) = self.materials.get(&material.0).cloned() else { continue };
            tinted.base_color = colour;
            let tinted = self.materials.add(tinted);
            self.commands.entity(entity).insert(MeshMaterial3d(tinted));
        }
    }
}

/// Each entity under `root` with its path of names from `root` (the way the
/// glTF loader names animation targets: the root's own name first).
fn paths_from(
    root: Entity,
    named: &[(Entity, String)],
    children: &Query<&Children>,
    names: &Query<&Name>,
) -> Vec<(Entity, Vec<String>)> {
    let wanted: HashSet<Entity> = named.iter().map(|(entity, _)| *entity).collect();
    let mut paths = Vec::new();
    let Ok(root_name) = names.get(root) else { return paths };
    let mut stack = vec![(root, vec![root_name.as_str().to_string()])];
    while let Some((entity, path)) = stack.pop() {
        if let Ok(kids) = children.get(entity) {
            for child in kids.iter() {
                let Ok(name) = names.get(child) else { continue };
                let mut child_path = path.clone();
                child_path.push(name.as_str().to_string());
                stack.push((child, child_path));
            }
        }
        if wanted.contains(&entity) {
            paths.push((entity, path));
        }
    }
    paths
}

/// The body mesh without the triangles clothes cover: those whose corners
/// all belong mostly to bones in the covered parts of the body.
fn mask(source: &Mesh, joint_names: &[String], covered: &[BodyRegion]) -> Mesh {
    let hidden_bones: HashSet<&str> = covered.iter().flat_map(|region| region.bones().iter().copied()).collect();
    let hidden_joints: Vec<bool> = joint_names.iter().map(|name| hidden_bones.contains(name.as_str())).collect();
    let (Some(VertexAttributeValues::Uint16x4(joints)), Some(VertexAttributeValues::Float32x4(weights))) =
        (source.attribute(Mesh::ATTRIBUTE_JOINT_INDEX), source.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT))
    else {
        return source.clone();
    };
    // Whether each corner is hidden: its most-weighted bone is.
    let hidden: Vec<bool> = joints
        .iter()
        .zip(weights)
        .map(|(joint, weight)| {
            let strongest = (0..4).max_by(|a, b| weight[*a].total_cmp(&weight[*b])).unwrap_or(0);
            hidden_joints.get(joint[strongest] as usize).copied().unwrap_or(false)
        })
        .collect();
    let indices: Vec<u32> = match source.indices() {
        Some(indices) => indices.iter().map(|index| index as u32).collect(),
        None => (0..hidden.len() as u32).collect(),
    };
    let kept: Vec<u32> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|triangle| !triangle.iter().all(|&corner| hidden[corner as usize]))
        .flatten()
        .copied()
        .collect();
    let mut masked = source.clone();
    masked.insert_indices(Indices::U32(kept));
    masked
}

/// Removes placeholder bodies from people dressed in models (they come back
/// whenever a look changes: see [`crate::characters::model::rebuild_body`]).
fn remove_placeholders(
    mut commands: Commands,
    placeholders: Query<(Entity, &ChildOf), With<PlaceholderPart>>,
    dressed: Query<(), (With<Dressed>, Without<Assembling>)>,
) {
    for (part, parent) in &placeholders {
        if dressed.contains(parent.parent()) {
            commands.entity(part).despawn();
        }
    }
}

/// Plays the animation for what each person is doing.
fn animate_humans(
    time: Res<Time>,
    models: Res<HumanModels>,
    mut people: Query<(&Gait, &mut HumanRig)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut transforms: Query<&mut Transform>,
) {
    let Some((_, clips)) = &models.animations else { return };
    let now = time.elapsed_secs();
    for (gait, mut rig) in &mut people {
        let Ok((mut player, mut transitions)) = players.get_mut(rig.armature) else { continue };
        // Jumps lift the whole body.
        if let Ok(mut transform) = transforms.get_mut(rig.body_root) {
            let lift = gait.jump_amount().0;
            if transform.translation.y != lift {
                transform.translation.y = lift;
            }
        }
        let swinging = gait.swing_progress().is_some();
        let waving = gait.is_waving();
        // One-off animations start once, and play to the end.
        let mut one_off = None;
        if swinging && !rig.started_swing {
            one_off = Some(if gait.is_casting() {
                "cast"
            } else if rig.armed {
                "attack"
            } else {
                "punch"
            });
        } else if waving && !rig.started_wave {
            one_off = Some("wave");
        } else if rig.was_resting && !gait.resting {
            one_off = Some("stand_up");
        } else if gait.resting && !rig.was_resting {
            one_off = Some("sit_down");
        }
        rig.started_swing = swinging;
        rig.started_wave = waving;
        rig.was_resting = gait.resting;
        let (moving, speed) = (gait.speed > 0.2, gait.speed);
        if let Some(name) = one_off
            && let Some(&(node, _)) = clips.get(name)
        {
            transitions.play(&mut player, node, Duration::from_secs_f32(BLEND_SECONDS * 0.5));
            rig.playing = Some((name, now));
            continue;
        }
        // Let a one-off finish (unless the character starts moving).
        if let Some((name, since)) = rig.playing
            && matches!(name, "attack" | "punch" | "cast" | "wave" | "sit_down" | "stand_up")
            && clips.get(name).is_some_and(|&(_, length)| now - since < length)
            && !(moving && matches!(name, "wave" | "sit_down" | "stand_up"))
        {
            continue;
        }
        let (name, rate) = if gait.jump_amount().0 > 0.0 {
            ("jump", 1.0)
        } else if gait.resting {
            ("sitting", 1.0)
        } else if moving && speed < WALK_BELOW {
            ("walk", speed / WALK_ANIMATION_SPEED)
        } else if moving {
            ("run", speed / RUN_ANIMATION_SPEED)
        } else {
            ("idle", 1.0)
        };
        let Some(&(node, _)) = clips.get(name) else { continue };
        if rig.playing.map(|(playing, _)| playing) != Some(name) {
            transitions.play(&mut player, node, Duration::from_secs_f32(BLEND_SECONDS)).repeat();
            rig.playing = Some((name, now));
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_speed(rate.clamp(0.3, 2.5));
        }
    }
}
