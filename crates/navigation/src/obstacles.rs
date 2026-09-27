//! Which terrain cells can be walked on.

use venture_world::{CHUNK_SIZE, Chunk, FluidKind, PropKind};

/// What a character can walk through.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkingLimits {
    /// Deepest water that can be waded through, in metres.
    pub max_wading_depth: f32,
    /// Largest height difference across one cell that can be walked up, in metres.
    pub max_step: f32,
}

impl Default for WalkingLimits {
    fn default() -> Self {
        Self { max_wading_depth: 0.6, max_step: 1.2 }
    }
}

/// Which cells of one chunk are blocked.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkObstacles {
    blocked: Vec<bool>,
}

impl ChunkObstacles {
    /// Works out the blocked cells of a chunk.
    ///
    /// Props block the cells around their base. A prop near the chunk's edge
    /// only blocks cells in its own chunk, which is close enough for now.
    pub fn from_chunk(chunk: &Chunk, limits: &WalkingLimits) -> Self {
        let size = CHUNK_SIZE as usize;
        let mut blocked = vec![false; size * size];
        for y in 0..size {
            for x in 0..size {
                let heights = chunk.cell_corner_heights(x, y);
                let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
                let highest = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let too_deep = chunk.cell(x, y).fluid.is_some_and(|fluid| {
                    let depth = fluid.level - (lowest + highest) / 2.0;
                    fluid.kind == FluidKind::Lava || depth > limits.max_wading_depth
                });
                blocked[y * size + x] = too_deep || highest - lowest > limits.max_step;
            }
        }

        let (origin_x, origin_y) = chunk.pos.origin_cell();
        for prop in &chunk.props {
            let Some(radius) = blocking_radius(prop.kind) else { continue };
            let radius = radius * prop.scale;
            let (local_x, local_y) = (prop.x - origin_x as f32, prop.y - origin_y as f32);
            let cells = |centre: f32| {
                let first = (centre - radius).floor().max(0.0) as usize;
                let last = ((centre + radius).floor() as usize).min(size - 1);
                first..=last
            };
            for y in cells(local_y) {
                for x in cells(local_x) {
                    // Block the cell if its centre is within the prop's radius (or the prop stands in it).
                    let (dx, dy) = (x as f32 + 0.5 - local_x, y as f32 + 0.5 - local_y);
                    let contains_base = local_x.floor() as usize == x && local_y.floor() as usize == y;
                    if contains_base || dx.hypot(dy) < radius {
                        blocked[y * size + x] = true;
                    }
                }
            }
        }
        Self { blocked }
    }

    /// Whether the cell at local coordinates `(x, y)` is blocked.
    pub fn is_blocked(&self, x: usize, y: usize) -> bool {
        self.blocked[y * CHUNK_SIZE as usize + x]
    }
}

/// How far around its base a prop blocks, at scale 1, in metres. Bushes can
/// be pushed through.
fn blocking_radius(kind: PropKind) -> Option<f32> {
    match kind {
        PropKind::Oak | PropKind::Pine => Some(0.5),
        PropKind::Birch | PropKind::DeadTree => Some(0.35),
        PropKind::Boulder => Some(1.1),
        PropKind::Bush => None,
    }
}

#[cfg(test)]
mod tests {
    use venture_world::{ChunkPos, Fluid, FluidBody, Prop};

    use super::*;

    #[test]
    fn flat_dry_ground_is_open() {
        let obstacles = ChunkObstacles::from_chunk(&Chunk::new(ChunkPos::new(0, 0)), &WalkingLimits::default());
        assert!(!obstacles.is_blocked(10, 10));
    }

    #[test]
    fn deep_water_and_trees_block() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.cell_mut(5, 5).fluid = Some(Fluid { kind: FluidKind::Water, body: FluidBody::Lake, level: 2.0 });
        chunk.cell_mut(6, 5).fluid = Some(Fluid { kind: FluidKind::Water, body: FluidBody::Lake, level: 0.3 });
        chunk.props.push(Prop { kind: PropKind::Oak, x: 20.5, y: 20.5, ground_height: 0.0, scale: 1.0, rotation: 0.0 });
        let obstacles = ChunkObstacles::from_chunk(&chunk, &WalkingLimits::default());
        assert!(obstacles.is_blocked(5, 5), "deep water");
        assert!(!obstacles.is_blocked(6, 5), "shallow water can be waded");
        assert!(obstacles.is_blocked(20, 20), "tree trunk");
        assert!(!obstacles.is_blocked(22, 20), "beside the tree");
    }

    #[test]
    fn steep_slopes_block() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set_corner_height(31, 31, 3.0);
        let obstacles = ChunkObstacles::from_chunk(&chunk, &WalkingLimits::default());
        assert!(obstacles.is_blocked(30, 30));
        assert!(!obstacles.is_blocked(40, 40));
    }
}
