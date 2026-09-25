//! Scattering props (trees, rocks, bushes) over a chunk.
//!
//! Candidate spots sit on a world-aligned grid, jittered so they don't look
//! like rows. Whether a spot gets a prop, and which, depends on the biome, a
//! woodland noise (which groups trees into woods and clearings), the slope
//! and the ground. Every choice comes from a hash of the spot's grid
//! coordinates, so the same spot always gets the same prop.

use std::f32::consts::TAU;

use venture_world::{Biome, CHUNK_SIZE, Chunk, Ground, Prop, PropKind};

use crate::WorldGenerator;
use crate::grid::point_hash;
use crate::noise_fields::smoothstep;
use crate::overlay::LocalOverlay;

/// Salts for the independent random numbers drawn at each spot.
mod salt {
    pub const JITTER_X: u32 = 101;
    pub const JITTER_Y: u32 = 102;
    pub const BIOME: u32 = 103;
    pub const CHOICE: u32 = 104;
    pub const SPECIES: u32 = 105;
    pub const SCALE: u32 = 106;
    pub const ROTATION: u32 = 107;
}

/// Trees don't grow on ground steeper than this (height change across a cell).
const MAX_TREE_SLOPE: f32 = 1.2;

/// The props whose base lies in `chunk`. Needs the chunk's heights and cells
/// to already be generated.
pub fn scatter_props(generator: &WorldGenerator, chunk: &Chunk, overlay: &LocalOverlay) -> Vec<Prop> {
    let config = generator.config();
    let spacing = config.prop_spacing;
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    let (origin_x, origin_y) = (origin_x as f32, origin_y as f32);
    let first = |origin: f32| (origin / spacing).ceil() as i32;
    let last = |origin: f32| ((origin + CHUNK_SIZE as f32) / spacing).ceil() as i32;

    let mut props = Vec::new();
    for gy in first(origin_y)..last(origin_y) {
        for gx in first(origin_x)..last(origin_x) {
            let hash = |salt| point_hash(gx, gy, config.seed.wrapping_add(salt));
            let x = (gx as f32 + hash(salt::JITTER_X) * 0.9) * spacing;
            let y = (gy as f32 + hash(salt::JITTER_Y) * 0.9) * spacing;
            let (local_x, local_y) = (x - origin_x, y - origin_y);
            if !(0.0..CHUNK_SIZE as f32).contains(&local_x) || !(0.0..CHUNK_SIZE as f32).contains(&local_y) {
                continue;
            }

            let (cx, cy) = (local_x as usize, local_y as usize);
            let cell = chunk.cell(cx, cy);
            if cell.fluid.is_some() || cell.ground == Ground::Path || overlay.covers(x, y) {
                continue;
            }
            let heights = chunk.cell_corner_heights(cx, cy);
            let slope = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - heights.iter().copied().fold(f32::INFINITY, f32::min);

            let biome = generator.regions.weights_at(x, y).pick(hash(salt::BIOME));
            let Some(kind) =
                choose_kind(generator, biome, cell.ground, slope, x, y, hash(salt::CHOICE), hash(salt::SPECIES))
            else {
                continue;
            };
            let size = hash(salt::SCALE);
            let scale = match kind {
                // Mostly small rocks, with the odd huge one.
                PropKind::Boulder => 0.5 + size * size * size * 2.5,
                _ => 0.75 + size * 0.55,
            };
            props.push(Prop {
                kind,
                x,
                y,
                ground_height: chunk.height_at(local_x, local_y),
                scale,
                rotation: hash(salt::ROTATION) * TAU,
            });
        }
    }
    props
}

/// Decides what (if anything) grows at a spot. `choice` and `species` are
/// independent random numbers in `0.0..1.0`.
#[allow(clippy::too_many_arguments)]
fn choose_kind(
    generator: &WorldGenerator,
    biome: Biome,
    ground: Ground,
    slope: f32,
    x: f32,
    y: f32,
    choice: f32,
    species: f32,
) -> Option<PropKind> {
    let settings = generator.config().biome(biome);
    let woodland = generator.noise.forest(x as f64, y as f64);

    let can_grow = slope < MAX_TREE_SLOPE && !matches!(ground, Ground::Rock | Ground::Snow | Ground::Sand);
    let tree_chance = if can_grow {
        // Dense in woods, with the odd lone tree in the open.
        (settings.forest_cover * smoothstep(0.35, 0.75, woodland) * 1.6).min(0.85) + 0.01
    } else {
        0.0
    };
    let boulder_chance = settings.boulders * if ground == Ground::Rock { 0.12 } else { 0.03 };
    let bush_chance = if can_grow { settings.bushes * 0.07 * (0.5 + woodland) } else { 0.0 };

    if choice < tree_chance {
        return Some(tree_species(biome, species));
    }
    if choice < tree_chance + boulder_chance {
        return Some(PropKind::Boulder);
    }
    if choice < tree_chance + boulder_chance + bush_chance {
        return Some(PropKind::Bush);
    }
    None
}

/// Which tree grows in a biome, where `t` in `0.0..1.0` is a random number.
fn tree_species(biome: Biome, t: f32) -> PropKind {
    match biome {
        Biome::Grassland if t < 0.8 => PropKind::Oak,
        Biome::Grassland => PropKind::Birch,
        Biome::Meadow if t < 0.6 => PropKind::Birch,
        Biome::Meadow => PropKind::Oak,
        Biome::Highlands if t < 0.85 => PropKind::Pine,
        Biome::Highlands => PropKind::Birch,
        Biome::Volcanic => PropKind::DeadTree,
    }
}
