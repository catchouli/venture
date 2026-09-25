//! Choosing each cell's surface material.

use venture_world::{Biome, Fluid, Ground};

use crate::WorldGenConfig;
use crate::config::ROCK_TRANSITION;
use crate::noise_fields::smoothstep;

/// What the surface rules need to know about a cell.
pub struct SurfaceInputs {
    /// Heights of the cell's four corners.
    pub corner_heights: [f32; 4],
    /// Liquid over the cell, if any.
    pub fluid: Option<Fluid>,
    /// The nearest shore, if the cell is within beach reach of water.
    pub shore: Option<Shore>,
    /// Whether a road runs over the cell.
    pub on_road: bool,
    /// Material painted here by an overlay, which overrides everything but liquid.
    pub painted: Option<Ground>,
    /// The cell's biome.
    pub biome: Biome,
    /// Height where rock starts to appear, blended between nearby biomes.
    pub rock_start_height: f32,
    /// Patchy noise in `0.0..=1.0` for breaking up the edges of rock, snow and sand.
    pub edge_noise: f32,
    /// Larger-scale patchy noise in `0.0..=1.0` for patches of a biome's second material.
    pub patch_noise: f32,
}

/// How close a dry cell is to water.
#[derive(Clone, Copy, Debug)]
pub struct Shore {
    /// Distance to the water as a fraction of how far beaches can reach there:
    /// `0.0` right by the water, `1.0` at the furthest a beach could go.
    pub closeness: f32,
    /// The surface height of that water.
    pub level: f32,
}

/// Picks a cell's material. The rules are checked in order; the first match wins.
pub fn choose_ground(config: &WorldGenConfig, cell: &SurfaceInputs) -> Ground {
    let heights = cell.corner_heights;
    let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let highest = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mean = heights.iter().sum::<f32>() / 4.0;
    let volcanic = cell.biome == Biome::Volcanic;
    let rock = if volcanic { Ground::Basalt } else { Ground::Rock };

    // River, lake and sea beds.
    if cell.fluid.is_some_and(|fluid| lowest < fluid.level) {
        return if volcanic { Ground::Basalt } else { Ground::Sand };
    }
    if let Some(ground) = cell.painted {
        return ground;
    }
    if cell.on_road {
        return Ground::Path;
    }
    // Snow caps, with a ragged edge.
    if !volcanic && mean > config.snow_height + (cell.edge_noise - 0.5) * 8.0 {
        return Ground::Snow;
    }
    // Cliffs.
    if highest - lowest > config.cliff_slope {
        return rock;
    }
    // The land gets rockier the higher it goes.
    let rockiness = smoothstep(cell.rock_start_height, cell.rock_start_height + ROCK_TRANSITION, mean);
    if cell.edge_noise < rockiness {
        return rock;
    }
    // Beaches and river banks, reaching further in some places than others.
    if let Some(shore) = &cell.shore {
        let reach = 0.45 + 0.55 * cell.edge_noise;
        if shore.closeness < reach && mean < shore.level + config.beach_height {
            return if volcanic { Ground::Ash } else { Ground::Sand };
        }
    }
    // Each biome's own ground, with patches of a second material.
    let patch = |threshold: f32, patch: Ground, main: Ground| if cell.patch_noise > threshold { patch } else { main };
    match cell.biome {
        Biome::Grassland => patch(0.8, Ground::Dirt, Ground::Grass),
        Biome::Meadow => patch(0.72, Ground::Grass, Ground::MeadowGrass),
        Biome::Highlands => patch(0.68, Ground::Dirt, Ground::Grass),
        Biome::Volcanic => patch(0.75, Ground::Basalt, Ground::Ash),
    }
}
