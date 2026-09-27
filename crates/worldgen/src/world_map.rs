//! Stage 1: the coarse map of the whole world.
//!
//! Land height is sampled every `map_cell_size` metres, then drained (see
//! [`crate::hydrology`]) to find the sea, lakes and rivers. Settlement sites
//! are chosen and joined by roads. Chunk generation reads this map, which is
//! what keeps rivers and roads continuous across chunks.

use venture_world::{Biome, Fluid, FluidBody, FluidKind};

use crate::WorldGenConfig;
use crate::distance_field::{Nearest, nearest_sources};
use crate::geometry::Point;
use crate::grid::cell_hash;
use crate::hydrology::{drain, find_lakes};
use crate::noise_fields::{NoiseFields, lerp};
use crate::regions::Regions;
use crate::rivers::{RiverPoint, RiverSegment};
use crate::roads::{RoadSegment, plan_roads};
use crate::settlements::{Settlement, choose_settlements};
use crate::spatial::Buckets;

/// How far river points are nudged off the map grid, as a fraction of a cell.
const RIVER_MEANDER: f32 = 0.7;

/// Road segments are grouped into buckets this many metres wide.
const ROAD_BUCKET_SIZE: f32 = 64.0;

/// The coarse whole-world map. Points are `map_cell_size` metres apart, and
/// point `(i, j)` sits at `(i * map_cell_size, j * map_cell_size)`.
pub struct WorldMap {
    /// Points along each axis.
    size: usize,
    /// Metres between points.
    cell_size: f32,
    /// Land (or sea/lake bed) height at each point.
    land_height: Vec<f32>,
    /// The sea, lake or lava lake covering each point, if any.
    still_fluid: Vec<Option<Fluid>>,
    /// The river flowing out of each point, if it carries one.
    rivers: Vec<Option<RiverSegment>>,
    /// The nearest sea or lake point to each point.
    shore: Vec<Option<Nearest>>,
    /// How far (in points) a river can affect the land around it.
    river_reach: usize,
    /// Settlement sites, best first.
    settlements: Vec<Settlement>,
    /// Road segments, grouped by position.
    roads: Buckets<RoadSegment>,
    /// How far a road can affect the land around it, including the length of
    /// a segment reaching out of its bucket.
    road_reach: f32,
}

/// The nearest sea or lake to some position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShoreSample {
    /// Approximate distance to it, in metres.
    pub distance: f32,
    /// Its liquid.
    pub fluid: Fluid,
    /// The position of the nearest map point it covers.
    pub source: Point,
}

/// Read-only views of the map's grids, used by the planning algorithms while
/// the map is being built.
pub struct MapGrid<'a> {
    /// Points along each axis.
    pub size: usize,
    /// Metres between points.
    pub cell_size: f32,
    /// Land height at each point.
    pub land_height: &'a [f32],
    /// Sea or lake at each point.
    pub still_fluid: &'a [Option<Fluid>],
    /// Whether each point carries a river.
    pub is_river: &'a [bool],
}

impl MapGrid<'_> {
    /// Whether a point is covered by sea or lake.
    pub fn is_water(&self, index: usize) -> bool {
        self.still_fluid[index].is_some()
    }

    /// Whether a point is on the edge of the map.
    pub fn is_edge(&self, index: usize) -> bool {
        let (i, j) = (index % self.size, index / self.size);
        i == 0 || j == 0 || i == self.size - 1 || j == self.size - 1
    }

    /// Position of a point, in metres.
    pub fn position(&self, index: usize) -> Point {
        ((index % self.size) as f32 * self.cell_size, (index / self.size) as f32 * self.cell_size)
    }
}

