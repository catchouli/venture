//! The noise functions that shape the land, before rivers, lakes and roads.
//!
//! Standard coherent-noise techniques are layered here:
//! - fractal Brownian motion (fBm) for continents, hills and region masks,
//! - ridged multifractal noise for sharp mountain ridges,
//! - a falloff near the world's edges so the world is surrounded by sea.
//!
//! How strongly each layer applies depends on the biome (see [`crate::regions`]).

use noise::{Fbm, MultiFractal, NoiseFn, Perlin, RidgedMulti};
use venture_world::Biome;

use crate::WorldGenConfig;
use crate::config::BiomeTerrain;
use crate::regions::Regions;

/// Seeded noise generators, created once per world.
pub struct NoiseFields {
    continent: Fbm<Perlin>,
    mountain_variation: Fbm<Perlin>,
    mountains: RidgedMulti<Perlin>,
    hilliness: Fbm<Perlin>,
    hills: Fbm<Perlin>,
    relief: Fbm<Perlin>,
    patches: Fbm<Perlin>,
    forest: Fbm<Perlin>,
}

impl NoiseFields {
    /// Creates the generators. Each gets its own seed so the layers are independent.
    pub fn new(config: &WorldGenConfig) -> Self {
        let seed = config.seed;
        let fbm = |offset: u32, octaves: usize, frequency: f64| {
            Fbm::<Perlin>::new(seed.wrapping_add(offset)).set_octaves(octaves).set_frequency(frequency)
        };
        Self {
            continent: fbm(0, 4, config.continent_frequency),
            mountain_variation: fbm(1, 3, config.mountain_variation_frequency),
            mountains: RidgedMulti::<Perlin>::new(seed.wrapping_add(2))
                .set_octaves(4)
                .set_frequency(config.mountain_frequency),
            hilliness: fbm(3, 2, config.hilliness_frequency),
            hills: fbm(4, 4, config.hill_frequency),
            relief: fbm(5, 5, config.relief_frequency),
            patches: fbm(6, 3, config.patch_frequency),
            forest: fbm(7, 3, config.forest_frequency),
        }
    }

    /// Large-scale land height at a point: continents, biome base heights,
    /// mountains, hills, plains and volcanoes.
    pub fn land_height(&self, config: &WorldGenConfig, regions: &Regions, x: f64, y: f64) -> f32 {
        let weights = regions.weights_at(x as f32, y as f32);
        let biome_value = |value: fn(&BiomeTerrain) -> f32| weights.blend(|biome| value(config.biome(biome)));

        let continent = sample(&self.continent, x, y);
        let base = config.land_base_height + continent * config.continent_height + biome_value(|b| b.base_height);

        // Ridged noise gives sharp crests; squaring it keeps valleys broad.
        let variation = 0.6 + 0.4 * sample(&self.mountain_variation, x, y);
        let ridges = (sample(&self.mountains, x, y) * 0.5 + 0.5).clamp(0.0, 1.0);
        let mountains = ridges * ridges * config.mountain_height * biome_value(|b| b.mountains) * variation;

        // Within a biome, some areas are hilly and others flat.
        let hilliness = smoothstep(-0.3, 0.4, sample(&self.hilliness, x, y));
        let hills = sample(&self.hills, x, y) * hilliness * config.hill_height * biome_value(|b| b.hills);

        let volcano = regions.volcano_height(config, x as f32, y as f32) * weights.of(Biome::Volcanic);
        let land = base + hills + mountains + volcano;

        // Slope down into the ocean near the world's edges. The continent noise
        // pushes the coast in and out so it isn't a straight line.
        let edge_distance = x.min(y).min(config.world_size as f64 - x.max(y)) as f32;
        let coast_wobble = continent * config.edge_falloff_width * 0.6;
        let shore = smoothstep(0.0, config.edge_falloff_width, edge_distance + coast_wobble);
        lerp(config.ocean_floor_height, land, shore)
    }

    /// Fine relief: the bumps, dips and mounds that make the ground feel
    /// natural up close. `amount` scales it (per biome).
    pub fn relief(&self, config: &WorldGenConfig, amount: f32, x: f64, y: f64) -> f32 {
        sample(&self.relief, x, y) * config.relief_height * amount
    }

    /// Patchy noise in `0.0..=1.0`, used to break up the edges between materials.
    pub fn patch(&self, x: f64, y: f64) -> f32 {
        // fBm rarely strays far from zero, so stretch `-0.5..0.5` to fill the range.
        (sample(&self.patches, x, y) + 0.5).clamp(0.0, 1.0)
    }

    /// Woodland density in `0.0..=1.0`: high in woods, low in clearings.
    pub fn forest(&self, x: f64, y: f64) -> f32 {
        (sample(&self.forest, x, y) + 0.5).clamp(0.0, 1.0)
    }
}

fn sample(noise: &impl NoiseFn<f64, 2>, x: f64, y: f64) -> f32 {
    noise.get([x, y]) as f32
}

/// Linear interpolation from `a` (at `t = 0`) to `b` (at `t = 1`).
///
/// Written so the result is exactly `a` at `t = 0` and exactly `b` at
/// `t = 1`. Neighbouring chunks rely on this: a feature that is out of reach
/// must leave heights bit-for-bit unchanged, or chunk edges won't match.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// Smooth 0-to-1 transition as `x` goes from `edge0` to `edge1` (clamped outside).
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
