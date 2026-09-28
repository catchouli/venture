//! Bakes a world to disk: generates every chunk, draws the world map from
//! them, and writes the world folder the server loads (see
//! `venture_world::WorldManifest`). (`bake_map` draws the map again for a
//! world already baked.)
//!
//! ```text
//! cargo run --release -p venture_worldgen --bin bake -- [output folder] [seed]
//! ```
//!
//! The output folder defaults to `world`, and the seed to the default config's.
//!
//! Authored world data (`world_authoring/`, see `docs/world-authoring.md`)
//! is baked in too: walls and fences (`walls.ron`, see `docs/plan-walls.md`)
//! go in the manifest, block the cells they stand on, and clear the trees and
//! rocks in their way.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use venture_structures::{WALLS_FILE, bake_wall, cells_near, parse_walls, wall_cells};
use venture_world::{
    BakedWall, CHUNK_FORMAT_VERSION, CHUNK_SIZE, CellPos, Chunk, ChunkPos, MANIFEST_FILE, SettlementInfo,
    WorldManifest, chunk_file_path, encode_chunk,
};
use venture_worldgen::map_render::bake_map;
use venture_worldgen::{GENERATOR_VERSION, WorldGenConfig, WorldGenerator};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(args.next().unwrap_or_else(|| "world".to_string()));
    let mut config = WorldGenConfig::default();
    if let Some(seed) = args.next() {
        config.seed = seed.parse().expect("the seed must be a whole number");
    }

    let started = Instant::now();
    let world = WorldGenerator::new(config);
    println!("World map built in {:.2?}.", started.elapsed());

    let manifest = WorldManifest {
        world_size: world.config().world_size,
        chunk_size: CHUNK_SIZE,
        chunk_format: CHUNK_FORMAT_VERSION,
        generator_version: GENERATOR_VERSION,
        seed: world.config().seed,
        spawn_point: world.spawn_point(),
        settlements: world
            .map()
            .settlements()
            .iter()
            .map(|settlement| SettlementInfo { position: settlement.position, biome: settlement.biome })
            .collect(),
        map: None,
        tree_species: world.config().tree_species(),
        walls: Vec::new(),
    };
    let mut manifest = manifest;
    manifest.walls = bake_walls(&world, Path::new(AUTHORING_FOLDER));
    let structures = Structures::of(&manifest.walls);
    std::fs::create_dir_all(output.join("chunks"))?;

    let started = Instant::now();
    bake_chunks(&world, &manifest, &structures, &output)?;
    println!("{} chunks baked in {:.2?}.", manifest.chunks_per_side().pow(2), started.elapsed());

    // The map is drawn from the chunks just written.
    let started = Instant::now();
    manifest.map = Some(bake_map(&output, &manifest)?);
    println!("Map drawn in {:.2?}.", started.elapsed());

    // The manifest is written last, so a half-finished bake is never mistaken for a complete one.
    std::fs::write(output.join(MANIFEST_FILE), manifest.to_ron())?;
    println!("World written to {}.", output.display());
    Ok(())
}

/// The authored world data's folder.
const AUTHORING_FOLDER: &str = "world_authoring";
/// Trees and rocks this near a wall's line (beyond its own width) are cleared, in metres.
const WALL_CLEARANCE: f32 = 1.5;
/// Walls open where a road passes within this much of its edge, in metres.
const ROAD_GAP_MARGIN: f32 = 0.5;

