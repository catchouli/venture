//! Stage 2: full detail for one chunk.
//!
//! Beaches depend on water in neighbouring cells, so heights and water are
//! worked out for a slightly larger area than the chunk: the chunk plus a
//! margin as wide as the widest beach. Every value depends only on its world
//! position, so the margins agree with the neighbouring chunks.

use venture_world::{CHUNK_SIZE, Chunk, ChunkPos, Fluid, FluidKind, TerrainCell};

use crate::distance_field::{Nearest, nearest_sources};
use crate::geometry::{Point, lerp_point};
use crate::noise_fields::{lerp, smoothstep};
use crate::overlay::LocalOverlay;
use crate::props::scatter_props;
use crate::rivers::{RiverSegment, nearest_river};
use crate::roads::{RoadSegment, nearest_road};
use crate::surface::{Shore, SurfaceInputs, choose_ground};
use crate::{WorldGenConfig, WorldGenerator};

/// Dips in the ground this close (in metres) to a sea or lake are flooded by it.
const STILL_WATER_REACH: f32 = 32.0;

/// ...as long as they are no deeper than this, in metres.
const STILL_WATER_MAX_DEPTH: f32 = 2.5;

/// Ground this close (in metres) to a river's edge that dips below its
/// surface is under its water too (the channel only lowers land, so land
/// already lower beside it would otherwise be dry below the river's level,
/// and its shore a line along the cells.
const RIVER_SPILL_REACH: f32 = 6.0;

/// Spacing of the ground samples when checking that water can reach a cell, in metres.
const WATER_CHECK_STEP: f32 = 2.0;

/// Salt for the random number that dithers biome borders cell by cell.
const BIOME_DITHER_SALT: u32 = 201;

/// Everything that can affect the terrain in some area: rivers, roads and overlay stamps.
pub struct LocalFeatures {
    rivers: Vec<RiverSegment>,
    roads: Vec<RoadSegment>,
    overlay: LocalOverlay,
}

impl LocalFeatures {
    /// Collects the features that can affect the area from `min` to `max`.
    pub fn near(generator: &WorldGenerator, min: Point, max: Point) -> Self {
        let map = generator.map();
        Self {
            rivers: map.rivers_near(min, max),
            roads: map.roads_near(min, max),
            overlay: generator.overlay.near(min, max),
        }
    }
}

/// Liquid over one cell, remembering whether it is a river (for bank widths).
#[derive(Clone, Copy)]
struct CellFluid {
    fluid: Fluid,
    is_river: bool,
}

/// Generates one chunk.
pub fn generate_chunk(generator: &WorldGenerator, pos: ChunkPos) -> Chunk {
    let config = generator.config();
    let margin = config.beach_width.max(config.river_bank_width).max(0);
    let (chunk_x, chunk_y) = pos.origin_cell();
    let area = Area { x: chunk_x - margin, y: chunk_y - margin, size: CHUNK_SIZE + 2 * margin };
    let features = LocalFeatures::near(
        generator,
        (area.x as f32, area.y as f32),
        ((area.x + area.size) as f32, (area.y + area.size) as f32),
    );

    // Corner heights and liquids for the whole area, margin included.
    let corners =
        Grid::new(area.size + 1, |x, y| corner_height(generator, &features, (area.x + x) as f32, (area.y + y) as f32));
    let cell_corners =
        |x: i32, y: i32| [corners.get(x, y), corners.get(x + 1, y), corners.get(x, y + 1), corners.get(x + 1, y + 1)];
    let fluids = Grid::new(area.size, |x, y| {
        let centre = ((area.x + x) as f32 + 0.5, (area.y + y) as f32 + 0.5);
        fluid_over_cell(generator, &features, centre, cell_corners(x, y))
    });
    let shores = shore_field(config, &fluids);

    let mut chunk = Chunk::new(pos);
    for y in 0..=CHUNK_SIZE {
        for x in 0..=CHUNK_SIZE {
            chunk.set_corner_height(x as usize, y as usize, corners.get(x + margin, y + margin));
        }
    }
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            // Position within the area grids, and in the world.
            let (ax, ay) = (x + margin, y + margin);
            let (world_x, world_y) = (chunk_x + x, chunk_y + y);
            let (centre_x, centre_y) = (world_x as f32 + 0.5, world_y as f32 + 0.5);

            let weights = generator.regions.weights_at(centre_x, centre_y);
            let biome = weights.pick(crate::grid::point_hash(world_x, world_y, config.seed ^ BIOME_DITHER_SALT));
            let fluid = fluids.get(ax, ay).map(|cell| cell.fluid);
            let noise = &generator.noise;
            let inputs = SurfaceInputs {
                corner_heights: cell_corners(ax, ay),
                fluid,
                shore: shores.get(ax, ay),
                on_road: nearest_road(&features.roads, centre_x, centre_y)
                    .is_some_and(|road| road.distance < config.road_half_width),
                painted: features.overlay.painted_ground(centre_x, centre_y),
                biome,
                rock_start_height: weights.blend(|b| config.biome(b).rock_start_height),
                edge_noise: noise.patch(centre_x as f64, centre_y as f64),
                patch_noise: noise.patch(centre_x as f64 * 0.3 + 5000.0, centre_y as f64 * 0.3 + 5000.0),
            };
            *chunk.cell_mut(x as usize, y as usize) =
                TerrainCell { ground: choose_ground(config, &inputs), fluid, biome };
        }
    }
    chunk.props = scatter_props(generator, &chunk, &features.overlay);
    chunk
}

