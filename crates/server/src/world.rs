//! The baked world, as the server needs it: which cells can be walked on,
//! which are water (of what body, in what biome: for fishing), and where the
//! trees are (for woodcutting).
//!
//! When the world is baked again (its manifest changes, and stays unchanged
//! for a moment), it's loaded again in the background and swapped in: the
//! chunks that changed are announced as [`WorldReloaded`] (woodcutting
//! forgets felled trees in them), and players are told
//! ([`ServerMessage::WorldChanged`]), so their clients load them again.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::SystemTime;

use bevy::prelude::*;
use venture_navigation::{ChunkObstacles, WalkingLimits};
use venture_protocol::{ServerMessage, TreeId};
use venture_world::{
    Biome, CHUNK_SIZE, CellPos, Chunk, ChunkPos, FluidBody, FluidKind, MANIFEST_FILE, PropKind, WorldManifest,
    chunk_file_path, decode_chunk,
};

/// The loaded world.
#[derive(Resource)]
pub struct ServerWorld {
    /// The world's description.
    pub manifest: WorldManifest,
    /// Blocked cells for every chunk, indexed `y * chunks_per_side + x`.
    obstacles: Vec<ChunkObstacles>,
    /// Each cell's water, for every chunk (see [`Waters`]), in the same order.
    waters: Vec<Waters>,
    /// Each chunk's trees (bushes too: they're small trees), by their place among its props, in the same order.
    trees: Vec<Vec<WorldTree>>,
    /// A hash of each chunk's file, in the same order (to tell which changed when it's loaded again).
    hashes: Vec<u64>,
    /// The folder it was loaded from, and when its manifest was last
    /// changed then (none for a world made in memory, which isn't watched).
    source: Option<(PathBuf, Option<SystemTime>)>,
    /// Every chunk, for a world made in memory (one loaded from a folder
    /// reads its chunks from there as they're needed).
    in_memory: Vec<Chunk>,
}

/// A hash of a chunk file's bytes.
fn hash_of(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// When a world folder's manifest was last changed.
fn manifest_changed_at(folder: &Path) -> Option<SystemTime> {
    std::fs::metadata(folder.join(MANIFEST_FILE)).and_then(|meta| meta.modified()).ok()
}

/// A tree standing in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTree {
    /// Its place among its chunk's props.
    pub index: u16,
    /// Where it stands, in metres.
    pub at: (f32, f32),
    /// Its species: an index into the manifest's `tree_species`.
    pub species: u16,
    /// Young, mature or old (0, 1 or 2).
    pub variant: u8,
}

/// A chunk's trees (props that are trees, bushes included).
fn trees_in(chunk: &Chunk) -> Vec<WorldTree> {
    chunk
        .props
        .iter()
        .enumerate()
        .filter_map(|(index, prop)| match prop.kind {
            PropKind::Tree { species, variant } => {
                Some(WorldTree { index: index as u16, at: (prop.x, prop.y), species, variant })
            }
            _ => None,
        })
        .collect()
}

/// A chunk's water: a byte a cell, `y * CHUNK_SIZE + x`: 0 for none, or the
/// body of water (1 river, 2 lake, 3 sea) plus 4 times the biome's index.
struct Waters(Box<[u8]>);

impl Waters {
    fn from_chunk(chunk: &Chunk) -> Self {
        let size = CHUNK_SIZE as usize;
        let cells = (0..size * size)
            .map(|index| {
                let cell = chunk.cell(index % size, index / size);
                let Some(fluid) = cell.fluid.filter(|fluid| fluid.kind == FluidKind::Water) else { return 0 };
                let body = match fluid.body {
                    FluidBody::River => 1,
                    FluidBody::Lake => 2,
                    FluidBody::Sea => 3,
                };
                body + 4 * cell.biome.index() as u8
            })
            .collect();
        Self(cells)
    }
}

