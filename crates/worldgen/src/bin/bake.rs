//! Bakes a world to disk: generates every chunk, draws the world map from
//! them, and writes the world folder the server loads (see
//! `venture_world::WorldManifest`). (`bake_map` draws the map again for a
//! world already baked.)
//!
//! ```text
//! cargo run --release -p venture_worldgen --bin bake -- [--walls] [output folder] [seed] [authoring folder]
//! ```
//!
//! The output folder defaults to `world`, the seed to the default config's,
//! and the authoring folder to `world_authoring`.
//!
//! With `--walls`, only the chunks walls touch (as they were, and as they
//! are now) are baked again, and the map redrawn: about a second, rather than
//! several. It bakes everything if the world there was baked differently
//! (another seed, generator or chunk format), or isn't there.
//!
//! Authored world data (`world_authoring/`, see `docs/world-authoring.md`)
//! is baked in too: walls and fences (`walls.ron`, see `docs/plan-walls.md`)
//! go in the manifest, and clear the trees and rocks in their way.
//!
//! Last, each chunk's navigation mesh tile is made (see
//! `docs/plan-collision.md`) from the solid shapes of it and the chunks round
//! it, as written: those of every chunk baked, and of their neighbours.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use venture_collision::Collision;
use venture_navigation::build_tile;
use venture_structures::buildings::{
    BUILDINGS_FILE, BuildingStyle, STYLES_FILE, bake_building, building_clearing, cell_bounds, check_plan, levelled,
    parse_buildings, parse_styles,
};
use venture_structures::{WALLS_FILE, cells_near, check_walls, parse_walls};
use venture_world::{
    BakedBuilding, BakedWall, CHUNK_FORMAT_VERSION, CHUNK_SIZE, CellPos, Chunk, ChunkPos, Ground, MANIFEST_FILE,
    SettlementInfo, WorldManifest, chunk_file_path, decode_chunk, encode_chunk,
};
use venture_worldgen::map_render::bake_map;
use venture_worldgen::structures::bake_authored_wall;
use venture_worldgen::{GENERATOR_VERSION, WorldGenConfig, WorldGenerator};

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::io::Result<()> {
    let (flags, positional): (Vec<String>, Vec<String>) =
        std::env::args().skip(1).partition(|arg| arg.starts_with("--"));
    // (`--walls` is the old name for `--structures`.)
    let only_structures = flags.iter().any(|flag| flag == "--structures" || flag == "--walls");
    let mut args = positional.into_iter();
    let output = PathBuf::from(args.next().unwrap_or_else(|| "world".to_string()));
    let mut config = WorldGenConfig::default();
    if let Some(seed) = args.next() {
        config.seed = seed.parse().expect("the seed must be a whole number");
    }
    let authoring = PathBuf::from(args.next().unwrap_or_else(|| AUTHORING_FOLDER.to_string()));

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
        buildings: Vec::new(),
    };
    let mut manifest = manifest;
    let styles = building_styles(Path::new(CONTENT_FOLDER));
    manifest.walls = bake_walls(&world, &authoring);
    manifest.buildings = bake_buildings(&world, &authoring, &styles);
    let structures = Structures::of(&manifest.walls, &manifest.buildings);
    std::fs::create_dir_all(output.join("chunks"))?;

    // Only the chunks walls and buildings touch, if asked and the world there matches.
    let only = only_structures.then(|| previous_structures(&output, &manifest)).flatten().map(|(walls, buildings)| {
        let mut chunks = structures.chunks();
        chunks.extend(Structures::of(&walls, &buildings).chunks());
        chunks.sort_unstable_by_key(|pos| (pos.y, pos.x));
        chunks.dedup();
        chunks
    });
    if only_structures && only.is_none() {
        println!("The world in {} doesn't match (or isn't there), so all of it is baked.", output.display());
    }

    let started = Instant::now();
    let all: Vec<ChunkPos> = {
        let chunks = manifest.chunks_per_side();
        (0..chunks * chunks).map(|i| ChunkPos::new(i % chunks, i / chunks)).collect()
    };
    let positions = only.unwrap_or(all);
    bake_chunks(&world, &positions, &structures, &output)?;
    println!("{} chunks baked in {:.2?}.", positions.len(), started.elapsed());

    let started = Instant::now();
    let tiles = with_neighbours(&positions, &manifest);
    bake_navmesh(&tiles, &manifest, &styles, &output)?;
    println!("{} navigation mesh tiles made in {:.2?}.", tiles.len(), started.elapsed());

    // The map is drawn from the chunks just written.
    let started = Instant::now();
    manifest.map = Some(bake_map(&output, &manifest)?);
    println!("Map drawn in {:.2?}.", started.elapsed());

    // The manifest is written last, so a half-finished bake is never mistaken for a complete one.
    std::fs::write(output.join(MANIFEST_FILE), manifest.to_ron())?;
    println!("World written to {}.", output.display());
    Ok(())
}

