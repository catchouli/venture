//! Venture's world generation engine: deterministic procedural terrain with
//! authored content layered on top.
//!
//! Generation happens in two stages (see `docs/world-generation-engine.md`):
//! 1. [`WorldMap`]: a coarse map of the whole world, with biome regions,
//!    land height, sea, lakes, rivers, settlement sites and roads. Built once
//!    when the generator is created.
//! 2. [`WorldGenerator::generate_chunk`]: full detail for one chunk (heights,
//!    materials, liquids and props), read from the world map plus fine noise,
//!    then shaped by overlay stamps.
//!
//! Everything is a pure function of the config, the overlay and the
//! position, so chunks can be generated in any order and always line up with
//! their neighbours.

mod chunk_gen;
mod config;
mod distance_field;
mod geometry;
mod grid;
mod hydrology;
pub mod map_labels;
pub mod map_render;
mod noise_fields;
mod overlay;
mod props;
mod regions;
mod rivers;
mod roads;
mod settlements;
mod spatial;
mod surface;
mod world_map;

pub use config::{BiomeTerrain, WorldGenConfig};
pub use geometry::Point;
pub use overlay::Stamp;
pub use regions::{BiomeWeights, RegionSite};
pub use rivers::{RiverPoint, RiverSegment};
pub use roads::RoadSegment;
pub use settlements::Settlement;
pub use world_map::WorldMap;

use noise_fields::{NoiseFields, lerp};
use overlay::Overlay;
use regions::Regions;
use venture_world::{Chunk, ChunkPos, Ground};

/// The version of the generator. Increase it whenever a change alters the
/// generated world, so worlds baked by an older version are noticed.
pub const GENERATOR_VERSION: u32 = 1;

/// How far a settlement's levelled ground blends into the land around it, in metres.
const SETTLEMENT_FALLOFF: f32 = 35.0;

/// A settlement's square, as a fraction of its radius.
const SETTLEMENT_SQUARE: f32 = 0.3;

/// Generates a world from a [`WorldGenConfig`] and any authored overlay stamps.
pub struct WorldGenerator {
    config: WorldGenConfig,
    noise: NoiseFields,
    regions: Regions,
    map: WorldMap,
    overlay: Overlay,
}

impl WorldGenerator {
    /// Creates a purely procedural world.
    pub fn new(config: WorldGenConfig) -> Self {
        Self::with_authored_stamps(config, Vec::new())
    }

    /// Creates a world with authored stamps applied on top of the procedural
    /// terrain (after the generator's own stamps). This builds the coarse
    /// world map, which is the slow part.
    pub fn with_authored_stamps(config: WorldGenConfig, authored: Vec<Stamp>) -> Self {
        let noise = NoiseFields::new(&config);
        let regions = Regions::new(&config);
        let map = WorldMap::generate(&config, &noise, &regions);

        let mut stamps = settlement_stamps(&config, map.settlements());
        stamps.extend(authored);
        let overlay = Overlay::new(config.world_size as f32, stamps, |(x, y)| map.land_height_at(x, y));

        Self { config, noise, regions, map, overlay }
    }

    /// The settings this world was generated with.
    pub fn config(&self) -> &WorldGenConfig {
        &self.config
    }

    /// The coarse whole-world map.
    pub fn map(&self) -> &WorldMap {
        &self.map
    }

    /// How much a point belongs to each biome.
    pub fn biome_weights(&self, x: f32, y: f32) -> BiomeWeights {
        self.regions.weights_at(x, y)
    }

    /// Every biome region's centre.
    pub fn region_sites(&self) -> &[RegionSite] {
        self.regions.sites()
    }

    /// Generates the chunk at `pos`.
    pub fn generate_chunk(&self, pos: ChunkPos) -> Chunk {
        chunk_gen::generate_chunk(self, pos)
    }

    /// Whether the chunk at `pos` lies inside the world.
    pub fn contains_chunk(&self, pos: ChunkPos) -> bool {
        let (x, y) = pos.origin_cell();
        let size = self.config.world_size;
        (0..size).contains(&x) && (0..size).contains(&y)
    }

    /// A good place to start exploring: the settlement closest to the centre
    /// of the world, in metres.
    pub fn spawn_point(&self) -> Point {
        let centre = self.config.world_size as f32 / 2.0;
        let distance_to_centre = |&(x, y): &Point| (x - centre).hypot(y - centre);
        self.map
            .settlements()
            .iter()
            .map(|settlement| settlement.position)
            .min_by(|a, b| distance_to_centre(a).total_cmp(&distance_to_centre(b)))
            .unwrap_or((centre, centre))
    }

    /// Ground height at any point, interpolated between cell corners.
    /// Matches the heights stored in generated chunks.
    pub fn ground_height(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let features = chunk_gen::LocalFeatures::near(self, (x0, y0), (x0 + 1.0, y0 + 1.0));
        let corner = |cx: f32, cy: f32| chunk_gen::corner_height(self, &features, cx, cy);
        let (fx, fy) = (x - x0, y - y0);
        let top = lerp(corner(x0, y0), corner(x0 + 1.0, y0), fx);
        let bottom = lerp(corner(x0, y0 + 1.0), corner(x0 + 1.0, y0 + 1.0), fx);
        lerp(top, bottom, fy)
    }
}

/// The generator's own stamps: level ground and a square for each settlement.
fn settlement_stamps(config: &WorldGenConfig, settlements: &[Settlement]) -> Vec<Stamp> {
    settlements
        .iter()
        .flat_map(|settlement| {
            [
                Stamp::Flatten {
                    centre: settlement.position,
                    radius: config.settlement_radius,
                    falloff: SETTLEMENT_FALLOFF,
                    height: None,
                },
                Stamp::Paint {
                    centre: settlement.position,
                    radius: config.settlement_radius * SETTLEMENT_SQUARE,
                    ground: Ground::Path,
                },
            ]
        })
        .collect()
}
