//! Loading chunks around a point of interest and unloading distant ones.
//!
//! Streaming happens in two steps, both on Bevy's async task pools (on the
//! web, tasks run on the main thread between frames):
//! 1. **Data**: baked chunk files are downloaded from the server (or read from
//!    disk in `--local` mode), decoded, and kept in the [`ChunkStore`].
//! 2. **Meshes**: once a chunk *and its eight neighbours* have data, its
//!    meshes are built, so shading and colours blend across chunk borders.
//!
//! Data is therefore kept one chunk further out than meshes are shown.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task, futures::check_ready};
use venture_navigation::{ChunkObstacles, WalkingLimits};
use venture_world::{Chunk, ChunkPos, FluidKind, decode_chunk};

use crate::coords::to_bevy;
use crate::palette;
use crate::prop_mesh::{PropTemplates, props_mesh};
use crate::terrain_material::{TerrainExtension, TerrainMaterial};
use crate::terrain_mesh::{Neighbourhood, fluid_mesh, terrain_mesh};
use crate::world_source::{WorldInfo, WorldSource};

/// Chunks within this many chunks of the anchor are shown.
const VIEW_RADIUS: i32 = 4;

/// Chunk data is kept this far out: one further than shown, for meshing neighbours.
const DATA_RADIUS: i32 = VIEW_RADIUS + 1;

/// Chunks are dropped once they are this much further out than they need to
/// be, so they don't flicker in and out at the boundary.
const UNLOAD_SLACK: i32 = 1;

/// At most this many new tasks are started per frame. The web has no
/// background threads, so work there must be spread thinly across frames.
const MAX_TASKS_STARTED_PER_FRAME: usize = if cfg!(target_arch = "wasm32") { 1 } else { 4 };

/// Streams terrain chunks around the [`StreamingFocus`].
pub struct ChunkStreamingPlugin;

impl Plugin for ChunkStreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreamingFocus>()
            .init_resource::<ChunkStore>()
            .init_resource::<ObstacleStore>()
            .init_resource::<ShownChunks>()
            .init_resource::<StreamingProgress>()
            .init_resource::<DataTasks>()
            .add_systems(Startup, create_assets)
            .add_systems(
                Update,
                (
                    unload_distant_chunks,
                    start_data_tasks,
                    finish_data_tasks,
                    start_mesh_tasks,
                    finish_mesh_tasks,
                    measure_progress,
                )
                    .chain()
                    .run_if(resource_exists::<WorldInfo>),
            );
    }
}

/// The point chunks are loaded around (usually what the camera looks at).
#[derive(Resource, Default)]
pub struct StreamingFocus(pub Vec3);

/// The data of every loaded chunk.
#[derive(Resource, Default)]
pub struct ChunkStore(pub HashMap<ChunkPos, Arc<Chunk>>);

/// Where characters can't walk, for every loaded chunk.
#[derive(Resource, Default)]
pub struct ObstacleStore(pub HashMap<ChunkPos, Arc<ChunkObstacles>>);

/// The entity showing each visible chunk (whether its meshes are built yet or not).
#[derive(Resource, Default)]
pub struct ShownChunks(pub HashMap<ChunkPos, Entity>);

/// How many visible chunks are still waiting for their data or meshes.
#[derive(Resource, Default)]
pub struct StreamingProgress {
    /// Whether `waiting` has been measured yet.
    pub measured: bool,
    pub waiting: usize,
}

/// Seconds to wait before trying a chunk again after it failed to load.
const RETRY_SECONDS: f32 = 2.0;

/// Chunks whose data is being loaded.
#[derive(Resource, Default)]
struct DataTasks {
    running: HashMap<ChunkPos, Task<Result<(Chunk, ChunkObstacles), String>>>,
    /// Chunks that failed to load, and when they may be tried again (in seconds since startup).
    retry_at: HashMap<ChunkPos, f32>,
}

/// A chunk entity whose meshes are still being built.
#[derive(Component)]
pub struct MeshTask(Task<ChunkMeshes>);