impl ServerWorld {
    /// Loads a baked world folder, reading every chunk (using all CPU cores).
    pub fn load(folder: &Path) -> Result<Self, String> {
        let changed_at = manifest_changed_at(folder);
        let manifest_path = folder.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "can't read {} ({error}). Bake a world first: cargo run --release -p venture_worldgen --bin bake",
                manifest_path.display()
            )
        })?;
        let manifest = WorldManifest::from_ron(&text).map_err(|error| error.to_string())?;

        let chunks = manifest.chunks_per_side();
        let positions: Vec<ChunkPos> = (0..chunks * chunks).map(|i| ChunkPos::new(i % chunks, i / chunks)).collect();
        let threads = std::thread::available_parallelism().map_or(4, |count| count.get());
        let batch = positions.len().div_ceil(threads);
        let limits = WalkingLimits::default();

        let loaded = std::thread::scope(|scope| {
            let workers: Vec<_> = positions
                .chunks(batch)
                .map(|batch| {
                    let limits = &limits;
                    scope.spawn(move || {
                        batch
                            .iter()
                            .map(|&pos| {
                                let path = folder.join(chunk_file_path(pos));
                                let bytes = std::fs::read(&path)
                                    .map_err(|error| format!("can't read {}: {error}", path.display()))?;
                                let chunk =
                                    decode_chunk(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
                                Ok((
                                    ChunkObstacles::from_chunk(&chunk, limits),
                                    Waters::from_chunk(&chunk),
                                    trees_in(&chunk),
                                    hash_of(&bytes),
                                ))
                            })
                            .collect::<Result<Vec<_>, String>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("a loading thread panicked"))
                .collect::<Result<Vec<_>, String>>()
        })?;

        let (mut obstacles, mut waters, mut trees, mut hashes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (chunk_obstacles, chunk_waters, chunk_trees, hash) in loaded.into_iter().flatten() {
            obstacles.push(chunk_obstacles);
            waters.push(chunk_waters);
            trees.push(chunk_trees);
            hashes.push(hash);
        }
        let source = Some((folder.to_path_buf(), changed_at));
        Ok(Self { manifest, obstacles, waters, trees, hashes, source, in_memory: Vec::new() })
    }

    /// A world made from chunks already in memory (for tests and tools).
    /// `chunks` must hold every chunk in the manifest, in row order.
    pub fn from_chunks(manifest: WorldManifest, chunks: &[Chunk]) -> Self {
        assert_eq!(chunks.len(), manifest.chunks_per_side().pow(2) as usize, "one chunk per world chunk");
        let limits = WalkingLimits::default();
        let obstacles = chunks.iter().map(|chunk| ChunkObstacles::from_chunk(chunk, &limits)).collect();
        let waters = chunks.iter().map(Waters::from_chunk).collect();
        let trees = chunks.iter().map(trees_in).collect();
        let hashes = chunks.iter().map(|chunk| hash_of(&venture_world::encode_chunk(chunk))).collect();
        Self { manifest, obstacles, waters, trees, hashes, source: None, in_memory: chunks.to_vec() }
    }

    /// A chunk of the world, in full (read from the world's folder, for a
    /// world loaded from one), or `None` if it isn't in the world or can't be read.
    pub fn chunk(&self, pos: ChunkPos) -> Option<Chunk> {
        if !self.manifest.contains_chunk(pos) {
            return None;
        }
        let Some((folder, _)) = &self.source else {
            return self.in_memory.get((pos.y * self.manifest.chunks_per_side() + pos.x) as usize).cloned();
        };
        let path = folder.join(chunk_file_path(pos));
        match std::fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| decode_chunk(&bytes).map_err(|error| error.to_string()))
        {
            Ok(chunk) => Some(chunk),
            Err(error) => {
                warn!("Couldn't read {}: {error}", path.display());
                None
            }
        }
    }

    /// The chunks that differ between this world and `newer` (all of them,
    /// if they're different sizes).
    pub fn changed_chunks(&self, newer: &ServerWorld) -> Vec<ChunkPos> {
        let side = newer.manifest.chunks_per_side();
        (0..side * side)
            .filter(|&i| self.hashes.len() != newer.hashes.len() || self.hashes[i as usize] != newer.hashes[i as usize])
            .map(|i| ChunkPos::new(i % side, i / side))
            .collect()
    }

    /// Whether cell `(x, y)` can't be walked on. Everything outside the world is blocked.
    pub fn is_blocked(&self, x: i32, y: i32) -> bool {
        let cell = CellPos::new(x, y);
        let chunk = cell.chunk();
        if !self.manifest.contains_chunk(chunk) {
            return true;
        }
        let (local_x, local_y) = cell.local();
        self.obstacles[(chunk.y * self.manifest.chunks_per_side() + chunk.x) as usize].is_blocked(local_x, local_y)
    }

    /// The water at a point (in metres), if it's in water: its body, and the
    /// biome it's in.
    pub fn water_at(&self, (x, y): (f32, f32)) -> Option<(FluidBody, Biome)> {
        let cell = CellPos::new(x.floor() as i32, y.floor() as i32);
        let chunk = cell.chunk();
        if !self.contains((x, y)) || !self.manifest.contains_chunk(chunk) {
            return None;
        }
        let (local_x, local_y) = cell.local();
        let waters = &self.waters[(chunk.y * self.manifest.chunks_per_side() + chunk.x) as usize];
        let byte = waters.0[local_y * CHUNK_SIZE as usize + local_x];
        let body = match byte % 4 {
            1 => FluidBody::River,
            2 => FluidBody::Lake,
            3 => FluidBody::Sea,
            _ => return None,
        };
        Some((body, Biome::ALL[(byte / 4) as usize % Biome::COUNT]))
    }

    /// A standing tree (or bush), if there's one with this id.
    pub fn tree(&self, id: TreeId) -> Option<WorldTree> {
        let chunk = ChunkPos::new(id.chunk_x, id.chunk_y);
        if !self.manifest.contains_chunk(chunk) {
            return None;
        }
        let trees = &self.trees[(chunk.y * self.manifest.chunks_per_side() + chunk.x) as usize];
        trees.binary_search_by_key(&id.index, |tree| tree.index).ok().map(|found| trees[found])
    }

    /// A tree species' id (in the content), by its index in the manifest.
    pub fn species_id(&self, species: u16) -> Option<&str> {
        self.manifest.tree_species.get(usize::from(species)).map(String::as_str)
    }

    /// Whether a point (in metres) is inside the world.
    pub fn contains(&self, (x, y): (f32, f32)) -> bool {
        let size = self.manifest.world_size as f32;
        (0.0..size).contains(&x) && (0.0..size).contains(&y)
    }

    /// The nearest point to `at` that can be stood on, at most `reach`
    /// metres away (`at` itself if it's open; otherwise the middle of the
    /// nearest open cell), or `None` if there's none.
    pub fn open_ground_near(&self, at: (f32, f32), reach: f32) -> Option<(f32, f32)> {
        let open = |(x, y): (f32, f32)| self.contains((x, y)) && !self.is_blocked(x.floor() as i32, y.floor() as i32);
        if open(at) {
            return Some(at);
        }
        let cells = reach.ceil() as i32;
        let (centre_x, centre_y) = (at.0.floor() as i32, at.1.floor() as i32);
        let distance = |point: (f32, f32)| (point.0 - at.0).hypot(point.1 - at.1);
        (-cells..=cells)
            .flat_map(|dy| (-cells..=cells).map(move |dx| (centre_x + dx, centre_y + dy)))
            .map(|(x, y)| (x as f32 + 0.5, y as f32 + 0.5))
            .filter(|&point| distance(point) <= reach && open(point))
            .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
    }
}

/// Loads the world again when it's baked again.
pub struct WorldReloadPlugin;

impl Plugin for WorldReloadPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WorldReloaded>()
            .init_resource::<WorldWatch>()
            .add_systems(FixedUpdate, reload_when_baked.in_set(crate::ServerSystems::Sessions));
    }
}

