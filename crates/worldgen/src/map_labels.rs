//! Place names for the world map, found in the land itself until places are
//! named by hand (see `docs/plan-map.md`):
//!
//! - **Regions** ([`LabelKind::Region`]): large stretches of one biome
//!   ("The Windswept Downs"), and seas (water reaching the world's edge).
//! - **Areas** ([`LabelKind::Area`]): the settlements, and big lakes.
//! - **Places** ([`LabelKind::Place`]): smaller lakes and ponds, and woods.
//!
//! Names are made up from the world's seed, so the same world always gets
//! the same ones.

use std::collections::HashSet;

use venture_world::{Biome, LabelKind, MapLabel, MapLabels, WorldManifest};

/// Metres across a block of the water grid.
const WATER_BLOCK: i32 = 16;
/// Metres across a block of the tree grid.
const TREE_BLOCK: i32 = 32;
/// Metres across a region block (a chunk).
const REGION_BLOCK: i32 = 64;
/// Fewest region blocks a region (or sea, or big lake) has.
const MIN_REGION_BLOCKS: usize = 24;
/// Fewest water blocks a named lake has.
const MIN_LAKE_BLOCKS: usize = 20;
/// Fewest trees in a tree block for it to count as woodland.
const WOODLAND_TREES: u32 = 10;
/// Fewest woodland blocks a named wood has.
const MIN_WOOD_BLOCKS: usize = 6;
/// Woodland blocks from which a wood is a forest.
const FOREST_BLOCKS: usize = 40;

/// What the place names need to know about the land, counted cell by cell
/// while the map is drawn.
pub struct LandFacts {
    world_size: i32,
    /// Cells of each biome in each region block.
    biomes: Vec<[u32; Biome::COUNT]>,
    /// Water cells in each water block.
    water: Vec<u32>,
    /// Trees in each tree block.
    trees: Vec<u32>,
}

impl LandFacts {
    /// Nothing counted yet, for a world `world_size` metres across.
    pub fn new(world_size: i32) -> Self {
        let blocks = |size: i32| ((world_size + size - 1) / size).pow(2) as usize;
        Self {
            world_size,
            biomes: vec![[0; Biome::COUNT]; blocks(REGION_BLOCK)],
            water: vec![0; blocks(WATER_BLOCK)],
            trees: vec![0; blocks(TREE_BLOCK)],
        }
    }

    fn side(&self, block: i32) -> i32 {
        (self.world_size + block - 1) / block
    }

    fn index(&self, block: i32, x: i32, y: i32) -> usize {
        let side = self.side(block);
        ((y / block).clamp(0, side - 1) * side + (x / block).clamp(0, side - 1)) as usize
    }

    /// Counts one cell (at metres `(x, y)`).
    pub fn count_cell(&mut self, x: i32, y: i32, biome: Biome, water: bool) {
        let region = self.index(REGION_BLOCK, x, y);
        self.biomes[region][biome.index()] += 1;
        if water {
            let block = self.index(WATER_BLOCK, x, y);
            self.water[block] += 1;
        }
    }

    /// Counts a tree (at metres `(x, y)`).
    pub fn count_tree(&mut self, x: f32, y: f32) {
        let block = self.index(TREE_BLOCK, x as i32, y as i32);
        self.trees[block] += 1;
    }

    /// Adds another count (of other chunks) to this one.
    pub fn merge(&mut self, other: &LandFacts) {
        for (mine, theirs) in self.biomes.iter_mut().zip(&other.biomes) {
            for (a, b) in mine.iter_mut().zip(theirs) {
                *a += b;
            }
        }
        for (a, b) in self.water.iter_mut().zip(&other.water) {
            *a += b;
        }
        for (a, b) in self.trees.iter_mut().zip(&other.trees) {
            *a += b;
        }
    }

