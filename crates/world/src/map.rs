//! The world map, baked with the world (see `docs/plan-map.md`): pictures of
//! the land in square tiles at several levels of detail, and the names of
//! places to write over them.
//!
//! In a baked world folder:
//! - `map/{level}/{x}_{y}.jpg`: the tiles ([`map_tile_path`]). Level 0 is the
//!   most detailed ([`MapInfo::finest_metres_per_pixel`] metres a pixel);
//!   each level after covers twice the distance per pixel, up to the last,
//!   one tile for the whole world. Tile `(x, y)` starts `x` tiles east and
//!   `y` tiles south of the world's north-west corner.
//! - `map/labels.ron`: the place names ([`MapLabels`]).
//!
//! The manifest says whether a world has a map ([`crate::WorldManifest::map`]).

use serde::{Deserialize, Serialize};

/// Where the place names are, in a baked world folder.
pub const MAP_LABELS_FILE: &str = "map/labels.ron";

/// How a baked world's map is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapInfo {
    /// Width (and height) of a tile, in pixels.
    pub tile_pixels: u32,
    /// Metres a pixel covers at level 0.
    pub finest_metres_per_pixel: f32,
    /// How many levels there are (the last is one tile for the whole world).
    pub levels: u32,
}

impl MapInfo {
    /// The layout for a world `world_size` metres across: 256-pixel tiles,
    /// a metre a pixel at level 0, and levels down to one tile.
    pub fn for_world(world_size: i32) -> Self {
        let tile_pixels = 256;
        let finest_metres_per_pixel = 1.0;
        let tiles = (world_size as f32 / (tile_pixels as f32 * finest_metres_per_pixel)).ceil().max(1.0) as u32;
        // Level L has ceil(tiles / 2^L) tiles a side; the last has one.
        let levels = tiles.next_power_of_two().trailing_zeros() + 1;
        Self { tile_pixels, finest_metres_per_pixel, levels }
    }

    /// Metres a pixel covers at a level.
    pub fn metres_per_pixel(&self, level: u32) -> f32 {
        self.finest_metres_per_pixel * (1u32 << level) as f32
    }

    /// Metres a tile covers at a level.
    pub fn tile_metres(&self, level: u32) -> f32 {
        self.metres_per_pixel(level) * self.tile_pixels as f32
    }

    /// How many tiles a side a level has, for a world `world_size` metres across.
    pub fn tiles_per_side(&self, level: u32, world_size: i32) -> u32 {
        (world_size as f32 / self.tile_metres(level)).ceil().max(1.0) as u32
    }
}

/// The path of a map tile, relative to the baked world folder.
pub fn map_tile_path(level: u32, x: u32, y: u32) -> String {
    format!("map/{level}/{x}_{y}.jpg")
}

/// Every place name on the map.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapLabels {
    /// The names, biggest kinds first.
    pub labels: Vec<MapLabel>,
}

/// A name written on the map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapLabel {
    /// The name.
    pub name: String,
    /// What kind of place it is, which decides how it's written and at
    /// which zooms it shows.
    pub kind: LabelKind,
    /// Where it's written (its middle), in metres.
    pub at: (f32, f32),
}

/// What kind of place a label names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LabelKind {
    /// A whole region (a stretch of one kind of land, a sea): shown when
    /// zoomed out, in large spaced capitals.
    Region,
    /// A local area, such as a town: shown at middle zooms.
    Area,
    /// A small place (a village, a lake, a wood, later dungeons): shown
    /// when zoomed in.
    Place,
}

impl MapLabels {
    /// Reads place names from RON text.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|error| error.to_string())
    }

    /// Writes the place names as RON text.
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("labels always serialise")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_go_down_to_one_tile() {
        let map = MapInfo::for_world(4096);
        assert_eq!(map.levels, 5);
        assert_eq!(map.tiles_per_side(0, 4096), 16);
        assert_eq!(map.tiles_per_side(4, 4096), 1);
        assert_eq!(map.metres_per_pixel(4), 16.0);
        // A world that isn't a power of two still ends at one tile.
        let odd = MapInfo::for_world(3000);
        assert_eq!(odd.tiles_per_side(odd.levels - 1, 3000), 1);
        assert_eq!(odd.tiles_per_side(odd.levels - 2, 3000), 2);
    }

    #[test]
    fn labels_survive_a_round_trip() {
        let labels = MapLabels {
            labels: vec![MapLabel { name: "Millbrook".to_string(), kind: LabelKind::Area, at: (10.0, 20.0) }],
        };
        assert_eq!(MapLabels::from_ron(&labels.to_ron()).unwrap(), labels);
    }
}