/// The meshes for one chunk.
pub struct ChunkMeshes {
    terrain: Mesh,
    water: Option<Mesh>,
    lava: Option<Mesh>,
    props: Option<Mesh>,
}

/// Assets shared by every chunk.
#[derive(Resource)]
struct ChunkAssets {
    terrain: Handle<TerrainMaterial>,
    water: Handle<StandardMaterial>,
    lava: Handle<StandardMaterial>,
    props: Handle<StandardMaterial>,
    prop_templates: Arc<PropTemplates>,
}

fn create_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
) {
    commands.insert_resource(ChunkAssets {
        // White, so the vertex colours show through; the shader adds the detail.
        terrain: terrain_materials.add(TerrainMaterial {
            base: StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.15, ..default() },
            extension: TerrainExtension::default(),
        }),
        water: materials.add(StandardMaterial {
            base_color: palette::WATER,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.35,
            reflectance: 0.3,
            ..default()
        }),
        lava: materials.add(StandardMaterial {
            base_color: palette::LAVA,
            emissive: palette::LAVA_GLOW,
            perceptual_roughness: 0.6,
            ..default()
        }),
        props: materials.add(StandardMaterial { perceptual_roughness: 0.9, reflectance: 0.2, ..default() }),
        prop_templates: Arc::new(PropTemplates::new()),
    });
}

/// The chunk the focus is in.
fn focus_chunk(focus: &StreamingFocus) -> ChunkPos {
    venture_world::CellPos::containing(focus.0.x, focus.0.z).chunk()
}

/// Distance between chunks in chunks, measured as the larger axis difference.
fn chunk_distance(a: ChunkPos, b: ChunkPos) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Chunk positions within `radius` of `centre`, closest first.
fn chunks_around(centre: ChunkPos, radius: i32) -> Vec<ChunkPos> {
    let mut positions: Vec<ChunkPos> = (-radius..=radius)
        .flat_map(|dy| (-radius..=radius).map(move |dx| ChunkPos::new(centre.x + dx, centre.y + dy)))
        .collect();
    positions.sort_by_key(|pos| chunk_distance(*pos, centre));
    positions
}

fn unload_distant_chunks(
    mut commands: Commands,
    mut store: ResMut<ChunkStore>,
    mut obstacles: ResMut<ObstacleStore>,
    mut shown: ResMut<ShownChunks>,
    mut data_tasks: ResMut<DataTasks>,
    focus: Res<StreamingFocus>,
) {
    let centre = focus_chunk(&focus);
    let keep_data = |pos: &ChunkPos| chunk_distance(*pos, centre) <= DATA_RADIUS + UNLOAD_SLACK;
    store.0.retain(|pos, _| keep_data(pos));
    obstacles.0.retain(|pos, _| keep_data(pos));
    // Dropping a task cancels it.
    data_tasks.running.retain(|pos, _| keep_data(pos));
    data_tasks.retry_at.retain(|pos, _| keep_data(pos));
    shown.0.retain(|&pos, &mut entity| {
        let keep = chunk_distance(pos, centre) <= VIEW_RADIUS + UNLOAD_SLACK;
        if !keep {
            commands.entity(entity).despawn();
        }
        keep
    });
}

fn start_data_tasks(
    time: Res<Time>,
    mut data_tasks: ResMut<DataTasks>,
    store: Res<ChunkStore>,
    world: Res<WorldInfo>,
    source: Res<WorldSource>,
    focus: Res<StreamingFocus>,
) {
    let pool = IoTaskPool::get();
    let missing: Vec<ChunkPos> = chunks_around(focus_chunk(&focus), DATA_RADIUS)
        .into_iter()
        .filter(|pos| {
            world.0.contains_chunk(*pos) && !store.0.contains_key(pos) && !data_tasks.running.contains_key(pos)
        })
        .filter(|pos| data_tasks.retry_at.get(pos).is_none_or(|&at| time.elapsed_secs() >= at))
        .take(MAX_TASKS_STARTED_PER_FRAME)
        .collect();
    for pos in missing {
        let read = source.read_chunk(pos);
        data_tasks.running.insert(
            pos,
            pool.spawn(async move {
                let chunk = decode_chunk(&read.await?).map_err(|error| error.to_string())?;
                let obstacles = ChunkObstacles::from_chunk(&chunk, &WalkingLimits::default());
                Ok((chunk, obstacles))
            }),
        );
    }
}

