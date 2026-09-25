//! Biome regions: dividing the world into grasslands, meadows, highlands and
//! volcanic lands.
//!
//! Region centres ("sites") sit on a jittered grid, and biomes are dealt to
//! them from a shuffled deck.
//! A point belongs mostly to its nearest site (a Voronoi diagram), but
//! weights are blended near borders so terrain changes smoothly from one
//! biome to the next. Positions are domain-warped with noise before finding
//! the nearest site, so borders wander instead of being straight lines.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use venture_world::Biome;

use crate::WorldGenConfig;
use crate::grid::cell_hash;

/// How common each biome is, in the order of [`Biome::ALL`].
const BIOME_ODDS: [f32; Biome::COUNT] = [0.36, 0.25, 0.28, 0.11];

/// How many grid cells around a point are searched for its nearest sites.
/// Two covers any site the warp could make nearest.
const SEARCH_RADIUS: i32 = 2;

/// The centre of one region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionSite {
    /// Position in metres.
    pub x: f32,
    /// Position in metres.
    pub y: f32,
    /// The region's biome.
    pub biome: Biome,
}

/// How much a point belongs to each biome. The weights add up to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeWeights([f32; Biome::COUNT]);

impl BiomeWeights {
    /// The weight of one biome.
    pub fn of(&self, biome: Biome) -> f32 {
        self.0[biome.index()]
    }

    /// Blends a per-biome value by these weights.
    pub fn blend(&self, value: impl Fn(Biome) -> f32) -> f32 {
        Biome::ALL.iter().map(|&biome| self.of(biome) * value(biome)).sum()
    }

    /// The biome with the most weight.
    pub fn dominant(&self) -> Biome {
        Biome::ALL.into_iter().max_by(|a, b| self.of(*a).total_cmp(&self.of(*b))).expect("there is at least one biome")
    }

    /// Picks a biome at random in proportion to the weights, where `t` in
    /// `0.0..1.0` is the random number. Using patchy noise for `t` gives
    /// ragged, natural-looking borders.
    pub fn pick(&self, t: f32) -> Biome {
        let mut total = 0.0;
        for biome in Biome::ALL {
            total += self.of(biome);
            if t < total {
                return biome;
            }
        }
        self.dominant()
    }
}

/// The world's regions.
pub struct Regions {
    sites: Vec<RegionSite>,
    /// Grid cells (one site each) along each axis.
    per_side: i32,
    spacing: f32,
    blend: f32,
    warp_amount: f32,
    warp_x: Fbm<Perlin>,
    warp_y: Fbm<Perlin>,
}

impl Regions {
    /// Lays out the regions for a world.
    pub fn new(config: &WorldGenConfig) -> Self {
        let per_side = (config.world_size as f32 / config.region_spacing).ceil() as i32;
        let spacing = config.world_size as f32 / per_side as f32;
        let biomes = deal_biomes((per_side * per_side) as usize, per_side, config.seed);
        let sites: Vec<RegionSite> = biomes
            .into_iter()
            .enumerate()
            .map(|(index, biome)| {
                let (i, j) = (index as i32 % per_side, index as i32 / per_side);
                let hash = |salt: u32| cell_hash(index, config.seed.wrapping_add(salt));
                RegionSite {
                    x: (i as f32 + 0.1 + 0.8 * hash(11)) * spacing,
                    y: (j as f32 + 0.1 + 0.8 * hash(12)) * spacing,
                    biome,
                }
            })
            .collect();

        let warp = |salt: u32| {
            Fbm::<Perlin>::new(config.seed.wrapping_add(salt))
                .set_octaves(3)
                .set_frequency(1.0 / (config.region_spacing as f64 * 0.6))
        };
        Self {
            sites,
            per_side,
            spacing,
            blend: config.region_blend,
            warp_amount: config.region_warp,
            warp_x: warp(20),
            warp_y: warp(21),
        }
    }

    /// Every region site.
    pub fn sites(&self) -> &[RegionSite] {
        &self.sites
    }