    /// Whether a region block is mostly water.
    fn region_is_water(&self, index: usize) -> bool {
        let side = self.side(REGION_BLOCK) as usize;
        let (rx, ry) = ((index % side) as i32 * REGION_BLOCK, (index / side) as i32 * REGION_BLOCK);
        let per = REGION_BLOCK / WATER_BLOCK;
        let wet: u32 = (0..per)
            .flat_map(|dy| (0..per).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| self.water[self.index(WATER_BLOCK, rx + dx * WATER_BLOCK, ry + dy * WATER_BLOCK)])
            .sum();
        wet as i32 * 2 > REGION_BLOCK * REGION_BLOCK
    }
}

/// Every place name, biggest kinds first.
pub fn place_names(manifest: &WorldManifest, facts: &LandFacts) -> MapLabels {
    let mut names = Namer::new(manifest.seed);
    let mut labels = Vec::new();

    // Regions: stretches of one biome, and seas (and big lakes) over water.
    let side = facts.side(REGION_BLOCK);
    let kinds: Vec<Option<usize>> = (0..facts.biomes.len())
        .map(|index| {
            if facts.region_is_water(index) {
                return Some(Biome::COUNT);
            }
            let counts = facts.biomes[index];
            (0..Biome::COUNT).max_by_key(|&biome| counts[biome]).filter(|&biome| counts[biome] > 0)
        })
        .collect();
    for region in groups(side, |index| kinds[index], MIN_REGION_BLOCKS) {
        let at = region.centre(REGION_BLOCK);
        let label = match region.kind {
            water if water == Biome::COUNT && region.touches_edge => {
                MapLabel { name: format!("The {} Sea", names.adjective()), kind: LabelKind::Region, at }
            }
            water if water == Biome::COUNT => {
                MapLabel { name: format!("Lake {}", names.short()), kind: LabelKind::Area, at }
            }
            biome => MapLabel {
                name: format!("The {} {}", names.adjective(), names.pick(REGION_NOUNS[biome])),
                kind: LabelKind::Region,
                at,
            },
        };
        labels.push(label);
    }

    // Areas: the settlements.
    for settlement in &manifest.settlements {
        labels.push(MapLabel { name: names.town(), kind: LabelKind::Area, at: settlement.position });
    }

    // Places: lakes and ponds (not rivers: those are long and thin), then woods.
    let water_side = facts.side(WATER_BLOCK);
    let wet = |index: usize| (facts.water[index] as i32 * 2 > WATER_BLOCK * WATER_BLOCK).then_some(0);
    for lake in groups(water_side, wet, MIN_LAKE_BLOCKS) {
        let small_enough = lake.blocks < MIN_REGION_BLOCKS * (REGION_BLOCK / WATER_BLOCK).pow(2) as usize;
        if !small_enough || lake.touches_edge || lake.fill() < 0.25 {
            continue;
        }
        let name = if lake.blocks < 60 { format!("{} Pond", names.short()) } else { format!("{} Mere", names.short()) };
        labels.push(MapLabel { name, kind: LabelKind::Place, at: lake.centre(WATER_BLOCK) });
    }
    let tree_side = facts.side(TREE_BLOCK);
    let wooded = |index: usize| (facts.trees[index] >= WOODLAND_TREES).then_some(0);
    for wood in groups(tree_side, wooded, MIN_WOOD_BLOCKS) {
        let name = if wood.blocks >= FOREST_BLOCKS {
            format!("{} Forest", names.short())
        } else {
            format!("{} Wood", names.short())
        };
        labels.push(MapLabel { name, kind: LabelKind::Place, at: wood.centre(TREE_BLOCK) });
    }

    MapLabels { labels }
}

/// A group of neighbouring blocks of one kind.
struct Group {
    kind: usize,
    blocks: usize,
    /// The block nearest the group's middle (so the label is on it, even for
    /// a curved group whose middle isn't).
    middle: (i32, i32),
    /// Its bounds, in blocks: west, north, east, south.
    bounds: (i32, i32, i32, i32),
    touches_edge: bool,
}

impl Group {
    /// Where its label goes, in metres.
    fn centre(&self, block: i32) -> (f32, f32) {
        ((self.middle.0 * block + block / 2) as f32, (self.middle.1 * block + block / 2) as f32)
    }

