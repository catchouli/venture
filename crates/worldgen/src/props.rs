//! Scattering props (trees, rocks, bushes) over a chunk.
//!
//! Candidate spots sit on a world-aligned grid, jittered so they don't look
//! like rows. Whether a spot gets a prop, and which, depends on the biome, a
//! woodland noise (which groups trees into woods and clearings), the slope
//! and the ground. Every choice comes from a hash of the spot's grid
//! coordinates, so the same spot always gets the same prop.
//!
//! A tree's species comes from its biome's mix (willows near water), and
//! it's young, mature or old at random. Bushes are young trees of the
//! biome's bush species, small. The client grows them all with the tree
//! generator (see `docs/plan-trees.md`).

use std::f32::consts::TAU;

use venture_world::{Biome, CHUNK_SIZE, Chunk, Ground, Prop, PropKind};

use crate::config::BiomeTerrain;

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
    pub const VARIANT: u32 = 108;
    pub const WATERSIDE: u32 = 109;
}

/// How likely a tree is to be young, mature or old.
const VARIANT_SHARES: [f32; 3] = [0.25, 0.55, 0.2];
/// Water within this many cells makes a spot waterside (for willows).
const WATERSIDE_CELLS: i32 = 4;

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

    let palette = config.tree_species();
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
            let Some(choice) = choose_kind(generator, biome, cell.ground, slope, x, y, hash(salt::CHOICE)) else {
                continue;
            };
            let size = hash(salt::SCALE);
            let settings = config.biome(biome);
            let waterside =
                || near_water(chunk, cx as i32, cy as i32) && hash(salt::WATERSIDE) < config.waterside_share;
            let (kind, scale) = match choice {
                // Mostly small rocks, with the odd huge one.
                Choice::Boulder => (Some(PropKind::Boulder), 0.5 + size * size * size * 2.5),
                Choice::Tree => {
                    let id =
                        if waterside() { config.waterside_tree } else { pick(settings.trees, hash(salt::SPECIES)) };
                    let variant = pick_variant(hash(salt::VARIANT));
                    (tree(&palette, id, variant), 0.85 + size * 0.3)
                }
                Choice::Bush => {
                    (tree(&palette, pick(settings.bush_species, hash(salt::SPECIES)), 0), 0.35 + size * 0.2)
                }
            };
            let Some(kind) = kind else { continue };
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

/// What grows at a spot.
enum Choice {
    Tree,
    Bush,
    Boulder,
}

/// Decides what (if anything) grows at a spot. `choice` is a random number in `0.0..1.0`.
fn choose_kind(
    generator: &WorldGenerator,
    biome: Biome,
    ground: Ground,
    slope: f32,
    x: f32,
    y: f32,
    choice: f32,
) -> Option<Choice> {
    let settings: &BiomeTerrain = generator.config().biome(biome);
    let woodland = generator.noise.forest(x as f64, y as f64);

    let can_grow = slope < MAX_TREE_SLOPE && !matches!(ground, Ground::Rock | Ground::Snow | Ground::Sand);
    let tree_chance = if can_grow {
        // Dense in woods, with the odd lone tree in the open.
        ((settings.forest_cover * smoothstep(0.35, 0.75, woodland) * 1.6).min(0.85) + 0.01)
            * generator.config().tree_share
    } else {
        0.0
    };
    let boulder_chance = settings.boulders * if ground == Ground::Rock { 0.12 } else { 0.03 };
    let bush_chance = if can_grow { settings.bushes * 0.07 * (0.5 + woodland) } else { 0.0 };

    if choice < tree_chance {
        // (Volcanic land's trees are dead ones: see its mix.)
        return Some(Choice::Tree);
    }
    if choice < tree_chance + boulder_chance {
        return Some(Choice::Boulder);
    }
    if choice < tree_chance + boulder_chance + bush_chance {
        return Some(Choice::Bush);
    }
    None
}

/// One of a mix of species (ids with how common each is), where `t` in `0.0..1.0` is a random number.
fn pick(mix: &[(&'static str, f32)], t: f32) -> &'static str {
    let total: f32 = mix.iter().map(|(_, weight)| weight).sum();
    let mut left = t * total;
    for (id, weight) in mix {
        left -= weight;
        if left < 0.0 {
            return id;
        }
    }
    mix.last().map_or("", |(id, _)| id)
}

/// Young, mature or old (0, 1 or 2), where `t` in `0.0..1.0` is a random number.
fn pick_variant(t: f32) -> u8 {
    let mut left = t;
    for (variant, share) in VARIANT_SHARES.iter().enumerate() {
        left -= share;
        if left < 0.0 {
            return variant as u8;
        }
    }
    VARIANT_SHARES.len() as u8 - 1
}

/// A tree prop of species `id` (by its place in the world's species), if it's one of them.
fn tree(palette: &[String], id: &str, variant: u8) -> Option<PropKind> {
    let species = palette.iter().position(|known| known == id)? as u16;
    Some(PropKind::Tree { species, variant })
}

/// Whether there's water within [`WATERSIDE_CELLS`] of a cell (in its chunk).
fn near_water(chunk: &Chunk, x: i32, y: i32) -> bool {
    let last = CHUNK_SIZE - 1;
    (-WATERSIDE_CELLS..=WATERSIDE_CELLS).any(|dy| {
        (-WATERSIDE_CELLS..=WATERSIDE_CELLS).any(|dx| {
            let (nx, ny) = ((x + dx).clamp(0, last) as usize, (y + dy).clamp(0, last) as usize);
            chunk.cell(nx, ny).fluid.is_some_and(|fluid| fluid.kind == venture_world::FluidKind::Water)
        })
    })
}
