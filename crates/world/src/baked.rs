//! The baked world: the finished world written to disk ahead of time, so the
//! server and clients use exactly the same terrain.
//!
//! A baked world is a folder containing:
//! - `manifest.ron`: a [`WorldManifest`] describing the world,
//! - `chunks/{x}_{y}.bin`: one file per chunk (see [`encode_chunk`]),
//! - `map/`: the world map, if it has one (see [`crate::MapInfo`]).
//!
//! Heights and liquid levels are stored as whole centimetres (`i16`, so from
//! -327 m to 327 m), which keeps chunk files small. Everything else is stored
//! exactly.

use serde::{Deserialize, Serialize};

use crate::{
    Biome, CHUNK_CORNERS, CHUNK_SIZE, Chunk, ChunkPos, Fluid, FluidBody, FluidKind, Ground, Prop, TerrainCell,
};

/// The version of the chunk file layout. Increase it whenever the layout changes.
pub const CHUNK_FORMAT_VERSION: u16 = 4;

/// Name of the manifest file in a baked world folder.
pub const MANIFEST_FILE: &str = "manifest.ron";

/// Description of a baked world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldManifest {
    /// The world covers `0..world_size` metres on both horizontal axes.
    pub world_size: i32,
    /// Chunk width in cells. Must match [`CHUNK_SIZE`].
    pub chunk_size: i32,
    /// Version of the chunk file layout the world was written with.
    pub chunk_format: u16,
    /// Version of the generator that made the world.
    pub generator_version: u32,
    /// The generator's seed.
    pub seed: u32,
    /// Where new characters start, in metres.
    pub spawn_point: (f32, f32),
    /// The settlement sites, best first. Content places things relative to
    /// them (see `docs/content-authoring.md`).
    #[serde(default)]
    pub settlements: Vec<SettlementInfo>,
    /// The world map's layout, if the world has one (worlds baked before
    /// maps don't: `bake_map` makes one).
    #[serde(default)]
    pub map: Option<crate::MapInfo>,
    /// The tree species the world's trees are (ids of the content's
    /// `trees/species.ron`), which [`crate::PropKind::Tree`] refers to by index.
    #[serde(default)]
    pub tree_species: Vec<String>,
    /// The world's walls and fences (authored, then baked: see `docs/plan-walls.md`).
    #[serde(default)]
    pub walls: Vec<crate::BakedWall>,
    /// The world's buildings (authored, then baked: see `docs/plan-buildings.md`).
    #[serde(default)]
    pub buildings: Vec<crate::BakedBuilding>,
}

/// A settlement site in a baked world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettlementInfo {
    /// Centre of the settlement, in metres.
    pub position: (f32, f32),
    /// The biome it's in.
    pub biome: Biome,
}

impl WorldManifest {
    /// Number of chunks along each axis.
    pub fn chunks_per_side(&self) -> i32 {
        self.world_size / self.chunk_size
    }

    /// Whether the chunk at `pos` is part of the world.
    pub fn contains_chunk(&self, pos: ChunkPos) -> bool {
        let chunks = self.chunks_per_side();
        (0..chunks).contains(&pos.x) && (0..chunks).contains(&pos.y)
    }

    /// The settlement new characters start in: the one at the spawn point.
    pub fn starting_settlement(&self) -> Option<&SettlementInfo> {
        let (x, y) = self.spawn_point;
        self.settlements.iter().min_by(|a, b| {
            let distance = |s: &SettlementInfo| (s.position.0 - x).hypot(s.position.1 - y);
            distance(a).total_cmp(&distance(b))
        })
    }

    /// Reads a manifest from RON text.
    pub fn from_ron(text: &str) -> Result<Self, BakedWorldError> {
        let manifest: Self = ron::from_str(text).map_err(|error| BakedWorldError::Manifest(error.to_string()))?;
        if manifest.chunk_size != CHUNK_SIZE || manifest.chunk_format != CHUNK_FORMAT_VERSION {
            return Err(BakedWorldError::Incompatible);
        }
        Ok(manifest)
    }

    /// Writes the manifest as RON text.
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("a manifest always serialises")
    }
}

/// The path of a chunk's file, relative to the baked world folder.
pub fn chunk_file_path(pos: ChunkPos) -> String {
    format!("chunks/{}_{}.bin", pos.x, pos.y)
}

/// Something wrong with baked world data.
#[derive(Debug, Clone, PartialEq)]
pub enum BakedWorldError {
    /// The manifest couldn't be read.
    Manifest(String),
    /// The world was baked with a different chunk size or file layout.
    Incompatible,
    /// A chunk file couldn't be decoded.
    Chunk(String),
}

impl std::fmt::Display for BakedWorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BakedWorldError::Manifest(error) => write!(f, "can't read the world manifest: {error}"),
            BakedWorldError::Incompatible => write!(f, "the world was baked by an incompatible version; bake it again"),
            BakedWorldError::Chunk(error) => write!(f, "can't read a chunk file: {error}"),
        }
    }
}

impl std::error::Error for BakedWorldError {}

