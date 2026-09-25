//! Small helpers shared by the algorithms that work on the coarse map grid.

use std::cmp::Ordering;

/// Offsets to the 8 neighbours of a cell (D8 connectivity).
const NEIGHBOURS: [(isize, isize); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// Indices of the in-bounds neighbours of `index` in a `size` × `size` grid.
pub fn neighbours(index: usize, size: usize) -> impl Iterator<Item = usize> {
    let (x, y) = ((index % size) as isize, (index / size) as isize);
    NEIGHBOURS.iter().filter_map(move |&(dx, dy)| {
        let (nx, ny) = (x + dx, y + dy);
        let in_bounds = (0..size as isize).contains(&nx) && (0..size as isize).contains(&ny);
        in_bounds.then(|| ny as usize * size + nx as usize)
    })
}

/// Distance between two cells of a `size`-wide grid, in cells.
pub fn cell_distance(a: usize, b: usize, size: usize) -> f32 {
    let (ax, ay) = ((a % size) as f32, (a / size) as f32);
    let (bx, by) = ((b % size) as f32, (b / size) as f32);
    (ax - bx).hypot(ay - by)
}

/// A repeatable pseudo-random number in `0.0..1.0` for a grid cell and seed.
pub fn cell_hash(index: usize, seed: u32) -> f32 {
    let mut x = (index as u32) ^ seed.wrapping_mul(0x9e37_79b9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x & 0xffff) as f32 / 65536.0
}

/// A repeatable pseudo-random number in `0.0..1.0` for integer coordinates
/// and a salt (use different salts for independent numbers at one position).
pub fn point_hash(x: i32, y: i32, salt: u32) -> f32 {
    let mixed = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
    cell_hash(mixed as usize, salt)
}

/// An entry in a min-priority queue (`BinaryHeap<MinEntry>` pops the lowest
/// `key` first). Ties are broken by index so results never depend on heap internals.
#[derive(PartialEq)]
pub struct MinEntry {
    /// The priority; lowest comes out first.
    pub key: f32,
    /// The grid cell this entry is for.
    pub index: usize,
}

impl Eq for MinEntry {}

impl Ord for MinEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed, because `BinaryHeap` pops the largest item.
        other.key.total_cmp(&self.key).then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for MinEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
