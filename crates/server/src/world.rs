//! The baked world, as the server needs it: which cells can be walked on.

use std::path::Path;

use bevy::prelude::*;
use venture_navigation::{ChunkObstacles, WalkingLimits};
use venture_world::{CellPos, Chunk, ChunkPos, MANIFEST_FILE, WorldManifest, chunk_file_path, decode_chunk};

/// The loaded world.
#[derive(Resource)]
pub struct ServerWorld {
    /// The world's description.
    pub manifest: WorldManifest,
    /// Blocked cells for every chunk, indexed `y * chunks_per_side + x`.
    obstacles: Vec<ChunkObstacles>,
}

impl ServerWorld {
    /// Loads a baked world folder, reading every chunk (using all CPU cores).
    pub fn load(folder: &Path) -> Result<Self, String> {
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

        let obstacles = std::thread::scope(|scope| {
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
                                Ok(ChunkObstacles::from_chunk(&chunk, limits))
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

        Ok(Self { manifest, obstacles: obstacles.into_iter().flatten().collect() })
    }

    /// A world made from chunks already in memory (for tests and tools).
    /// `chunks` must hold every chunk in the manifest, in row order.
    pub fn from_chunks(manifest: WorldManifest, chunks: &[Chunk]) -> Self {
        assert_eq!(chunks.len(), manifest.chunks_per_side().pow(2) as usize, "one chunk per world chunk");
        let limits = WalkingLimits::default();
        let obstacles = chunks.iter().map(|chunk| ChunkObstacles::from_chunk(chunk, &limits)).collect();
        Self { manifest, obstacles }
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

    /// Whether a point (in metres) is inside the world.
    pub fn contains(&self, (x, y): (f32, f32)) -> bool {
        let size = self.manifest.world_size as f32;
        (0.0..size).contains(&x) && (0.0..size).contains(&y)
    }
}