/// A chunk as stored on disk.
#[derive(Serialize, Deserialize)]
struct ChunkFile {
    format: u16,
    pos: ChunkPos,
    heights: Vec<i16>,
    cells: Vec<CellRecord>,
    props: Vec<Prop>,
    structure_cells: Vec<u16>,
}

#[derive(Serialize, Deserialize)]
struct CellRecord {
    ground: Ground,
    biome: Biome,
    fluid: Option<(FluidKind, FluidBody, i16)>,
}

/// Metres to whole centimetres, clamped to what an `i16` can hold.
fn to_centimetres(metres: f32) -> i16 {
    (metres * 100.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

fn to_metres(centimetres: i16) -> f32 {
    centimetres as f32 / 100.0
}

/// Encodes a chunk as the bytes of its file.
pub fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let corners = CHUNK_CORNERS as usize;
    let size = CHUNK_SIZE as usize;
    let heights =
        (0..corners * corners).map(|i| to_centimetres(chunk.corner_height(i % corners, i / corners))).collect();
    let cells = (0..size * size)
        .map(|i| {
            let cell = chunk.cell(i % size, i / size);
            CellRecord {
                ground: cell.ground,
                biome: cell.biome,
                fluid: cell.fluid.map(|fluid| (fluid.kind, fluid.body, to_centimetres(fluid.level))),
            }
        })
        .collect();
    let file = ChunkFile {
        format: CHUNK_FORMAT_VERSION,
        pos: chunk.pos,
        heights,
        cells,
        props: chunk.props.clone(),
        structure_cells: chunk.structure_cells.clone(),
    };
    postcard::to_stdvec(&file).expect("a chunk always serialises")
}

/// Decodes a chunk from the bytes of its file.
pub fn decode_chunk(bytes: &[u8]) -> Result<Chunk, BakedWorldError> {
    let file: ChunkFile = postcard::from_bytes(bytes).map_err(|error| BakedWorldError::Chunk(error.to_string()))?;
    let (corners, size) = (CHUNK_CORNERS as usize, CHUNK_SIZE as usize);
    if file.format != CHUNK_FORMAT_VERSION {
        return Err(BakedWorldError::Incompatible);
    }
    if file.heights.len() != corners * corners || file.cells.len() != size * size {
        return Err(BakedWorldError::Chunk("wrong number of heights or cells".to_string()));
    }
    let mut chunk = Chunk::new(file.pos);
    for (i, &height) in file.heights.iter().enumerate() {
        chunk.set_corner_height(i % corners, i / corners, to_metres(height));
    }
    for (i, record) in file.cells.into_iter().enumerate() {
        *chunk.cell_mut(i % size, i / size) = TerrainCell {
            ground: record.ground,
            biome: record.biome,
            fluid: record.fluid.map(|(kind, body, level)| Fluid { kind, body, level: to_metres(level) }),
        };
    }
    chunk.props = file.props;
    chunk.structure_cells = file.structure_cells;
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PropKind;

    #[test]
    fn chunks_survive_a_round_trip_to_the_centimetre() {
        let mut chunk = Chunk::new(ChunkPos::new(3, -2));
        chunk.set_corner_height(5, 7, 12.3456);
        chunk.cell_mut(1, 2).fluid = Some(Fluid { kind: FluidKind::Lava, body: FluidBody::Lake, level: 40.004 });
        chunk.cell_mut(1, 2).ground = Ground::Basalt;
        let kind = PropKind::Tree { species: 2, variant: 1 };
        chunk.props.push(Prop { kind, x: 1.5, y: 2.5, ground_height: 3.0, scale: 1.1, rotation: 0.4 });

        let decoded = decode_chunk(&encode_chunk(&chunk)).unwrap();
        assert_eq!(decoded.pos, chunk.pos);
        assert!((decoded.corner_height(5, 7) - 12.3456).abs() <= 0.005);
        assert_eq!(decoded.cell(1, 2).ground, Ground::Basalt);
        assert!((decoded.cell(1, 2).fluid.unwrap().level - 40.004).abs() <= 0.005);
        assert_eq!(decoded.cell(1, 2).fluid.unwrap().body, FluidBody::Lake);
        assert_eq!(decoded.props, chunk.props);
        assert_eq!(decoded.structure_cells, chunk.structure_cells);
    }

    #[test]
    fn manifests_survive_a_round_trip() {
        let manifest = WorldManifest {
            world_size: 4096,
            chunk_size: CHUNK_SIZE,
            chunk_format: CHUNK_FORMAT_VERSION,
            generator_version: 1,
            seed: 7,
            spawn_point: (10.0, 20.0),
            settlements: vec![SettlementInfo { position: (10.0, 20.0), biome: Biome::Meadow }],
            map: Some(crate::MapInfo::for_world(4096)),
            tree_species: Vec::new(),
            walls: Vec::new(),
            buildings: Vec::new(),
        };
        assert_eq!(WorldManifest::from_ron(&manifest.to_ron()).unwrap(), manifest);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(decode_chunk(&[1, 2, 3]).is_err());
    }
}
