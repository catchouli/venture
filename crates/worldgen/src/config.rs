//! Every tunable number used by world generation, in one place.
//!
//! Distances and heights are in metres. Frequencies are in cycles per metre,
//! so `1.0 / 500.0` means features roughly 500 metres across.
//! See `docs/terrain-generation.md`.

use venture_world::Biome;

/// How many metres above a biome's `rock_start_height` the ground is all rock.
pub const ROCK_TRANSITION: f32 = 30.0;

/// Settings for generating a world. The same config always produces the same world.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGenConfig {
    /// Random seed. Changing it produces a completely different world.
    pub seed: u32,
    /// The world covers `0..world_size` metres on both horizontal axes.
    pub world_size: i32,
    /// Spacing of the coarse world map used for rivers, lakes and roads, in metres.
    /// Must divide `world_size` evenly.
    pub map_cell_size: i32,
    /// Height of the sea surface.
    pub sea_level: f32,

    /// Average distance between the centres of biome regions.
    pub region_spacing: f32,
    /// How wide the blend between neighbouring regions is.
    pub region_blend: f32,
    /// How far region borders are pushed around, so they aren't straight lines.
    pub region_warp: f32,
    /// Terrain settings for each biome.
    pub grassland: BiomeTerrain,
    /// See [`Self::grassland`].
    pub meadow: BiomeTerrain,
    /// See [`Self::grassland`].
    pub highlands: BiomeTerrain,
    /// See [`Self::grassland`].
    pub volcanic: BiomeTerrain,

    /// Frequency of the large-scale land/sea noise.
    pub continent_frequency: f64,
    /// How far the continent noise raises or lowers the land.
    pub continent_height: f32,
    /// Average land height before biome and other features are added.
    pub land_base_height: f32,
    /// Width of the band along the world's edges that slopes down into ocean.
    pub edge_falloff_width: f32,
    /// Height of the sea floor at the very edge of the world.
    pub ocean_floor_height: f32,

    /// Frequency of the ridged noise that shapes individual mountains.
    pub mountain_frequency: f64,
    /// Height of the tallest mountains, in a biome with full mountain amount.
    pub mountain_height: f32,
    /// Frequency of the noise that varies mountain height within a region.
    pub mountain_variation_frequency: f64,
    /// Frequency of the noise deciding which areas are hilly and which are flat.
    pub hilliness_frequency: f64,
    /// Frequency of the hills themselves.
    pub hill_frequency: f64,
    /// Height of hills, in a biome with full hill amount.
    pub hill_height: f32,
    /// Frequency of the fine relief (bumps, dips and mounds) on every slope.
    pub relief_frequency: f64,
    /// Height of the fine relief, in a biome with full relief amount.
    pub relief_height: f32,

    /// Height of a volcano's cone above the surrounding land.
    pub volcano_height: f32,
    /// Radius of a volcano's base.
    pub volcano_radius: f32,
    /// Radius of the crater at the top.
    pub crater_radius: f32,
    /// Depth of the crater.
    pub crater_depth: f32,

    /// Minimum depth, below its spill height, for a basin to hold a lake.
    pub lake_min_depth: f32,
    /// Minimum number of map cells a lake must cover. Smaller basins are filled in.
    pub lake_min_cells: usize,

    /// Number of upstream map cells that must drain through a cell for it to carry a river.
    pub river_min_flow: u32,
    /// Flow at which a river reaches its full width and depth.
    pub river_full_flow: u32,
    /// Half the width of the smallest river.
    pub river_min_half_width: f32,
    /// Half the width of the largest river.
    pub river_max_half_width: f32,
    /// Depth of the smallest river's channel below its water surface.
    pub river_min_depth: f32,
    /// Depth of the largest river's channel below its water surface.
    pub river_max_depth: f32,
    /// Width of the flat land either side of the largest rivers.
    pub floodplain_width: f32,
    /// How far above the water surface floodplains sit.
    pub bank_height: f32,

    /// Dry ground up to this height above nearby water can become beach.
    pub beach_height: f32,
    /// How far beaches reach from lakes and the sea, in cells.
    pub beach_width: i32,
    /// How far sandy banks reach from rivers, in cells.
    pub river_bank_width: i32,
    /// Land near lakes and the sea is graded to rise at most this much per metre.
    pub shore_slope: f32,
    /// How far from lakes and the sea the grading reaches.
    pub shore_reach: f32,
    /// Cells whose corners differ in height by more than this are bare rock.
    pub cliff_slope: f32,
    /// Height above which peaks are covered in snow (except in volcanic regions).
    pub snow_height: f32,
    /// Frequency of the patchy noise used to break up the edges between materials.
    pub patch_frequency: f64,

    /// Minimum distance between settlements.
    pub settlement_spacing: f32,
    /// Radius of the levelled ground a settlement is built on.
    pub settlement_radius: f32,
    /// Half the width of a road.
    pub road_half_width: f32,
    /// Width of the levelled verge either side of a road.
    pub road_verge_width: f32,
    /// Extra cost of building roads up slopes, per metre climbed per metre travelled.
    pub road_slope_cost: f32,
    /// Extra cost of a road crossing a river, in metres of flat road.
    pub road_river_crossing_cost: f32,
    /// Cost multiplier for reusing an existing road, so roads merge into networks.
    pub road_reuse_discount: f32,

    /// Spacing of the grid that props (trees, rocks, bushes) are scattered on.
    pub prop_spacing: f32,
    /// Frequency of the noise that groups trees into woods and clearings.
    pub forest_frequency: f64,
    /// The species that grows by water (weeping willow), and the share of
    /// trees near water that are it.
    pub waterside_tree: &'static str,
    /// (See [`WorldGenConfig::waterside_tree`].)
    pub waterside_share: f32,
    /// Of the spots where a tree could grow, the share that gets one. Real
    /// trees need room (a mature oak's crown is 15 to 20 m across), so a
    /// wood has a tree every 8 to 10 m, not every spot on the grid.
    pub tree_share: f32,
}