impl WorldMap {
    /// Generates the map for a world.
    pub fn generate(config: &WorldGenConfig, noise: &NoiseFields, regions: &Regions) -> Self {
        let size = config.map_points();
        let cell_size = config.map_cell_size as f32;
        let position = |index: usize| ((index % size) as f32 * cell_size, (index / size) as f32 * cell_size);

        let raw: Vec<f32> = (0..size * size)
            .map(|index| {
                let (x, y) = position(index);
                noise.land_height(config, regions, x as f64, y as f64)
            })
            .collect();

        let drainage = drain(&raw, size);
        let lakes =
            find_lakes(&raw, &drainage.filled, size, config.sea_level, config.lake_min_depth, config.lake_min_cells);
        let lava_lakes = lava_lakes(&lakes, regions, &position);

        // A filled height below sea level means the point is connected to the
        // sea without rising above it: it is sea.
        let still_fluid: Vec<Option<Fluid>> = (0..size * size)
            .map(|index| {
                if drainage.filled[index] < config.sea_level {
                    return Some(Fluid { kind: FluidKind::Water, body: FluidBody::Sea, level: config.sea_level });
                }
                let lake = lakes[index]?;
                let kind = if lava_lakes[lake] { FluidKind::Lava } else { FluidKind::Water };
                Some(Fluid { kind, body: FluidBody::Lake, level: drainage.filled[index] })
            })
            .collect();

        // Under liquid, keep the real bed. On land, use the filled height so
        // small pits are smoothed away and rivers never flow uphill.
        let land_height: Vec<f32> = (0..size * size)
            .map(|index| match still_fluid[index] {
                Some(_) => raw[index],
                None => drainage.filled[index],
            })
            .collect();

        let surface = |index: usize| still_fluid[index].map_or(land_height[index], |fluid| fluid.level);
        // Nudge each river point off the grid a little, so rivers meander
        // instead of running in straight lines. Neighbouring segments share
        // points, so rivers stay connected.
        let meander = |index: usize| {
            let (x, y) = position(index);
            let nudge =
                |salt: u32| (cell_hash(index, config.seed.wrapping_add(salt)) - 0.5) * RIVER_MEANDER * cell_size;
            (x + nudge(31), y + nudge(32))
        };
        let rivers: Vec<Option<RiverSegment>> = (0..size * size)
            .map(|index| {
                let flow = drainage.flow[index];
                let next = drainage.downstream[index]?;
                if flow < config.river_min_flow || still_fluid[index].is_some() {
                    return None;
                }
                let (x, y) = meander(index);
                let (next_x, next_y) = if still_fluid[next].is_some() { position(next) } else { meander(next) };
                let start = RiverPoint::new(config, x, y, surface(index), flow);
                // Rivers keep their size as they enter a lake or the sea.
                let next_flow = if still_fluid[next].is_some() { flow } else { drainage.flow[next] };
                let end = RiverPoint::new(config, next_x, next_y, surface(next), next_flow);
                Some(RiverSegment { start, end })
            })
            .collect();

        let is_river: Vec<bool> = rivers.iter().map(Option::is_some).collect();
        let grid =
            MapGrid { size, cell_size, land_height: &land_height, still_fluid: &still_fluid, is_river: &is_river };
        let sites = choose_settlements(config, &grid);
        let road_lines = plan_roads(config, &grid, &sites);
        let settlements = sites
            .iter()
            .map(|&index| {
                let position = grid.position(index);
                Settlement { position, biome: regions.weights_at(position.0, position.1).dominant() }
            })
            .collect();

        let mut roads = Buckets::new(config.world_size as f32, ROAD_BUCKET_SIZE);
        for line in &road_lines {
            for pair in line.windows(2) {
                let middle = ((pair[0].0 + pair[1].0) / 2.0, (pair[0].1 + pair[1].1) / 2.0);
                roads.insert(middle, RoadSegment { start: pair[0], end: pair[1] });
            }
        }

        let is_still: Vec<bool> = still_fluid.iter().map(Option::is_some).collect();
        let shore = nearest_sources(size, &is_still);

        // A river affects land up to its half width plus floodplain away, and
        // its segments stretch up to one diagonal map cell from their start point.
        let river_reach = config.river_max_half_width + config.floodplain_width + cell_size * std::f32::consts::SQRT_2;
        Self {
            size,
            cell_size,
            land_height,
            still_fluid,
            rivers,
            shore,
            river_reach: (river_reach / cell_size).ceil() as usize + 1,
            settlements,
            roads,
            road_reach: config.road_half_width + config.road_verge_width + cell_size,
        }
    }

    /// Land height at any position, interpolated between map points.
    pub fn land_height_at(&self, x: f32, y: f32) -> f32 {
        let (i, j, fx, fy) = self.cell_at(x, y);
        let height = |i: usize, j: usize| self.land_height[j * self.size + i];
        let top = lerp(height(i, j), height(i + 1, j), fx);
        let bottom = lerp(height(i, j + 1), height(i + 1, j + 1), fx);
        lerp(top, bottom, fy)
    }

