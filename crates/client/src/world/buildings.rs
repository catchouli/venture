//! Buildings (see `docs/plan-buildings.md`): built by the building
//! generator (`venture_structures::buildings`) from the baked buildings in
//! the world's manifest (or the editor's), in their styles from the content.
//!
//! - **Building:** whenever the buildings shown ([`ShownBuildings`]) or the
//!   styles change, the buildings that changed are built again in the
//!   background.
//! - **Levels of detail:** every stone and board close up; flat walls with
//!   dark windows under a plain roof far away, crossfading between them.
//! - **Materials:** a material a surface (stone, boards, wood, thatch,
//!   plaster, floor, glass), the textured ones from the content's
//!   `textures/` (see `surfaces.rs`).
//! - **Inside:** while the player (or the camera) is inside a building, its
//!   upper parts (the roof, the floors above and the chimney) hide, so the
//!   room can be seen, and the camera never looks at a roof from inside.

use std::collections::HashMap;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use venture_structures::buildings::{BuildingMeshes, BuildingStyle, Detail, Surface, build_building, plan::contains};
use venture_world::BakedBuilding;

use crate::characters::player::LocalPlayer;
use crate::common::coords::from_bevy;
use crate::content::ContentStore;
use crate::world::source::WorldInfo;
use crate::world::surfaces::{SurfaceTextures, textured};
use crate::world::walls::to_mesh;

/// Buildings are every stone up to this far from the camera, in metres...
const FULL_UNTIL: f32 = 80.0;
/// ...crossfading to (and from) their far look over this distance...
const CROSSFADE: f32 = 10.0;
/// ...and aren't drawn at all beyond this.
const DRAWN_UNTIL: f32 = 600.0;
/// The textured surfaces: their texture, and how many metres it covers.
const TEXTURED: [(Surface, &str, f32); 5] = [
    (Surface::Stone, "drystone", 1.6),
    (Surface::Boards, "boards", 1.6),
    (Surface::Wood, "fence_wood", 1.0),
    (Surface::Thatch, "thatch", 1.6),
    (Surface::Floor, "fence_wood", 1.2),
];

/// Builds and shows buildings.
pub struct BuildingsPlugin;

impl Plugin for BuildingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Buildings>().init_resource::<ShownBuildings>().add_systems(
            Update,
            (make_materials, take_manifest_buildings, start_building, finish_building, show_inside).chain(),
        );
    }
}

/// The buildings to show.
#[derive(Resource, Default)]
pub struct ShownBuildings {
    /// The buildings.
    pub buildings: Vec<BakedBuilding>,
    /// Whether something else sets them (the editor), rather than their
    /// being the world's own, from its manifest.
    pub authored: bool,
}

/// What a building was built from: itself and its style.
type Source = (BakedBuilding, Option<BuildingStyle>);

/// The buildings: their materials, and what's built.
#[derive(Resource, Default)]
struct Buildings {
    materials: Option<HashMap<Surface, Handle<StandardMaterial>>>,
    /// Each building shown, by its id: what it was built from, and its root.
    built: HashMap<String, (Source, Entity)>,
    /// Building, if under way: every building's id (to know which have gone), and the ones built again.
    building: Option<Task<(Vec<String>, Vec<(String, Source, Option<BuildingMeshes>)>)>>,
    /// Whether the buildings (or styles) changed while others were being built.
    dirty: bool,
}

/// A building (the parent of its meshes), with its outline (for telling
/// when the player's inside).
#[derive(Component)]
pub struct BuildingRoot {
    /// Its outline, in the world.
    pub outline: Vec<Vec2>,
}

/// A building's mesh above its ground floor (hidden while the player's inside).
#[derive(Component)]
struct UpperPart;

/// Makes a material for each surface, once the content (which has the textures) is here.
fn make_materials(
    content: Res<ContentStore>,
    asset_server: Res<AssetServer>,
    mut textures: ResMut<SurfaceTextures>,
    mut buildings: ResMut<Buildings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if buildings.materials.is_some() || content.version.is_none() {
        return;
    }
    let mut made = HashMap::new();
    for (surface, texture, metres) in TEXTURED {
        let mut material = textured(textures.get(texture, &asset_server), metres);
        if surface == Surface::Thatch {
            material.reflectance = 0.15;
        }
        made.insert(surface, materials.add(material));
    }
    made.insert(
        Surface::Plaster,
        materials.add(StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.2, ..default() }),
    );
    made.insert(
        Surface::Dark,
        materials.add(StandardMaterial { perceptual_roughness: 0.15, reflectance: 0.5, ..default() }),
    );
    buildings.materials = Some(made);
}