/// The authored walls, baked (none if there's no walls file).
fn bake_walls(world: &WorldGenerator, folder: &Path) -> Vec<BakedWall> {
    let path = folder.join(WALLS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    let defs = parse_walls(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let road = world.config().road_half_width + ROAD_GAP_MARGIN;
    let walls: Vec<BakedWall> = defs
        .iter()
        .map(|def| bake_wall(def, |x, y| world.ground_height(x, y), |x, y| world.road_distance(x, y, road).is_some()))
        .collect();
    println!("{} walls and fences baked from {}.", walls.len(), path.display());
    walls
}

/// What the walls do to each chunk: the cells they block, and the cells
/// they clear of trees and rocks.
#[derive(Default)]
struct Structures {
    blocked: HashMap<ChunkPos, Vec<u16>>,
    cleared: HashMap<ChunkPos, HashSet<u16>>,
}

impl Structures {
    fn of(walls: &[BakedWall]) -> Self {
        let mut structures = Self::default();
        // A world cell as its chunk and its index in it.
        let place = |(x, y): (i32, i32)| {
            let cell = CellPos::new(x, y);
            let (local_x, local_y) = cell.local();
            (cell.chunk(), (local_y * CHUNK_SIZE as usize + local_x) as u16)
        };
        for wall in walls {
            for cell in wall_cells(wall) {
                let (chunk, index) = place(cell);
                structures.blocked.entry(chunk).or_default().push(index);
            }
            for cell in cells_near(wall, wall.kind.half_width() + WALL_CLEARANCE) {
                let (chunk, index) = place(cell);
                structures.cleared.entry(chunk).or_default().insert(index);
            }
        }
        for cells in structures.blocked.values_mut() {
            cells.sort_unstable();
            cells.dedup();
        }
        structures
    }

    /// Puts the walls' effects into a freshly generated chunk.
    fn apply(&self, chunk: &mut Chunk) {
        if let Some(cells) = self.blocked.get(&chunk.pos) {
            chunk.structure_cells = cells.clone();
        }
        if let Some(cleared) = self.cleared.get(&chunk.pos) {
            let (origin_x, origin_y) = chunk.pos.origin_cell();
            chunk.props.retain(|prop| {
                let (x, y) = ((prop.x - origin_x as f32) as usize, (prop.y - origin_y as f32) as usize);
                !cleared.contains(&((y.min(CHUNK_SIZE as usize - 1) * CHUNK_SIZE as usize + x) as u16))
            });
        }
    }
}

/// Generates and writes every chunk, spreading the work over all CPU cores.
fn bake_chunks(
    world: &WorldGenerator,
    manifest: &WorldManifest,
    structures: &Structures,
    output: &Path,
) -> std::io::Result<()> {
    let chunks = manifest.chunks_per_side();
    let total = (chunks * chunks) as usize;
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get());

    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> std::io::Result<()> {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= total {
                            return Ok(());
                        }
                        let pos = ChunkPos::new(index as i32 % chunks, index as i32 / chunks);
                        let mut chunk = world.generate_chunk(pos);
                        structures.apply(&mut chunk);
                        let bytes = encode_chunk(&chunk);
                        std::fs::write(output.join(chunk_file_path(pos)), bytes)?;
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|worker| worker.join().expect("a bake worker panicked"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use venture_world::{Prop, PropKind, WallKind, WallSample};

    #[test]
    fn walls_block_their_cells_and_clear_props_from_their_way() {
        // A straight wall along y = 10.5, from x = 2 to x = 20, in the first chunk.
        let samples =
            (0..=36).map(|i| WallSample { x: 2.0 + i as f32 * 0.5, y: 10.5, ground: 0.0, open: false }).collect();
        let wall = BakedWall { id: "w".into(), kind: WallKind::DryStone, height: 1.3, seed: 0, samples };
        let structures = Structures::of(&[wall]);
        let boulder =
            |x: f32, y: f32| Prop { kind: PropKind::Boulder, x, y, ground_height: 0.0, scale: 1.0, rotation: 0.0 };
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.props = vec![boulder(10.2, 11.3), boulder(10.2, 20.5)];
        structures.apply(&mut chunk);
        let index = |x: usize, y: usize| (y * CHUNK_SIZE as usize + x) as u16;
        assert!(chunk.structure_cells.contains(&index(10, 10)));
        assert!(!chunk.structure_cells.contains(&index(10, 13)));
        assert_eq!(chunk.props, vec![boulder(10.2, 20.5)], "only the boulder in the wall's way goes");
    }
}
