//! Whole-world checks on the coarse map and generated chunks.

use venture_world::{CHUNK_SIZE, ChunkPos};
use venture_worldgen::{WorldGenConfig, WorldGenerator};

fn world() -> WorldGenerator {
    WorldGenerator::new(WorldGenConfig { world_size: 1024, ..WorldGenConfig::default() })
}

#[test]
fn rivers_never_flow_uphill() {
    let world = world();
    let map = world.map();
    let mut rivers = 0;
    for j in 0..map.size() {
        for i in 0..map.size() {
            if let Some(river) = map.point_river(i, j) {
                rivers += 1;
                assert!(river.end.surface <= river.start.surface, "river at ({i}, {j}) flows uphill");
            }
        }
    }
    assert!(rivers > 0, "the test world should have rivers");
}

#[test]
fn liquids_cover_ground_below_their_surface() {
    let world = world();
    let chunks = world.config().world_size / CHUNK_SIZE;
    let mut wet_cells = 0;
    for cy in 0..chunks {
        for cx in 0..chunks {
            let chunk = world.generate_chunk(ChunkPos::new(cx, cy));
            for y in 0..CHUNK_SIZE as usize {
                for x in 0..CHUNK_SIZE as usize {
                    let Some(fluid) = chunk.cell(x, y).fluid else { continue };
                    wet_cells += 1;
                    let lowest = chunk.cell_corner_heights(x, y).into_iter().fold(f32::INFINITY, f32::min);
                    assert!(lowest < fluid.level, "a covered cell's ground is entirely above the surface");
                }
            }
        }
    }
    assert!(wet_cells > 0, "the test world should have water");
}

#[test]
fn props_stand_inside_their_chunk_on_dry_ground() {
    let world = world();
    let pos = ChunkPos::new(7, 7);
    let chunk = world.generate_chunk(pos);
    let (ox, oy) = pos.origin_cell();
    for prop in &chunk.props {
        let (lx, ly) = (prop.x - ox as f32, prop.y - oy as f32);
        assert!((0.0..CHUNK_SIZE as f32).contains(&lx) && (0.0..CHUNK_SIZE as f32).contains(&ly));
        assert!(chunk.cell(lx as usize, ly as usize).fluid.is_none());
    }
}

#[test]
fn settlements_have_level_ground() {
    let world = world();
    for settlement in world.map().settlements() {
        let (x, y) = settlement.position;
        let centre = world.ground_height(x, y);
        let nearby = world.ground_height(x + 10.0, y);
        assert!((centre - nearby).abs() < 0.01, "settlement at {x}, {y} isn't level");
    }
}