/// The height of the terrain at corner `(x, y)`. Built up in stages, each
/// stage refining the one before:
/// 1. the coarse map height plus fine relief (scaled by biome),
/// 2. graded down gently towards lakes and the sea,
/// 3. levelled under roads,
/// 4. shaped by overlay stamps (such as settlement grounds),
/// 5. carved by rivers.
pub fn corner_height(generator: &WorldGenerator, features: &LocalFeatures, x: f32, y: f32) -> f32 {
    let config = generator.config();
    let map = generator.map();
    let relief_amount = generator.regions.weights_at(x, y).blend(|b| config.biome(b).relief);
    let mut height = map.land_height_at(x, y) + generator.noise.relief(config, relief_amount, x as f64, y as f64);

    if let Some(shore) = map.shore_at(x, y) {
        let distance = shore.distance;
        let graded = height.min(shore.fluid.level + config.shore_slope * distance);
        let untouched = smoothstep(config.shore_reach * 0.5, config.shore_reach, distance);
        height = lerp(graded, height, untouched);
    }

    // Roads follow the smooth coarse terrain, blending into the land at their verges.
    if let Some(road) = nearest_road(&features.roads, x, y) {
        let road_height = map.land_height_at(road.closest.0, road.closest.1);
        let verge_end = config.road_half_width + config.road_verge_width;
        let levelled = 1.0 - smoothstep(config.road_half_width, verge_end, road.distance);
        height = lerp(height, road_height, levelled);
    }

    height = features.overlay.shape_height(x, y, height);

    if let Some(river) = nearest_river(&features.rivers, x, y) {
        height = river.carve(height, config.bank_height);
    }
    height
}

/// The liquid over the cell centred at `(x, y)`, if any. A cell is covered
/// when a river, lake or sea is there (or, for a river, just beside it) and
/// at least one corner dips below its surface.
fn fluid_over_cell(
    generator: &WorldGenerator,
    features: &LocalFeatures,
    (x, y): (f32, f32),
    corner_heights: [f32; 4],
) -> Option<CellFluid> {
    let lowest = corner_heights.into_iter().fold(f32::INFINITY, f32::min);
    let river = nearest_river(&features.rivers, x, y)
        .filter(|river| river.distance_to_edge() < RIVER_SPILL_REACH)
        .map(|river| CellFluid { fluid: Fluid { kind: FluidKind::Water, level: river.surface }, is_river: true });
    // The sea or lake at the surrounding map points, or failing that the
    // nearest one if it's close enough to flood a shallow dip here. Either
    // way, only if no higher ground (such as a crater's rim) lies between.
    let map = generator.map();
    let reachable = |source: Point, fluid: Fluid| open_to_water(generator, (x, y), source, fluid);
    let around = map
        .still_fluids_around(x, y)
        .filter(|&(source, fluid)| reachable(source, fluid))
        .map(|(_, fluid)| fluid)
        .reduce(|a, b| if b.level > a.level { b } else { a });
    let nearby = || {
        map.shore_at(x, y)
            .filter(|shore| shore.distance < STILL_WATER_REACH && lowest > shore.fluid.level - STILL_WATER_MAX_DEPTH)
            .filter(|shore| reachable(shore.source, shore.fluid))
            .map(|shore| shore.fluid)
    };
    let still = around.or_else(nearby).map(|fluid| CellFluid { fluid, is_river: false });
    [river, still]
        .into_iter()
        .flatten()
        .filter(|cell| lowest < cell.fluid.level)
        .reduce(|a, b| if b.fluid.level > a.fluid.level { b } else { a })
}

