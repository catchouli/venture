//! Chunks: square blocks of terrain.

use crate::{CHUNK_CORNERS, CHUNK_SIZE, ChunkPos, NavTile, Prop, TerrainCell};

/// A `CHUNK_SIZE` × `CHUNK_SIZE` block of terrain: a heightfield plus the
/// cells on it.
///
/// Heights are stored at cell *corners*, so a chunk has `CHUNK_CORNERS`²
/// heights and `CHUNK_SIZE`² cells. The corners on a chunk's edge are shared
/// with (and identical to) those of the neighbouring chunk.
///
/// The heightfield is only the ground surface. Caves, overhangs, bridges and
/// buildings are separate 3D structures placed in the world; see
/// `docs/world-model.md`.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    /// Where this chunk is in the world.
    pub pos: ChunkPos,
    /// Trees, rocks and bushes whose base is inside this chunk.
    pub props: Vec<Prop>,
    /// Where characters can stand, as a navigation mesh (empty until baked).
    pub navmesh: NavTile,
    heights: Vec<f32>,
    cells: Vec<TerrainCell>,
}

impl Chunk {
    /// A flat chunk at height zero, filled with default cells.
    pub fn new(pos: ChunkPos) -> Self {
        Self {
            pos,
            props: Vec::new(),
            navmesh: NavTile::default(),
            heights: vec![0.0; (CHUNK_CORNERS * CHUNK_CORNERS) as usize],
            cells: vec![TerrainCell::default(); (CHUNK_SIZE * CHUNK_SIZE) as usize],
        }
    }

    /// Height of the corner at local corner coordinates, each in `0..=CHUNK_SIZE`.
    pub fn corner_height(&self, x: usize, y: usize) -> f32 {
        self.heights[corner_index(x, y)]
    }

    /// Sets the height of the corner at local corner coordinates.
    pub fn set_corner_height(&mut self, x: usize, y: usize, height: f32) {
        self.heights[corner_index(x, y)] = height;
    }

    /// The cell at local cell coordinates, each in `0..CHUNK_SIZE`.
    pub fn cell(&self, x: usize, y: usize) -> &TerrainCell {
        &self.cells[cell_index(x, y)]
    }

    /// Mutable access to the cell at local cell coordinates.
    pub fn cell_mut(&mut self, x: usize, y: usize) -> &mut TerrainCell {
        &mut self.cells[cell_index(x, y)]
    }

    /// Ground height at a point inside the chunk, given in metres relative to
    /// the chunk's origin, interpolated between the surrounding corners.
    pub fn height_at(&self, local_x: f32, local_y: f32) -> f32 {
        let max = CHUNK_SIZE as f32;
        let (x, y) = (local_x.clamp(0.0, max), local_y.clamp(0.0, max));
        let (x0, y0) =
            ((x.floor() as usize).min(CHUNK_SIZE as usize - 1), (y.floor() as usize).min(CHUNK_SIZE as usize - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let [h00, h10, h01, h11] = self.cell_corner_heights(x0, y0);
        let top = h00 + (h10 - h00) * fx;
        let bottom = h01 + (h11 - h01) * fx;
        top + (bottom - top) * fy
    }

    /// The four corner heights of a cell, in the order
    /// `[(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]`.
    pub fn cell_corner_heights(&self, x: usize, y: usize) -> [f32; 4] {
        [
            self.corner_height(x, y),
            self.corner_height(x + 1, y),
            self.corner_height(x, y + 1),
            self.corner_height(x + 1, y + 1),
        ]
    }
}

fn corner_index(x: usize, y: usize) -> usize {
    debug_assert!(x < CHUNK_CORNERS as usize && y < CHUNK_CORNERS as usize);
    y * CHUNK_CORNERS as usize + x
}

fn cell_index(x: usize, y: usize) -> usize {
    debug_assert!(x < CHUNK_SIZE as usize && y < CHUNK_SIZE as usize);
    y * CHUNK_SIZE as usize + x
}