/// The world has been loaded again: these chunks changed.
#[derive(Message, Clone, Debug)]
pub struct WorldReloaded {
    /// The chunks that changed.
    pub changed: Vec<ChunkPos>,
}

/// How often the manifest is looked at, and how long it must stay unchanged
/// before the world is loaded again (so a bake has finished), in seconds.
const WATCH_EVERY: f32 = 1.0;
const SETTLE_SECONDS: f32 = 1.0;

/// Watching the world's manifest for a new bake.
#[derive(Resource, Default)]
struct WorldWatch {
    /// When it was last looked at (seconds of game time).
    looked_at: f32,
    /// When the manifest was seen to change, and when it was changed then.
    seen: Option<(f32, Option<SystemTime>)>,
    /// Loading the world again, in the background.
    loading: Option<JoinHandle<Result<ServerWorld, String>>>,
}

/// Loads the world again, in the background, once a new bake has settled,
/// and swaps it in.
fn reload_when_baked(
    time: Res<Time>,
    mut watch: ResMut<WorldWatch>,
    mut world: ResMut<ServerWorld>,
    mut reloaded: MessageWriter<WorldReloaded>,
    mut link: ResMut<crate::link::Link>,
    players: Query<&crate::entities::Player>,
) {
    let now = time.elapsed_secs();
    if let Some(loading) = watch.loading.take_if(|loading| loading.is_finished()) {
        match loading.join() {
            Ok(Ok(newer)) => {
                let changed = world.changed_chunks(&newer);
                info!("The world was baked again ({} chunks changed); loaded it.", changed.len());
                *world = newer;
                let chunks = changed.iter().map(|pos| (pos.x, pos.y)).collect();
                link.broadcast(players.iter().map(|player| player.connection), &ServerMessage::WorldChanged { chunks });
                reloaded.write(WorldReloaded { changed });
            }
            Ok(Err(error)) => {
                warn!("The world was baked again, but couldn't be loaded ({error}); keeping the old one.")
            }
            Err(_) => warn!("Loading the world again failed; keeping the old one."),
        }
        return;
    }
    let Some((folder, loaded_at)) = world.source.clone() else { return };
    if watch.loading.is_some() || now - watch.looked_at < WATCH_EVERY {
        return;
    }
    watch.looked_at = now;
    let changed_at = manifest_changed_at(&folder);
    if changed_at.is_none() || changed_at == loaded_at {
        watch.seen = None;
        return;
    }
    match watch.seen {
        Some((since, at)) if at == changed_at && now - since >= SETTLE_SECONDS => {
            watch.seen = None;
            watch.loading = Some(std::thread::spawn(move || ServerWorld::load(&folder)));
        }
        Some((_, at)) if at == changed_at => {}
        _ => watch.seen = Some((now, changed_at)),
    }
}