/// The walls and buildings of the world already baked in `folder`, if it
/// was baked the same way as `manifest` (so only the chunks they touch need
/// baking again).
fn previous_structures(folder: &Path, manifest: &WorldManifest) -> Option<(Vec<BakedWall>, Vec<BakedBuilding>)> {
    let text = std::fs::read_to_string(folder.join(MANIFEST_FILE)).ok()?;
    let old = WorldManifest::from_ron(&text).ok()?;
    let same = old.seed == manifest.seed
        && old.world_size == manifest.world_size
        && old.chunk_format == manifest.chunk_format
        && old.generator_version == manifest.generator_version
        && old.tree_species == manifest.tree_species;
    same.then_some((old.walls, old.buildings))
}

/// The content's folder (for building styles).
const CONTENT_FOLDER: &str = "content";
/// Trees and rocks this near a building (beyond its walls) are cleared, in metres.
const BUILDING_CLEARANCE: f32 = 2.0;

/// The content's building styles (none if there are none).
fn building_styles(content: &Path) -> Vec<BuildingStyle> {
    let path = content.join(STYLES_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    parse_styles(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The authored buildings, baked (none if there's no buildings file). A
/// building with problems is left out, and said so.
fn bake_buildings(world: &WorldGenerator, folder: &Path, styles: &[BuildingStyle]) -> Vec<BakedBuilding> {
    let path = folder.join(BUILDINGS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    let plans = parse_buildings(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut buildings = Vec::new();
    for plan in &plans {
        let problems = check_plan(plan, styles.iter().find(|style| style.id == plan.style));
        if problems.is_empty() {
            buildings.push(bake_building(plan, |x, y| world.ground_height(x, y)));
        } else {
            println!("Left out: {}", problems.join("; "));
        }
    }
    println!("{} buildings baked from {}.", buildings.len(), path.display());
    buildings
}

/// The authored world data's folder (unless another is given).
const AUTHORING_FOLDER: &str = "world_authoring";
/// Trees and rocks this near a wall's line (beyond its own width) are cleared, in metres.
const WALL_CLEARANCE: f32 = 1.5;

/// The authored walls, baked (none if there's no walls file).
fn bake_walls(world: &WorldGenerator, folder: &Path) -> Vec<BakedWall> {
    let path = folder.join(WALLS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    let defs = parse_walls(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let problems = check_walls(&defs);
    if !problems.is_empty() {
        panic!("{}: {}", path.display(), problems.join("; "));
    }
    let walls: Vec<BakedWall> = defs.iter().map(|def| bake_authored_wall(world, def)).collect();
    println!("{} walls and fences baked from {}.", walls.len(), path.display());
    walls
}

/// What walls and buildings do to each chunk: the cells they clear of trees
/// and rocks (their own ground among them), and (buildings) the ground they
/// level. (What they block is their shapes: see `venture_collision`, and the
/// navigation mesh tiles made from them.)
#[derive(Default)]
struct Structures {
    cleared: HashMap<ChunkPos, HashSet<u16>>,
    /// The buildings levelling ground in each chunk.
    levelled: HashMap<ChunkPos, Vec<BakedBuilding>>,
}

impl Structures {
    fn of(walls: &[BakedWall], buildings: &[BakedBuilding]) -> Self {
        let mut structures = Self::default();
        // A world cell as its chunk and its index in it.
        let place = |(x, y): (i32, i32)| {
            let cell = CellPos::new(x, y);
            let (local_x, local_y) = cell.local();
            (cell.chunk(), (local_y * CHUNK_SIZE as usize + local_x) as u16)
        };
        let mut clear = |cell| {
            let (chunk, index) = place(cell);
            structures.cleared.entry(chunk).or_default().insert(index);
        };
        for wall in walls {
            cells_near(wall, wall.kind.half_width() + WALL_CLEARANCE).into_iter().for_each(&mut clear);
        }
        for building in buildings {
            building_clearing(building, BUILDING_CLEARANCE).into_iter().for_each(&mut clear);
            let ((x0, y0), (x1, y1)) = cell_bounds(building, 0.0);
            let (low, high) = (CellPos::new(x0, y0).chunk(), CellPos::new(x1 + 1, y1 + 1).chunk());
            for y in low.y..=high.y {
                for x in low.x..=high.x {
                    structures.levelled.entry(ChunkPos::new(x, y)).or_default().push(building.clone());
                }
            }
        }
        structures
    }

    /// The chunks walls and buildings touch.
    fn chunks(&self) -> Vec<ChunkPos> {
        self.cleared.keys().chain(self.levelled.keys()).copied().collect()
    }

    /// Puts the walls' and buildings' effects into a freshly generated chunk.
    fn apply(&self, chunk: &mut Chunk) {
        let (origin_x, origin_y) = chunk.pos.origin_cell();
        if let Some(cleared) = self.cleared.get(&chunk.pos) {
            chunk.props.retain(|prop| {
                let (x, y) = ((prop.x - origin_x as f32) as usize, (prop.y - origin_y as f32) as usize);
                !cleared.contains(&((y.min(CHUNK_SIZE as usize - 1) * CHUNK_SIZE as usize + x) as u16))
            });
        }
        // Level the ground under buildings (and blend it round them), and
        // make it trodden earth right round them.
        for building in self.levelled.get(&chunk.pos).into_iter().flatten() {
            for y in 0..=CHUNK_SIZE as usize {
                for x in 0..=CHUNK_SIZE as usize {
                    let (wx, wy) = ((origin_x + x as i32) as f32, (origin_y + y as i32) as f32);
                    let natural = chunk.corner_height(x, y);
                    if let Some(height) = levelled(building, wx, wy, natural) {
                        chunk.set_corner_height(x, y, height);
                    }
                }
            }
            for y in 0..CHUNK_SIZE as usize {
                for x in 0..CHUNK_SIZE as usize {
                    let (wx, wy) = ((origin_x + x as i32) as f32 + 0.5, (origin_y + y as i32) as f32 + 0.5);
                    if levelled(building, wx, wy, building.floor + 1.0).is_some_and(|h| h < building.floor + 0.6) {
                        chunk.cell_mut(x, y).ground = Ground::Path;
                    }
                }
            }
        }
    }
}

/// Generates and writes chunks, spreading the work over all CPU cores.
fn bake_chunks(
    world: &WorldGenerator,
    positions: &[ChunkPos],
    structures: &Structures,
    output: &Path,
) -> std::io::Result<()> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get());

    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> std::io::Result<()> {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&pos) = positions.get(index) else { return Ok(()) };
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

/// The chunks and their neighbours (in the world), in order.
fn with_neighbours(positions: &[ChunkPos], manifest: &WorldManifest) -> Vec<ChunkPos> {
    let mut all: Vec<ChunkPos> = positions
        .iter()
        .flat_map(|pos| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| ChunkPos::new(pos.x + dx, pos.y + dy))))
        .filter(|&pos| manifest.contains_chunk(pos))
        .collect();
    all.sort_unstable_by_key(|pos| (pos.y, pos.x));
    all.dedup();
    all
}

/// Reads a chunk already written.
fn read_chunk(output: &Path, pos: ChunkPos) -> std::io::Result<Chunk> {
    let bytes = std::fs::read(output.join(chunk_file_path(pos)))?;
    decode_chunk(&bytes).map_err(|error| std::io::Error::other(format!("chunk {pos:?}: {error}")))
}

/// Writes a chunk's file whole (so no one reading it sees half of it).
fn write_chunk(output: &Path, chunk: &Chunk) -> std::io::Result<()> {
    let path = output.join(chunk_file_path(chunk.pos));
    let partial = path.with_extension("partial");
    std::fs::write(&partial, encode_chunk(chunk))?;
    std::fs::rename(partial, path)
}

/// Makes the navigation mesh tiles of these chunks (already written), from
/// them and the chunks round them, with the walls and buildings near, and
/// writes them in, spreading the work over all CPU cores.
fn bake_navmesh(
    positions: &[ChunkPos],
    manifest: &WorldManifest,
    styles: &[BuildingStyle],
    output: &Path,
) -> std::io::Result<()> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get());
    // (Anything within this of a chunk's edge matters to its tile.)
    let near = |pos: ChunkPos, low: (f32, f32), high: (f32, f32)| {
        let (x, y) = pos.origin_cell();
        let (x, y, size, margin) = (x as f32, y as f32, CHUNK_SIZE as f32, 4.0);
        high.0 >= x - margin && low.0 <= x + size + margin && high.1 >= y - margin && low.1 <= y + size + margin
    };
    let wall_bounds = |wall: &BakedWall| {
        wall.samples.iter().fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(low, high), s| {
            ((low.0.min(s.x), low.1.min(s.y)), (high.0.max(s.x), high.1.max(s.y)))
        })
    };
    let building_bounds = |building: &BakedBuilding| {
        building.plan.outline.iter().fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(low, high), &(x, y)| {
            ((low.0.min(x), low.1.min(y)), (high.0.max(x), high.1.max(y)))
        })
    };
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> std::io::Result<()> {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&pos) = positions.get(index) else { return Ok(()) };
                        let mut solid = Collision::new();
                        let mut chunk = None;
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                let at = ChunkPos::new(pos.x + dx, pos.y + dy);
                                if !manifest.contains_chunk(at) {
                                    continue;
                                }
                                let read = read_chunk(output, at)?;
                                solid.add_chunk(&read);
                                if at == pos {
                                    chunk = Some(read);
                                }
                            }
                        }
                        let walls: Vec<BakedWall> = manifest
                            .walls
                            .iter()
                            .filter(|wall| {
                                let (low, high) = wall_bounds(wall);
                                near(pos, low, high)
                            })
                            .cloned()
                            .collect();
                        let buildings: Vec<BakedBuilding> = manifest
                            .buildings
                            .iter()
                            .filter(|building| {
                                let (low, high) = building_bounds(building);
                                near(pos, low, high)
                            })
                            .cloned()
                            .collect();
                        solid.set_structures(&walls, &buildings, styles);
                        let mut chunk = chunk.expect("the chunk itself is in the world");
                        chunk.navmesh = build_tile(&solid, pos);
                        write_chunk(output, &chunk)?;
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|worker| worker.join().expect("a navigation mesh worker panicked"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use venture_world::{Prop, PropKind, WallKind, WallSample};

    #[test]
    fn walls_clear_props_from_their_way() {
        // A straight wall along y = 10.5, from x = 2 to x = 20, in the first chunk.
        let samples =
            (0..=36).map(|i| WallSample { x: 2.0 + i as f32 * 0.5, y: 10.5, ground: 0.0, open: false }).collect();
        let wall = BakedWall { id: "w".into(), kind: WallKind::DryStone, height: 1.3, seed: 0, samples };
        let structures = Structures::of(&[wall], &[]);
        let boulder =
            |x: f32, y: f32| Prop { kind: PropKind::Boulder, x, y, ground_height: 0.0, scale: 1.0, rotation: 0.0 };
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.props = vec![boulder(10.2, 11.3), boulder(10.2, 20.5)];
        structures.apply(&mut chunk);
        assert_eq!(chunk.props, vec![boulder(10.2, 20.5)], "only the boulder in the wall's way goes");
    }
}
