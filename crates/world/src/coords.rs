//! Terrain cell and chunk coordinates.

use serde::{Deserialize, Serialize};

use crate::CHUNK_SIZE;

/// The position of one terrain cell. Cell `(x, y)` covers the square from
/// `(x, y)` to `(x + 1, y + 1)` metres. `x` grows to the east and `y` to the south.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellPos {
    /// Cell coordinate along the east axis.
    pub x: i32,
    /// Cell coordinate along the south axis.
    pub y: i32,
}

impl CellPos {
    /// Creates a cell position.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The cell containing a point, given in metres.
    pub fn containing(x: f32, y: f32) -> Self {
        Self::new(x.floor() as i32, y.floor() as i32)
    }

    /// The chunk that contains this cell.
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos::containing(self.x, self.y)
    }

    /// This cell's coordinates relative to its chunk's origin, each in `0..CHUNK_SIZE`.
    pub const fn local(self) -> (usize, usize) {
        (self.x.rem_euclid(CHUNK_SIZE) as usize, self.y.rem_euclid(CHUNK_SIZE) as usize)
    }
}

/// The position of a chunk, measured in chunks (not cells).
///
/// Chunk `(0, 0)` covers cells `0..CHUNK_SIZE` on both axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkPos {
    /// Chunk coordinate along the east axis.
    pub x: i32,
    /// Chunk coordinate along the south axis.
    pub y: i32,
}

impl ChunkPos {
    /// Creates a chunk position.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The chunk containing cell `(cell_x, cell_y)`. Works for negative coordinates.
    pub const fn containing(cell_x: i32, cell_y: i32) -> Self {
        Self { x: cell_x.div_euclid(CHUNK_SIZE), y: cell_y.div_euclid(CHUNK_SIZE) }
    }

    /// Cell coordinates of this chunk's first (lowest x and y) cell.
    pub const fn origin_cell(self) -> (i32, i32) {
        (self.x * CHUNK_SIZE, self.y * CHUNK_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_cells_map_to_chunks() {
        let cell = CellPos::new(65, 130);
        assert_eq!(cell.chunk(), ChunkPos::new(1, 2));
        assert_eq!(cell.local(), (1, 2));
    }

    #[test]
    fn negative_cells_map_to_chunks() {
        let cell = CellPos::new(-1, -64);
        assert_eq!(cell.chunk(), ChunkPos::new(-1, -1));
        assert_eq!(cell.local(), (63, 0));
    }

    #[test]
    fn points_map_to_cells() {
        assert_eq!(CellPos::containing(3.7, -0.2), CellPos::new(3, -1));
    }

    #[test]
    fn origin_cell_round_trips() {
        let chunk = ChunkPos::new(-3, 7);
        let (x, y) = chunk.origin_cell();
        assert_eq!(ChunkPos::containing(x, y), chunk);
        assert_eq!(CellPos::new(x, y).local(), (0, 0));
    }
}