/// How a biome shapes and dresses the land. Neighbouring biomes blend their
/// numbers together across region borders.
#[derive(Clone, Debug, PartialEq)]
pub struct BiomeTerrain {
    /// Height added to (or removed from) the land.
    pub base_height: f32,
    /// How mountainous the biome is, from `0.0` to `1.0`.
    pub mountains: f32,
    /// How hilly the biome is, from `0.0` to `1.0`.
    pub hills: f32,
    /// How bumpy the fine relief is, relative to [`WorldGenConfig::relief_height`].
    pub relief: f32,
    /// Height where rock starts to appear among the soil. Everything is rock
    /// [`ROCK_TRANSITION`] metres higher.
    pub rock_start_height: f32,
    /// How much of the land is wooded, from `0.0` to `1.0`.
    pub forest_cover: f32,
    /// How many boulders are scattered around, from `0.0` to `1.0`.
    pub boulders: f32,
    /// How many bushes grow, from `0.0` to `1.0`.
    pub bushes: f32,
    /// Which tree species grow in its woods (ids in the content's
    /// `trees/species.ron`), with how common each is.
    pub trees: &'static [(&'static str, f32)],
    /// Which species its bushes are (they're young trees), with how common each is.
    pub bush_species: &'static [(&'static str, f32)],
    /// Whether regions of this biome have a volcano at their centre.
    pub volcanoes: bool,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            world_size: 4096,
            map_cell_size: 8,
            sea_level: 0.0,

            region_spacing: 850.0,
            region_blend: 90.0,
            region_warp: 220.0,
            grassland: BiomeTerrain {
                base_height: 0.0,
                mountains: 0.06,
                hills: 0.35,
                relief: 0.6,
                rock_start_height: 60.0,
                forest_cover: 0.3,
                boulders: 0.15,
                bushes: 0.4,
                trees: &[
                    ("oak", 4.0),
                    ("ash", 2.0),
                    ("beech", 1.5),
                    ("sycamore", 1.5),
                    ("hawthorn", 0.6),
                    ("dead_tree", 0.7),
                    ("yew", 0.2),
                ],
                bush_species: &[("hawthorn", 1.0), ("hazel", 1.0)],
                volcanoes: false,
            },
            meadow: BiomeTerrain {
                base_height: 4.0,
                mountains: 0.08,
                hills: 1.0,
                relief: 1.0,
                rock_start_height: 65.0,
                forest_cover: 0.45,
                boulders: 0.25,
                bushes: 0.8,
                trees: &[
                    ("beech", 3.0),
                    ("ash", 2.0),
                    ("oak", 2.0),
                    ("birch", 1.5),
                    ("sycamore", 1.0),
                    ("hazel", 0.5),
                    ("dead_tree", 0.7),
                ],
                bush_species: &[("hazel", 1.5), ("hawthorn", 1.0)],
                volcanoes: false,
            },
            highlands: BiomeTerrain {
                base_height: 22.0,
                mountains: 1.0,
                hills: 0.8,
                relief: 1.4,
                rock_start_height: 42.0,
                forest_cover: 0.4,
                boulders: 1.0,
                bushes: 0.2,
                trees: &[("scots_pine", 6.0), ("birch", 2.0), ("dead_tree", 0.8), ("yew", 0.2)],
                bush_species: &[("hazel", 1.0), ("birch", 0.5)],
                volcanoes: false,
            },
            volcanic: BiomeTerrain {
                base_height: 8.0,
                mountains: 0.35,
                hills: 0.5,
                relief: 1.1,
                rock_start_height: 25.0,
                forest_cover: 0.08,
                boulders: 0.7,
                bushes: 0.05,
                trees: &[("dead_tree", 1.0)],
                bush_species: &[("hawthorn", 1.0)],
                volcanoes: true,
            },

            continent_frequency: 1.0 / 2600.0,
            continent_height: 12.0,
            land_base_height: 8.0,
            edge_falloff_width: 800.0,
            ocean_floor_height: -20.0,

            mountain_frequency: 1.0 / 480.0,
            mountain_height: 90.0,
            mountain_variation_frequency: 1.0 / 900.0,
            hilliness_frequency: 1.0 / 900.0,
            hill_frequency: 1.0 / 170.0,
            hill_height: 14.0,
            relief_frequency: 1.0 / 40.0,
            relief_height: 2.2,

            volcano_height: 120.0,
            volcano_radius: 320.0,
            crater_radius: 50.0,
            crater_depth: 30.0,

            lake_min_depth: 2.5,
            lake_min_cells: 24,

            river_min_flow: 150,
            river_full_flow: 6000,
            river_min_half_width: 1.5,
            river_max_half_width: 8.0,
            river_min_depth: 0.8,
            river_max_depth: 2.5,
            floodplain_width: 30.0,
            bank_height: 0.4,

            beach_height: 2.5,
            beach_width: 20,
            river_bank_width: 3,
            shore_slope: 0.1,
            shore_reach: 28.0,
            cliff_slope: 1.4,
            snow_height: 105.0,
            patch_frequency: 1.0 / 14.0,

            settlement_spacing: 420.0,
            settlement_radius: 45.0,
            road_half_width: 2.0,
            road_verge_width: 3.0,
            road_slope_cost: 6.0,
            road_river_crossing_cost: 60.0,
            road_reuse_discount: 0.35,

            prop_spacing: 4.0,
            forest_frequency: 1.0 / 180.0,
            tree_share: 0.22,
            waterside_tree: "willow",
            waterside_share: 0.7,
        }
    }
}

impl WorldGenConfig {
    /// Number of coarse map points along each axis (one more than cells).
    pub fn map_points(&self) -> usize {
        (self.world_size / self.map_cell_size) as usize + 1
    }

    /// Every tree species the world can have (in a fixed order: tree props
    /// refer to them by their place in it).
    pub fn tree_species(&self) -> Vec<String> {
        let species: std::collections::BTreeSet<&str> =
            [&self.grassland, &self.meadow, &self.highlands, &self.volcanic]
                .into_iter()
                .flat_map(|biome| biome.trees.iter().chain(biome.bush_species))
                .map(|(id, _)| *id)
                .chain([self.waterside_tree])
                .collect();
        species.into_iter().map(str::to_string).collect()
    }

    /// The terrain settings for a biome.
    pub fn biome(&self, biome: Biome) -> &BiomeTerrain {
        match biome {
            Biome::Grassland => &self.grassland,
            Biome::Meadow => &self.meadow,
            Biome::Highlands => &self.highlands,
            Biome::Volcanic => &self.volcanic,
        }
    }
}