    /// How much of its bounding box it fills.
    fn fill(&self) -> f32 {
        let (west, north, east, south) = self.bounds;
        self.blocks as f32 / ((east - west + 1) * (south - north + 1)) as f32
    }
}

/// The groups of at least `smallest` side-by-side blocks of the same kind
/// on a square grid `side` blocks across (`kind` says each block's, if any).
fn groups(side: i32, kind: impl Fn(usize) -> Option<usize>, smallest: usize) -> Vec<Group> {
    let mut seen = vec![false; (side * side) as usize];
    let mut found = Vec::new();
    for start in 0..(side * side) as usize {
        let Some(wanted) = kind(start).filter(|_| !seen[start]) else { continue };
        seen[start] = true;
        let mut members = Vec::new();
        let mut stack = vec![start];
        while let Some(index) = stack.pop() {
            let (x, y) = ((index as i32) % side, (index as i32) / side);
            members.push((x, y));
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx < 0 || ny < 0 || nx >= side || ny >= side {
                    continue;
                }
                let next = (ny * side + nx) as usize;
                if !seen[next] && kind(next) == Some(wanted) {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        if members.len() < smallest {
            continue;
        }
        let count = members.len() as f32;
        let mean = (
            members.iter().map(|m| m.0 as f32).sum::<f32>() / count,
            members.iter().map(|m| m.1 as f32).sum::<f32>() / count,
        );
        let middle = *members
            .iter()
            .min_by(|a, b| {
                let distance = |m: &&(i32, i32)| (m.0 as f32 - mean.0).hypot(m.1 as f32 - mean.1);
                distance(a).total_cmp(&distance(b))
            })
            .expect("a group has blocks");
        let bounds =
            members.iter().fold((side, side, 0, 0), |(w, n, e, s), &(x, y)| (w.min(x), n.min(y), e.max(x), s.max(y)));
        let touches_edge = bounds.0 == 0 || bounds.1 == 0 || bounds.2 == side - 1 || bounds.3 == side - 1;
        found.push(Group { kind: wanted, blocks: members.len(), middle, bounds, touches_edge });
    }
    // Biggest first (a stable order, whatever order they're found in).
    found.sort_by(|a, b| b.blocks.cmp(&a.blocks).then(a.middle.cmp(&b.middle)));
    found
}

/// What each biome's regions are called ("The Golden Downs"), by [`Biome::index`].
const REGION_NOUNS: [&[&str]; Biome::COUNT] = [
    &["Plains", "Downs", "Fields", "Wolds"],
    &["Meadows", "Vale", "Glades", "Bloomfields"],
    &["Highlands", "Crags", "Fells", "Peaks"],
    &["Ashlands", "Cinderwaste", "Emberfields", "Smokelands"],
];
const ADJECTIVES: &[&str] = &[
    "Golden",
    "Whispering",
    "Grey",
    "Silent",
    "Windswept",
    "Amber",
    "Hollow",
    "Emerald",
    "Sunken",
    "Weeping",
    "Iron",
    "Lonely",
    "Bright",
    "Misty",
    "Restless",
    "Shimmering",
    "Broken",
    "Northern",
    "Southern",
    "Old",
];
const STARTS: &[&str] = &[
    "Ash", "Bar", "Bram", "Cal", "Dun", "El", "Fen", "Gar", "Hal", "Kel", "Lin", "Mor", "Nor", "Oak", "Pen", "Rav",
    "Sel", "Thorn", "Ul", "Vel", "Wyn", "Yar", "Ald", "Brin", "Cor", "Dray", "Eld", "Hart", "Ley", "Stan",
];
const MIDDLES: &[&str] = &["", "", "", "ro", "wen", "da", "li", "ber", "na", "ing"];
const TOWN_ENDS: &[&str] = &[
    "ford", "wick", "by", "ham", "mere", "stead", "holt", "ton", "bury", "dale", "more", "well", "fell", "gate",
    "crest", "worth", "brook", "vale",
];
const SHORT_ENDS: &[&str] = &["", "en", "ley", "ow", "ith", "ane", "or"];

/// Makes up names, the same ones for the same seed, never the same twice.
struct Namer {
    state: u64,
    used: HashSet<String>,
}

impl Namer {
    fn new(seed: u32) -> Self {
        Self { state: u64::from(seed) ^ 0x6d61_705f_6e61_6d65, used: HashSet::new() }
    }

    /// The next pseudo-random number (splitmix64).
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn pick(&mut self, from: &[&str]) -> String {
        from[(self.next() % from.len() as u64) as usize].to_string()
    }

    /// A name not used before, from `make` (tried a few times, then numbered).
    fn fresh(&mut self, make: impl Fn(&mut Self) -> String) -> String {
        for _ in 0..20 {
            let name = make(self);
            if self.used.insert(name.clone()) {
                return name;
            }
        }
        let name = format!("{} {}", make(self), self.used.len());
        self.used.insert(name.clone());
        name
    }

    fn adjective(&mut self) -> String {
        self.pick(ADJECTIVES)
    }

    /// A town's name ("Kelwenford").
    fn town(&mut self) -> String {
        self.fresh(|names| format!("{}{}{}", names.pick(STARTS), names.pick(MIDDLES), names.pick(TOWN_ENDS)))
    }

    /// A short name for a lake or wood ("Ravow").
    fn short(&mut self) -> String {
        self.fresh(|names| format!("{}{}", names.pick(STARTS), names.pick(SHORT_ENDS)))
    }
}

#[cfg(test)]
mod tests {
    use venture_world::{CHUNK_FORMAT_VERSION, CHUNK_SIZE, SettlementInfo};

    use super::*;

    fn manifest(settlements: Vec<SettlementInfo>) -> WorldManifest {
        WorldManifest {
            world_size: 1024,
            chunk_size: CHUNK_SIZE,
            chunk_format: CHUNK_FORMAT_VERSION,
            generator_version: 0,
            seed: 3,
            spawn_point: (0.0, 0.0),
            settlements,
            map: None,
            tree_species: Vec::new(),
            walls: Vec::new(),
            buildings: Vec::new(),
        }
    }

    #[test]
    fn land_regions_seas_towns_lakes_and_woods_are_named() {
        let mut facts = LandFacts::new(1024);
        for y in 0..1024 {
            for x in 0..1024 {
                // Grassland in the west, highlands in the east, a sea along
                // the south edge, and a round lake in the grassland.
                let sea = y >= 900;
                let lake = (x as f32 - 200.0).hypot(y as f32 - 200.0) < 70.0;
                let biome = if x < 512 { Biome::Grassland } else { Biome::Highlands };
                facts.count_cell(x, y, biome, sea || lake);
            }
        }
        // A thick wood in the east.
        for y in (600..800).step_by(4) {
            for x in (700..900).step_by(4) {
                facts.count_tree(x as f32, y as f32);
            }
        }
        let settlement = SettlementInfo { position: (300.0, 600.0), biome: Biome::Grassland };
        let labels = place_names(&manifest(vec![settlement]), &facts).labels;
        let of = |kind| labels.iter().filter(|label| label.kind == kind).collect::<Vec<_>>();
        let regions = of(LabelKind::Region);
        assert_eq!(regions.len(), 3, "{labels:#?}");
        assert!(regions.iter().any(|label| label.name.ends_with(" Sea") && label.at.1 > 900.0));
        assert!(regions.iter().any(|label| label.at.0 < 512.0 && !label.name.ends_with(" Sea")));
        assert_eq!(of(LabelKind::Area).len(), 1);
        let places = of(LabelKind::Place);
        assert!(places.iter().any(|label| label.name.ends_with(" Mere") || label.name.ends_with(" Pond")));
        assert!(places.iter().any(|label| label.name.ends_with(" Wood") || label.name.ends_with(" Forest")));

        // The same world, the same names.
        assert_eq!(place_names(&manifest(vec![settlement]), &facts).labels, labels);
    }

    #[test]
    fn names_are_never_repeated() {
        let mut names = Namer::new(1);
        let towns: HashSet<String> = (0..300).map(|_| names.town()).collect();
        assert_eq!(towns.len(), 300);
    }
}