/// Whether the ground on the straight line from `from` to a sea or lake's
/// map point `to` stays below the liquid's surface, so the liquid can reach
/// `from`. Rims and ridges are coarse features, so the coarse map height is
/// enough. The sea has no rims, so it is always reachable.
fn open_to_water(generator: &WorldGenerator, from: Point, to: Point, fluid: Fluid) -> bool {
    let config = generator.config();
    if fluid.kind == FluidKind::Water && fluid.level == config.sea_level {
        return true;
    }
    let steps = ((from.0 - to.0).hypot(from.1 - to.1) / WATER_CHECK_STEP).ceil().max(1.0) as usize;
    (1..steps).all(|step| {
        let (x, y) = lerp_point(from, to, step as f32 / steps as f32);
        generator.map().land_height_at(x, y) < fluid.level
    })
}

/// For every cell of the area, how close it is to a shore. Lakes and the sea
/// have wide beaches; rivers have narrow banks.
fn shore_field(config: &WorldGenConfig, fluids: &Grid<Option<CellFluid>>) -> Grid<Option<Shore>> {
    let size = fluids.size as usize;
    let sources = |rivers: bool| -> Vec<bool> {
        fluids.values.iter().map(|cell| cell.is_some_and(|cell| cell.is_river == rivers)).collect()
    };
    let near_still = nearest_sources(size, &sources(false));
    let near_river = nearest_sources(size, &sources(true));

    let as_shore = |nearest: Option<Nearest>, reach: i32| {
        let nearest = nearest?;
        let source = fluids.values[nearest.source]?;
        Some(Shore { closeness: nearest.distance / reach.max(1) as f32, level: source.fluid.level })
            .filter(|shore| shore.closeness <= 1.0)
    };
    Grid::new(fluids.size, |x, y| {
        let index = y as usize * size + x as usize;
        let still = as_shore(near_still[index], config.beach_width);
        let river = as_shore(near_river[index], config.river_bank_width);
        [still, river].into_iter().flatten().min_by(|a, b| a.closeness.total_cmp(&b.closeness))
    })
}

/// A square block of cells in world coordinates.
struct Area {
    x: i32,
    y: i32,
    size: i32,
}

/// A square grid of values, indexed from `(0, 0)`.
struct Grid<T> {
    size: i32,
    values: Vec<T>,
}

impl<T: Clone> Grid<T> {
    /// Builds a `size` × `size` grid by calling `value(x, y)` for every cell.
    fn new(size: i32, mut value: impl FnMut(i32, i32) -> T) -> Self {
        let values = (0..size * size).map(|i| value(i % size, i / size)).collect();
        Self { size, values }
    }

    fn get(&self, x: i32, y: i32) -> T {
        self.values[(y * self.size + x) as usize].clone()
    }
}

#[cfg(test)]
mod tests {
    use venture_world::CHUNK_SIZE;

    use super::*;
    use crate::WorldGenConfig;

    fn small_world() -> WorldGenerator {
        WorldGenerator::new(WorldGenConfig { world_size: 1024, ..WorldGenConfig::default() })
    }

    #[test]
    fn generation_is_deterministic() {
        let pos = ChunkPos::new(3, 4);
        assert_eq!(small_world().generate_chunk(pos), small_world().generate_chunk(pos));
    }

    #[test]
    fn neighbouring_chunks_share_edge_heights() {
        let world = small_world();
        let last = CHUNK_SIZE as usize;
        for cy in 0..7 {
            for cx in 0..7 {
                let here = world.generate_chunk(ChunkPos::new(cx, cy));
                let east = world.generate_chunk(ChunkPos::new(cx + 1, cy));
                let south = world.generate_chunk(ChunkPos::new(cx, cy + 1));
                for i in 0..=last {
                    assert_eq!(here.corner_height(last, i), east.corner_height(0, i));
                    assert_eq!(here.corner_height(i, last), south.corner_height(i, 0));
                }
            }
        }
    }

    #[test]
    fn ground_height_matches_chunk_corners() {
        let world = small_world();
        let chunk = world.generate_chunk(ChunkPos::new(2, 5));
        let (ox, oy) = chunk.pos.origin_cell();
        for (x, y) in [(0, 0), (10, 33), (64, 64)] {
            let expected = chunk.corner_height(x, y);
            let actual = world.ground_height((ox + x as i32) as f32, (oy + y as i32) as f32);
            assert!((expected - actual).abs() < 1e-4);
        }
    }
}