    /// The seas and lakes at the map points around a position, with each
    /// point's position.
    pub fn still_fluids_around(&self, x: f32, y: f32) -> impl Iterator<Item = (Point, Fluid)> + '_ {
        let (i, j, _, _) = self.cell_at(x, y);
        [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)].into_iter().filter_map(move |(i, j)| {
            let fluid = self.still_fluid[j * self.size + i]?;
            Some(((i as f32 * self.cell_size, j as f32 * self.cell_size), fluid))
        })
    }

    /// The nearest sea or lake to a position. The distance is interpolated
    /// between map points, so it is smooth but approximate.
    pub fn shore_at(&self, x: f32, y: f32) -> Option<ShoreSample> {
        let (i, j, fx, fy) = self.cell_at(x, y);
        let corners = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)].map(|(i, j)| self.shore[j * self.size + i]);
        let [Some(a), Some(b), Some(c), Some(d)] = corners else { return None };
        let distance = lerp(lerp(a.distance, b.distance, fx), lerp(c.distance, d.distance, fx), fy);
        let nearest = [a, b, c, d].into_iter().min_by(|p, q| p.distance.total_cmp(&q.distance))?;
        let source = nearest.source;
        Some(ShoreSample {
            distance: distance * self.cell_size,
            fluid: self.still_fluid[source]?,
            source: ((source % self.size) as f32 * self.cell_size, (source / self.size) as f32 * self.cell_size),
        })
    }

    /// Every river segment that could affect land inside the given area, in a
    /// fixed global order.
    pub fn rivers_near(&self, min: Point, max: Point) -> Vec<RiverSegment> {
        let to_point = |metres: f32| (metres / self.cell_size).floor().max(0.0) as usize;
        let last = self.size - 1;
        let i0 = to_point(min.0).saturating_sub(self.river_reach);
        let j0 = to_point(min.1).saturating_sub(self.river_reach);
        let i1 = (to_point(max.0) + self.river_reach).min(last);
        let j1 = (to_point(max.1) + self.river_reach).min(last);

        let mut segments = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                segments.extend(self.rivers[j * self.size + i]);
            }
        }
        segments
    }

    /// Every road segment that could affect land inside the given area, in a
    /// fixed global order.
    pub fn roads_near(&self, min: Point, max: Point) -> Vec<RoadSegment> {
        self.roads.near(min, max, self.road_reach)
    }

    /// Settlement sites, best first.
    pub fn settlements(&self) -> &[Settlement] {
        &self.settlements
    }

    /// Every road segment in the world.
    pub fn all_roads(&self) -> impl Iterator<Item = &RoadSegment> {
        self.roads.iter()
    }

    /// Points along each axis.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Land height at map point `(i, j)`.
    pub fn point_land_height(&self, i: usize, j: usize) -> f32 {
        self.land_height[j * self.size + i]
    }

    /// The sea or lake at map point `(i, j)`, if any.
    pub fn point_still_fluid(&self, i: usize, j: usize) -> Option<Fluid> {
        self.still_fluid[j * self.size + i]
    }

    /// The river flowing out of map point `(i, j)`, if any.
    pub fn point_river(&self, i: usize, j: usize) -> Option<&RiverSegment> {
        self.rivers[j * self.size + i].as_ref()
    }

    /// The map cell containing a position, as the cell's first point `(i, j)`
    /// and the position's fraction `(fx, fy)` across the cell. Positions
    /// outside the world are clamped to its edge.
    fn cell_at(&self, x: f32, y: f32) -> (usize, usize, f32, f32) {
        let max = (self.size - 1) as f32;
        let gx = (x / self.cell_size).clamp(0.0, max);
        let gy = (y / self.cell_size).clamp(0.0, max);
        let i = (gx.floor() as usize).min(self.size - 2);
        let j = (gy.floor() as usize).min(self.size - 2);
        (i, j, gx - i as f32, gy - j as f32)
    }
}

/// Lakes bigger than this many map cells hold water even in volcanic land;
/// lava only pools in craters and small hollows.
const MAX_LAVA_LAKE_CELLS: u32 = 120;

/// For each lake, whether it holds lava: true for small lakes that lie mostly in volcanic land.
fn lava_lakes(lakes: &[Option<usize>], regions: &Regions, position: &impl Fn(usize) -> Point) -> Vec<bool> {
    let lake_count = lakes.iter().flatten().max().map_or(0, |&last| last + 1);
    let mut volcanic = vec![0.0_f32; lake_count];
    let mut cells = vec![0_u32; lake_count];
    for (index, lake) in lakes.iter().enumerate() {
        let Some(lake) = *lake else { continue };
        let (x, y) = position(index);
        volcanic[lake] += regions.weights_at(x, y).of(Biome::Volcanic);
        cells[lake] += 1;
    }
    volcanic
        .iter()
        .zip(&cells)
        .map(|(&weight, &count)| count <= MAX_LAVA_LAKE_CELLS && weight / count as f32 > 0.5)
        .collect()
}