fn finish_data_tasks(
    time: Res<Time>,
    mut data_tasks: ResMut<DataTasks>,
    mut store: ResMut<ChunkStore>,
    mut obstacle_store: ResMut<ObstacleStore>,
) {
    let DataTasks { running, retry_at } = &mut *data_tasks;
    running.retain(|&pos, task| match check_ready(task) {
        Some(Ok((chunk, obstacles))) => {
            store.0.insert(pos, Arc::new(chunk));
            obstacle_store.0.insert(pos, Arc::new(obstacles));
            retry_at.remove(&pos);
            false
        }
        Some(Err(error)) => {
            warn!("Couldn't load chunk ({}, {}): {error}", pos.x, pos.y);
            retry_at.insert(pos, time.elapsed_secs() + RETRY_SECONDS);
            false
        }
        None => true,
    });
}

fn start_mesh_tasks(
    mut commands: Commands,
    mut shown: ResMut<ShownChunks>,
    store: Res<ChunkStore>,
    world: Res<WorldInfo>,
    assets: Res<ChunkAssets>,
    focus: Res<StreamingFocus>,
) {
    // A neighbour outside the world never gets data, so it doesn't count as missing.
    let has_data = |pos: ChunkPos| store.0.contains_key(&pos) || !world.0.contains_chunk(pos);
    let ready = chunks_around(focus_chunk(&focus), VIEW_RADIUS).into_iter().filter(|&pos| {
        store.0.contains_key(&pos) && !shown.0.contains_key(&pos) && chunks_around(pos, 1).into_iter().all(has_data)
    });

    let pool = AsyncComputeTaskPool::get();
    for pos in ready.take(MAX_TASKS_STARTED_PER_FRAME).collect::<Vec<_>>() {
        let area = Neighbourhood::collect(pos, |p| store.0.get(&p).cloned());
        let templates = assets.prop_templates.clone();
        let task = pool.spawn(async move {
            let chunk = area.centre();
            ChunkMeshes {
                terrain: terrain_mesh(&area),
                water: fluid_mesh(&area, FluidKind::Water),
                lava: fluid_mesh(&area, FluidKind::Lava),
                props: props_mesh(chunk, &templates),
            }
        });
        let (x, y) = pos.origin_cell();
        let entity = commands
            .spawn((
                Name::new(format!("Chunk ({}, {})", pos.x, pos.y)),
                MeshTask(task),
                Transform::from_translation(to_bevy(x as f32, y as f32, 0.0)),
                Visibility::default(),
            ))
            .id();
        shown.0.insert(pos, entity);
    }
}

fn finish_mesh_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut MeshTask)>,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<ChunkAssets>,
) {
    for (entity, mut task) in &mut tasks {
        let Some(built) = check_ready(&mut task.0) else { continue };
        let mut chunk = commands.entity(entity);
        chunk.remove::<MeshTask>().insert((Mesh3d(meshes.add(built.terrain)), MeshMaterial3d(assets.terrain.clone())));
        let parts = [(built.water, &assets.water), (built.lava, &assets.lava), (built.props, &assets.props)];
        for (mesh, material) in parts {
            if let Some(mesh) = mesh {
                chunk.with_child((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material.clone())));
            }
        }
    }
}

fn measure_progress(
    mut progress: ResMut<StreamingProgress>,
    shown: Res<ShownChunks>,
    world: Res<WorldInfo>,
    building: Query<(), With<MeshTask>>,
    focus: Res<StreamingFocus>,
) {
    let not_started = chunks_around(focus_chunk(&focus), VIEW_RADIUS)
        .into_iter()
        .filter(|pos| world.0.contains_chunk(*pos) && !shown.0.contains_key(pos))
        .count();
    progress.waiting = not_started + building.iter().count();
    progress.measured = true;
}