    /// How much the point `(x, y)` belongs to each biome.
    pub fn weights_at(&self, x: f32, y: f32) -> BiomeWeights {
        let (wx, wy) = (x as f64, y as f64);
        let x = x + self.warp_x.get([wx, wy]) as f32 * self.warp_amount;
        let y = y + self.warp_y.get([wx, wy]) as f32 * self.warp_amount;

        let nearby: Vec<(f32, Biome)> =
            self.sites_around(x, y, SEARCH_RADIUS).map(|site| ((site.x - x).hypot(site.y - y), site.biome)).collect();
        let nearest = nearby.iter().map(|(distance, _)| *distance).fold(f32::INFINITY, f32::min);

        // Soft Voronoi: sites only slightly further away than the nearest still count a bit.
        let mut weights = [0.0; Biome::COUNT];
        for (distance, biome) in nearby {
            weights[biome.index()] += (-(distance - nearest) / self.blend).exp();
        }
        let total: f32 = weights.iter().sum();
        BiomeWeights(weights.map(|weight| weight / total))
    }

    /// Height added by any volcano near `(x, y)`: a cone with a crater on top.
    pub fn volcano_height(&self, config: &WorldGenConfig, x: f32, y: f32) -> f32 {
        let mut height = 0.0_f32;
        for site in self.sites_around(x, y, 1) {
            if !config.biome(site.biome).volcanoes {
                continue;
            }
            let distance = (site.x - x).hypot(site.y - y);
            if distance >= config.volcano_radius {
                continue;
            }
            // Concave slopes, steepening towards the top, like a real volcano.
            let cone = config.volcano_height * (1.0 - distance / config.volcano_radius).powf(1.6);
            let crater = if distance < config.crater_radius {
                let across = distance / config.crater_radius;
                config.crater_depth * (1.0 - across * across)
            } else {
                0.0
            };
            height = height.max(cone - crater);
        }
        height
    }

    /// Sites in the grid cells within `radius` cells of the one containing `(x, y)`.
    fn sites_around(&self, x: f32, y: f32, radius: i32) -> impl Iterator<Item = &RegionSite> {
        let (ci, cj) = ((x / self.spacing).floor() as i32, (y / self.spacing).floor() as i32);
        (cj - radius..=cj + radius)
            .flat_map(move |j| (ci - radius..=ci + radius).map(move |i| (i, j)))
            .filter(|&(i, j)| (0..self.per_side).contains(&i) && (0..self.per_side).contains(&j))
            .map(|(i, j)| &self.sites[(j * self.per_side + i) as usize])
    }
}

/// Deals biomes to the region sites like cards from a shuffled deck, so
/// each biome gets close to its share of [`BIOME_ODDS`]. Then moves apart any
/// biomes that shouldn't border themselves (volcanic lands), so each is a
/// distinct place rather than one huge blob.
fn deal_biomes(count: usize, per_side: i32, seed: u32) -> Vec<Biome> {
    let mut deck: Vec<Biome> = Vec::with_capacity(count);
    for (biome, odds) in Biome::ALL.into_iter().zip(BIOME_ODDS) {
        let share = (odds * count as f32).round().max(1.0) as usize;
        deck.extend(std::iter::repeat_n(biome, share));
    }
    deck.resize(count, Biome::Grassland);

    // Fisher-Yates shuffle, driven by the seed.
    for i in (1..deck.len()).rev() {
        let j = (cell_hash(i, seed.wrapping_add(14)) * (i + 1) as f32) as usize;
        deck.swap(i, j.min(i));
    }

    let neighbours = |index: usize| {
        let (i, j) = (index as i32 % per_side, index as i32 / per_side);
        [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)]
            .into_iter()
            .filter(|&(i, j)| (0..per_side).contains(&i) && (0..per_side).contains(&j))
            .map(|(i, j)| (j * per_side + i) as usize)
            .collect::<Vec<_>>()
    };
    let crowded = |deck: &[Biome], index: usize| {
        deck[index] == Biome::Volcanic && neighbours(index).iter().any(|&n| deck[n] == Biome::Volcanic)
    };
    for index in 0..deck.len() {
        if !crowded(&deck, index) {
            continue;
        }
        // Swap with the first site (in a seeded order) where volcanic land wouldn't be crowded.
        let start = (cell_hash(index, seed.wrapping_add(15)) * deck.len() as f32) as usize;
        for offset in 0..deck.len() {
            let other = (start + offset) % deck.len();
            if deck[other] == Biome::Volcanic
                || neighbours(other).iter().any(|&n| n != index && deck[n] == Biome::Volcanic)
            {
                continue;
            }
            deck.swap(index, other);
            break;
        }
    }
    deck
}
