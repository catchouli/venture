//! Bridges (see `docs/plan-bridges.md`), built by `venture_structures` from
//! the manifest's baked bridges (or, in the editor, the ones being drawn),
//! in the background, a mesh for each material of each bridge.

use std::collections::HashMap;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use venture_structures::MeshPart;
use venture_structures::bridges::{BridgeSurface, build_bridge};
use venture_world::BakedBridge;

use crate::content::ContentStore;
use crate::world::source::WorldInfo;
use crate::world::surfaces::{SurfaceTextures, textured};
use crate::world::walls::to_mesh;

/// Bridges are drawn up to this far away, in metres.
const DRAWN_UNTIL: f32 = 450.0;

/// Shows bridges.
pub struct BridgesPlugin;

impl Plugin for BridgesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShownBridges>()
            .init_resource::<Bridges>()
            .add_systems(Update, (load_materials, take_manifest_bridges, start_building, finish_building).chain());
    }
}

/// The bridges to show: the world's own, unless something else (the editor)
/// sets them (and says so with `authored`).
#[derive(Resource, Default)]
pub struct ShownBridges {
    /// The bridges.
    pub bridges: Vec<BakedBridge>,
    /// Whether they were set by something other than the world.
    pub authored: bool,
}

/// A bridge's built meshes: where they're drawn from, and each material's.
type Built = Vec<(Vec3, Vec<(BridgeSurface, MeshPart)>)>;

/// The bridges' materials, and what's shown.
#[derive(Resource, Default)]
struct Bridges {
    materials: Option<HashMap<BridgeSurface, Handle<StandardMaterial>>>,
    shown: Vec<Entity>,
    building: Option<Task<Built>>,
    dirty: bool,
}

/// A bridge (the parent of its meshes).
#[derive(Component)]
pub struct Bridge;

fn load_materials(
    content: Res<ContentStore>,
    asset_server: Res<AssetServer>,
    mut textures: ResMut<SurfaceTextures>,
    mut bridges: ResMut<Bridges>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if bridges.materials.is_some() || content.version.is_none() {
        return;
    }
    let mut made = HashMap::new();
    made.insert(BridgeSurface::Stone, materials.add(textured(textures.get("drystone", &asset_server), 1.6)));
    made.insert(BridgeSurface::Wood, materials.add(textured(textures.get("fence_wood", &asset_server), 1.2)));
    made.insert(
        BridgeSurface::Rope,
        materials.add(StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.1, ..default() }),
    );
    bridges.materials = Some(made);
}

fn take_manifest_bridges(world: Option<Res<WorldInfo>>, mut shown: ResMut<ShownBridges>) {
    if let Some(world) = world
        && world.is_changed()
        && !shown.authored
    {
        shown.bridges = world.0.bridges.clone();
    }
}

/// Builds every bridge again, in the background, when they change (one build at a time).
fn start_building(shown: Res<ShownBridges>, mut bridges: ResMut<Bridges>) {
    if shown.is_changed() {
        bridges.dirty = true;
    }
    if !bridges.dirty || bridges.building.is_some() {
        return;
    }
    bridges.dirty = false;
    let all = shown.bridges.clone();
    bridges.building = Some(AsyncComputeTaskPool::get().spawn(async move {
        all.iter()
            .filter(|bridge| !bridge.deck.is_empty())
            .map(|bridge| {
                // (Drawn from its first point, so it's measured from there for distance and precision.)
                let [x, y, height] = bridge.deck[0];
                let origin = Vec3::new(x, height, y);
                let parts = build_bridge(bridge)
                    .parts
                    .into_iter()
                    .map(|(surface, mut part)| {
                        for position in &mut part.positions {
                            *position = (Vec3::from(*position) - origin).to_array();
                        }
                        (surface, part)
                    })
                    .collect();
                (origin, parts)
            })
            .collect()
    }));
}

fn finish_building(mut commands: Commands, mut bridges: ResMut<Bridges>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(materials) = bridges.materials.clone() else { return };
    let Some(task) = bridges.building.as_mut() else { return };
    let Some(built) = check_ready(task) else { return };
    bridges.building = None;
    for entity in std::mem::take(&mut bridges.shown) {
        commands.entity(entity).despawn();
    }
    let range =
        VisibilityRange { start_margin: 0.0..0.0, end_margin: DRAWN_UNTIL..DRAWN_UNTIL + 10.0, use_aabb: false };
    for (origin, parts) in built {
        let parent = commands
            .spawn((Bridge, Transform::from_translation(origin), Visibility::default()))
            .with_children(|parent| {
                for (surface, part) in &parts {
                    let Some(material) = materials.get(surface) else { continue };
                    parent.spawn((Mesh3d(meshes.add(to_mesh(part))), MeshMaterial3d(material.clone()), range.clone()));
                }
            })
            .id();
        bridges.shown.push(parent);
    }
}
