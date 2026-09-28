//! The baked world, as the server needs it: which cells can be walked on,
//! and which are water (of what body, in what biome: for fishing).

use std::path::Path;

use bevy::prelude::*;
use venture_navigation::{ChunkObstacles, WalkingLimits};
use venture_world::{
    Biome, CHUNK_SIZE, CellPos, Chunk, ChunkPos, FluidBody, FluidKind, MANIFEST_FILE, WorldManifest, chunk_file_path,
    decode_chunk,
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
                                Ok((ChunkObstacles::from_chunk(&chunk, limits), Waters::from_chunk(&chunk)))
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

        let (obstacles, waters) = obstacles.into_iter().flatten().unzip();
        Ok(Self { manifest, obstacles, waters })
    }

    /// A world made from chunks already in memory (for tests and tools).
    /// `chunks` must hold every chunk in the manifest, in row order.
    pub fn from_chunks(manifest: WorldManifest, chunks: &[Chunk]) -> Self {
        assert_eq!(chunks.len(), manifest.chunks_per_side().pow(2) as usize, "one chunk per world chunk");
        let limits = WalkingLimits::default();
        let obstacles = chunks.iter().map(|chunk| ChunkObstacles::from_chunk(chunk, &limits)).collect();
        let waters = chunks.iter().map(Waters::from_chunk).collect();
        Self { manifest, obstacles, waters }
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