/// Shows the world's own buildings, unless something else sets them.
fn take_manifest_buildings(world: Option<Res<WorldInfo>>, mut shown: ResMut<ShownBuildings>) {
    if let Some(world) = world
        && world.is_changed()
        && !shown.authored
    {
        shown.buildings = world.0.buildings.clone();
    }
}

/// Builds the buildings that changed (or whose style did) in the background, one build at a time.
fn start_building(shown: Res<ShownBuildings>, content: Res<ContentStore>, mut buildings: ResMut<Buildings>) {
    if shown.is_changed() || content.is_changed() {
        buildings.dirty = true;
    }
    if !buildings.dirty || buildings.building.is_some() || content.version.is_none() {
        return;
    }
    buildings.dirty = false;
    let ids: Vec<String> = shown.buildings.iter().map(|building| building.plan.id.clone()).collect();
    let changed: Vec<(String, Source)> = shown
        .buildings
        .iter()
        .map(|building| {
            let style = content.building_styles.iter().find(|style| style.id == building.plan.style).cloned();
            (building.plan.id.clone(), (building.clone(), style))
        })
        .filter(|(id, source)| buildings.built.get(id).is_none_or(|(built, _)| built != source))
        .collect();
    if changed.is_empty() && buildings.built.keys().all(|id| ids.contains(id)) {
        return;
    }
    buildings.building = Some(AsyncComputeTaskPool::get().spawn(async move {
        let built = changed
            .into_iter()
            .map(|(id, source)| {
                let meshes = source.1.as_ref().and_then(|style| build_building(&source.0, style));
                (id, source, meshes)
            })
            .collect();
        (ids, built)
    }));
}

/// Shows the buildings once they're built (and the materials exist), and
/// takes down the ones they replace or that have gone.
fn finish_building(mut commands: Commands, mut buildings: ResMut<Buildings>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(materials) = buildings.materials.clone() else { return };
    let Some(task) = buildings.building.as_mut() else { return };
    let Some((ids, built)) = check_ready(task) else { return };
    buildings.building = None;
    let gone: Vec<String> = buildings
        .built
        .keys()
        .filter(|id| !ids.contains(id) || built.iter().any(|(rebuilt, ..)| rebuilt == *id))
        .cloned()
        .collect();
    for id in gone {
        if let Some((_, root)) = buildings.built.remove(&id) {
            commands.entity(root).despawn();
        }
    }
    let range = |from: f32, to: f32| VisibilityRange {
        start_margin: from..if from > 0.0 { from + CROSSFADE } else { 0.0 },
        end_margin: to..to + CROSSFADE,
        use_aabb: false,
    };
    for (id, source, built) in built {
        let outline = source.0.plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        let Some(built) = built else {
            warn!("The building \"{id}\" can't be built (see the content check, or its plan).");
            let root = commands.spawn((BuildingRoot { outline }, Transform::default(), Visibility::default())).id();
            buildings.built.insert(id, (source, root));
            continue;
        };
        let [x, height, y] = built.origin;
        let root = commands
            .spawn((
                Name::new(format!("Building {id}")),
                BuildingRoot { outline },
                Transform::from_xyz(x, height, y),
                Visibility::default(),
            ))
            .id();
        for part in built.parts {
            let Some(material) = materials.get(&part.surface) else { continue };
            let range = match part.detail {
                Detail::Full => range(0.0, FULL_UNTIL),
                Detail::Far => range(FULL_UNTIL, DRAWN_UNTIL),
            };
            let mut child = commands.spawn((
                Mesh3d(meshes.add(to_mesh(&part.mesh))),
                MeshMaterial3d(material.clone()),
                range,
                ChildOf(root),
            ));
            if part.upper {
                child.insert(UpperPart);
            }
        }
        buildings.built.insert(id, (source, root));
    }
}

/// Hides a building's upper parts while the player (or the camera) is
/// inside it, so the camera never looks at the inside of a roof.
fn show_inside(
    player: Query<&Transform, With<LocalPlayer>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    roots: Query<(&BuildingRoot, &Children)>,
    mut uppers: Query<&mut Visibility, With<UpperPart>>,
) {
    let mut inside_at: Vec<Vec2> =
        player.iter().map(|transform| Vec2::from(from_bevy(transform.translation))).collect();
    inside_at.extend(
        cameras
            .iter()
            .filter(|(camera, _)| camera.is_active)
            .map(|(_, place)| Vec2::from(from_bevy(place.translation()))),
    );
    for (root, children) in &roots {
        let inside = inside_at.iter().any(|&at| contains(&root.outline, at));
        let wanted = if inside { Visibility::Hidden } else { Visibility::Inherited };
        for child in children {
            if let Ok(mut visibility) = uppers.get_mut(*child) {
                visibility.set_if_neq(wanted);
            }
        }
    }
}
