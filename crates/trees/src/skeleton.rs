//! Growing a tree's skeleton: its branches as curves with a radius at each
//! point, and where its leaves go.
//!
//! The model is Weber and Penn's parametric trees ("Creation and Rendering
//! of Realistic Trees", SIGGRAPH 1995), level by level:
//! 1. **Stems:** one trunk, or several spreading from the ground. A trunk may
//!    fork into big limbs part-way up (old oaks), each limb as thick as the
//!    pipe model says: the limbs' radii, squared, add up to the trunk's
//!    (Shinozaki et al., 1964, "da Vinci's rule").
//! 2. **Branches** grow from each stem or limb, then branches from those,
//!    and so on, one level per [`BranchLevel`]. Where along its parent a
//!    branch grows, which way (spiral, opposite pairs, whorls), at what
//!    angle, how long (the crown's outline sets it for branches on the
//!    trunk) and how thick (its parent's radius there, times its share of
//!    the parent's length to the power [`RATIO_POWER`]) all come from the
//!    species and the tree's age.
//! 3. **Each branch grows segment by segment,** curving (and curving back),
//!    wandering at random, and bending up to the light or down under its
//!    own weight (tropism), which is what makes a willow weep.
//! 4. **Leaves** go along the outermost branches.
//!
//! Every branch has its own random stream (see [`Random::child`]), so a
//! change in one place leaves the rest of the tree as it was.

use std::f32::consts::TAU;

use glam::{Quat, Vec3};

use crate::params::TreeParams;
use crate::random::Random;
use crate::species::{Arrangement, BranchLevel, Species};

/// How a branch's radius follows its length compared with its parent's:
/// radius share = length share to this power (Weber and Penn's
/// `RatioPower`).
const RATIO_POWER: f32 = 1.3;
/// No branch is thicker than this share of its parent's radius where it grows.
const MOST_OF_PARENT: f32 = 0.8;
/// The thinnest a branch's tip gets, in metres.
const TIP_RADIUS: f32 = 0.004;
/// Branches shorter than this are left out, in metres.
const SHORTEST_BRANCH: f32 = 0.08;
/// The golden angle, in degrees: each branch of a spiral is turned this far from the last.
const GOLDEN_ANGLE: f32 = 137.507_77;
/// How strongly tropism bends a branch in a segment, for a tropism of 1.
const TROPISM_STRENGTH: f32 = 0.25;
/// Leaves grow along the outer part of a branch: from this share of its length.
const LEAVES_FROM: f32 = 0.25;
/// The most leaf cards a tree has. A tree that would have more has fewer,
/// bigger ones instead, covering the same area (so big old trees stay
/// affordable and look just as full).
pub const MOST_LEAF_CARDS: usize = 3000;

/// A grown tree's skeleton.
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    /// Every branch, the stems first.
    pub branches: Vec<Branch>,
    /// Every leaf card.
    pub leaves: Vec<Leaf>,
    /// The tree's height, from the ground to its highest branch, in metres.
    pub height: f32,
    /// The trunk's radius at the ground (above the flare), in metres.
    pub trunk_radius: f32,
}

/// One branch (or stem): a curve from its base to its tip.
#[derive(Clone, Debug)]
pub struct Branch {
    /// Its level: 0 for stems (and the limbs they fork into), 1 for branches on them, and so on.
    pub level: usize,
    /// The points along it, from its base, in metres (y up).
    pub points: Vec<Vec3>,
    /// Its radius at each point, in metres.
    pub radii: Vec<f32>,
    /// Whether it's a stem that starts at the ground (and flares into roots there).
    pub from_ground: bool,
    /// The branch it grows from, if any.
    pub parent: Option<usize>,
}

impl Branch {
    /// Its length, in metres.
    pub fn length(&self) -> f32 {
        self.points.windows(2).map(|pair| pair[0].distance(pair[1])).sum()
    }
}

/// A leaf card: a picture of a sprig.
#[derive(Clone, Copy, Debug)]
pub struct Leaf {
    /// Where its base is (on its twig), in metres.
    pub position: Vec3,
    /// The way along the card from its base (a unit vector).
    pub along: Vec3,
    /// The way the card faces (a unit vector, across `along`).
    pub facing: Vec3,
    /// How wide it is, in metres.
    pub width: f32,
    /// How long it is, in metres.
    pub length: f32,
    /// Which sprig picture it shows.
    pub picture: u32,
}

/// How many sprig pictures there are to choose from (see [`crate::textures`]).
pub const SPRIG_PICTURES: u32 = 4;

/// What a branch about to grow is to be like.
struct Sprout {
    level: usize,
    start: Vec3,
    orientation: Quat,
    length: f32,
    radius: f32,
    /// How much it narrows along its length (1 to a point).
    taper: f32,
    segments: u32,
    curve: f32,
    curve_back: f32,
    wander: f32,
    tropism: f32,
    from_ground: bool,
    parent: Option<usize>,
    random: Random,
}

/// Grows a tree.
pub fn grow(species: &Species, params: &TreeParams) -> Skeleton {
    let mut random = Random::new(params.seed);
    let (shortest, tallest) = species.height;
    let height = (shortest + (tallest - shortest) * params.height.clamp(0.0, 1.0)) * params.height_growth();
    let trunk = &species.trunk;
    let trunk_radius = trunk.radius * height * params.girth_growth();
    let clear = (trunk.clear * params.clear_growth()).clamp(0.0, 0.95);
    let mut tree =
        Grower { species, params, height, clear, skeleton: Skeleton { height, trunk_radius, ..Skeleton::default() } };

    // 1. The stems, spread evenly around, leaning out if there are several.
    let stems = random.between(trunk.stems.0, trunk.stems.1.max(trunk.stems.0)).max(1);
    let turn = random.unit() * TAU;
    for stem in 0..stems {
        let mut stem_random = random.child(u64::from(stem));
        let lean = (trunk.lean * params.lean.max(0.0) * stem_random.unit()).to_radians();
        let spread = if stems > 1 { trunk.stem_spread.to_radians() * stem_random.range(0.6, 1.0) } else { 0.0 };
        let around = turn + TAU * stem as f32 / stems as f32 + stem_random.signed() * 0.4;
        let orientation = Quat::from_rotation_y(around) * Quat::from_rotation_x(spread + lean);
        // Several stems share out the girth (the pipe model), and are shorter at the edges.
        let share = 1.0 / (stems as f32).sqrt();
        let stem_height = height * if stem == 0 { 1.0 } else { stem_random.range(0.75, 1.0) };
        let offset = if stems > 1 { orientation * Vec3::Z * trunk_radius * share } else { Vec3::ZERO };
        tree.grow_stem(Sprout {
            level: 0,
            start: Vec3::new(offset.x, 0.0, offset.z),
            orientation,
            length: stem_height,
            radius: trunk_radius * share.max(0.45),
            taper: trunk.taper,
            segments: trunk.segments.max(2),
            curve: 0.0,
            curve_back: 0.0,
            wander: trunk.wander,
            tropism: 0.0,
            from_ground: true,
            parent: None,
            random: stem_random,
        });
    }
    keep_within_leaf_budget(&mut tree.skeleton.leaves, &mut random);
    let mut skeleton = tree.skeleton;
    scale_to_height(&mut skeleton, height);
    skeleton
}

/// Scales a tree so its highest point is at `height` (branches curving up
/// can reach above the trunk's top). Leaves keep their size: they're life-size.
fn scale_to_height(skeleton: &mut Skeleton, height: f32) {
    let top = skeleton
        .branches
        .iter()
        .flat_map(|branch| branch.points.iter())
        .chain(skeleton.leaves.iter().map(|leaf| &leaf.position))
        .map(|point| point.y)
        .fold(0.0, f32::max);
    if top <= 0.0 {
        return;
    }
    let scale = height / top;
    for branch in &mut skeleton.branches {
        for (point, radius) in branch.points.iter_mut().zip(&mut branch.radii) {
            *point *= scale;
            *radius *= scale;
        }
    }
    for leaf in &mut skeleton.leaves {
        leaf.position *= scale;
    }
    skeleton.trunk_radius *= scale;
    skeleton.height = height;
}

/// Thins out leaf cards to [`MOST_LEAF_CARDS`], making the rest bigger to
/// cover the same area.
fn keep_within_leaf_budget(leaves: &mut Vec<Leaf>, random: &mut Random) {
    if leaves.len() <= MOST_LEAF_CARDS {
        return;
    }
    let share = MOST_LEAF_CARDS as f32 / leaves.len() as f32;
    let grow_by = (1.0 / share).sqrt();
    let mut kept = 0;
    leaves.retain(|_| {
        let keep = kept < MOST_LEAF_CARDS && random.chance(share);
        kept += usize::from(keep);
        keep
    });
    for leaf in leaves.iter_mut() {
        leaf.width *= grow_by;
        leaf.length *= grow_by;
    }
}

/// Grows a tree's branches.
struct Grower<'a> {
    species: &'a Species,
    params: &'a TreeParams,
    /// The tree's height, in metres.
    height: f32,
    /// The share of the height clear of branches.
    clear: f32,
    skeleton: Skeleton,
}

impl Grower<'_> {
    /// Grows a stem: up to where it forks (if it does), then its limbs.
    fn grow_stem(&mut self, sprout: Sprout) {
        let trunk = &self.species.trunk;
        let forks = trunk.fork_at > 0.0 && self.params.age() > 0.5;
        if !forks {
            self.grow_branch(sprout);
            return;
        }
        let mut random = sprout.random.child(u64::MAX);
        let full_length = sprout.length;
        let fork_at = (trunk.fork_at * random.range(0.85, 1.15)).clamp(0.1, 0.9);
        let segments = ((sprout.segments as f32 * fork_at).round() as u32).max(2);
        let base_radius = sprout.radius;
        let level = sprout.level;
        let lower = Sprout { length: full_length * fork_at, taper: trunk.taper * fork_at * 0.5, segments, ..sprout };
        let Some(index) = self.grow_branch(lower) else { return };

        // The limbs, from the top of the lower trunk.
        let (top, orientation, radius) = {
            let branch = &self.skeleton.branches[index];
            let top = *branch.points.last().expect("branches have points");
            let direction = (top - branch.points[branch.points.len() - 2]).normalize_or(Vec3::Y);
            (top, Quat::from_rotation_arc(Vec3::Y, direction), *branch.radii.last().expect("and radii"))
        };
        let limbs = random.between(trunk.fork_limbs.0.max(2), trunk.fork_limbs.1.max(trunk.fork_limbs.0.max(2)));
        let turn = random.unit() * TAU;
        for limb in 0..limbs {
            let mut limb_random = random.child(u64::from(limb));
            let around = turn + TAU * limb as f32 / limbs as f32 + limb_random.signed() * 0.5;
            let tilt = trunk.fork_angle.to_radians() * limb_random.range(0.6, 1.2);
            self.grow_branch(Sprout {
                level,
                start: top,
                orientation: orientation * Quat::from_rotation_y(around) * Quat::from_rotation_x(tilt),
                length: full_length * (1.0 - fork_at) * limb_random.range(0.8, 1.05),
                radius: (radius / (limbs as f32).sqrt()).min(base_radius),
                taper: 0.95,
                segments: ((trunk.segments as f32 * (1.0 - fork_at)).round() as u32).max(3),
                curve: 0.0,
                curve_back: 0.0,
                wander: trunk.wander * 1.5,
                tropism: 0.0,
                from_ground: false,
                parent: Some(index),
                random: limb_random,
            });
        }
    }

    /// Grows a branch, then the branches on it. Returns its index, unless it
    /// was too short to grow.
    fn grow_branch(&mut self, mut sprout: Sprout) -> Option<usize> {
        if sprout.length < SHORTEST_BRANCH {
            return None;
        }
        // 3. Segment by segment, from the base.
        let segments = sprout.segments.max(1);
        let step = sprout.length / segments as f32;
        let flare = if sprout.from_ground { self.species.trunk.flare } else { 0.0 };
        let mut orientation = sprout.orientation;
        let mut points = vec![sprout.start];
        let mut orientations = vec![orientation];
        let mut radii = vec![radius_at(&sprout, 0.0, flare)];
        let half = (segments as f32 / 2.0).max(1.0);
        for segment in 0..segments {
            // Curving (positive curves up, back towards the parent's line), then curving back.
            let bend = if sprout.curve_back == 0.0 {
                sprout.curve / segments as f32
            } else if (segment as f32) < half {
                sprout.curve / half
            } else {
                sprout.curve_back / half
            };
            orientation *= Quat::from_rotation_x(-bend.to_radians());
            // Wandering.
            let wander = sprout.wander.to_radians();
            orientation *= Quat::from_rotation_x(wander * sprout.random.signed())
                * Quat::from_rotation_z(wander * sprout.random.signed());
            // Tropism: towards the sky, or the ground.
            let direction = orientation * Vec3::Y;
            if sprout.tropism != 0.0 {
                let pulled = (direction + Vec3::Y * sprout.tropism * TROPISM_STRENGTH).normalize_or(direction);
                orientation = Quat::from_rotation_arc(direction, pulled) * orientation;
            }
            let position = points[points.len() - 1] + orientation * Vec3::Y * step;
            points.push(position);
            orientations.push(orientation);
            radii.push(radius_at(&sprout, (segment + 1) as f32 / segments as f32, flare));
        }
        let index = self.skeleton.branches.len();
        self.skeleton.branches.push(Branch {
            level: sprout.level,
            points,
            radii,
            from_ground: sprout.from_ground,
            parent: sprout.parent,
        });

        // 2. The branches on it; 4. or, on the outermost, its leaves.
        let levels = &self.species.branches;
        match levels.get(sprout.level) {
            Some(level) => self.grow_children(index, &orientations, &sprout, level),
            None => self.grow_leaves(index, &orientations, &mut sprout.random),
        }
        // (Branches on the second-to-last level carry some leaves too, so
        // crowns aren't hollow where their outer branches are sparse.)
        if sprout.level + 1 == levels.len() && sprout.level > 0 {
            let mut random = sprout.random.child(u64::MAX - 1);
            self.grow_some_leaves(index, &orientations, &mut random, 0.35);
        }
        Some(index)
    }

    /// Grows the next level of branches on branch `index`.
    fn grow_children(&mut self, index: usize, orientations: &[Quat], parent: &Sprout, level: &BranchLevel) {
        let params = self.params;
        let branch_length = self.skeleton.branches[index].length();
        // Branches on shorter parents are fewer (compared with the longest on the parent's level).
        let whole = self.longest(parent.level);
        let wanted = level.count * params.branch_growth() * (branch_length / whole.max(0.01)).min(1.5);
        let mut random = parent.random.child(1);
        let count = (wanted.floor() + if random.chance(wanted.fract()) { 1.0 } else { 0.0 }) as u32;
        let start = if parent.level == 0 && parent.from_ground {
            // On the trunk, after the clear stem (a share of the whole tree's height).
            (self.clear * self.height / branch_length.max(0.01)).max(level.start).min(0.95)
        } else {
            level.start
        };
        let per_ring = match level.arrangement {
            Arrangement::Spiral => 1,
            Arrangement::Opposite => 2,
            Arrangement::Whorled(ring) => ring.max(1),
        };
        let rings = count.div_ceil(per_ring);
        let mut around = random.unit() * 360.0;
        for child in 0..count {
            let mut child_random = random.child(u64::from(child));
            let ring = child / per_ring;
            let in_ring = child % per_ring;
            // Where along the parent: evenly spread, a little jittered.
            let spacing = (1.0 - start) / rings.max(1) as f32;
            let along = (start + spacing * (ring as f32 + 0.5 + 0.35 * child_random.signed())).clamp(start, 0.98);
            // Which way round.
            let turn = match level.arrangement {
                Arrangement::Spiral => {
                    around += GOLDEN_ANGLE + child_random.signed() * 15.0;
                    around
                }
                Arrangement::Opposite => ring as f32 * 90.0 + in_ring as f32 * 180.0 + child_random.signed() * 12.0,
                Arrangement::Whorled(size) => {
                    ring as f32 * 37.0 + in_ring as f32 * 360.0 / size.max(1) as f32 + child_random.signed() * 12.0
                }
            };
            let (position, orientation, parent_radius) = self.point_along(index, orientations, along);
            let top_share = along.clamp(0.0, 1.0);
            let angle = level.angle + (level.top_angle - level.angle) * top_share;
            let angle = (angle + level.angle_variation * child_random.signed()).to_radians();
            let child_orientation =
                orientation * Quat::from_rotation_y(turn.to_radians()) * Quat::from_rotation_x(angle);
            let length_variation = 1.0 + level.length_variation * child_random.signed();
            let length = if parent.level == 0 {
                // Set by the crown's outline, by height up the crown.
                let in_crown = ((position.y / self.height.max(0.01)) - self.clear) / (1.0 - self.clear).max(0.05);
                self.height * level.length * self.species.crown.ratio(in_crown) * params.spread_growth()
            } else {
                level.length * (branch_length - 0.6 * along * branch_length)
            } * length_variation;
            let radius = (parent_radius * (length / branch_length.max(0.01)).powf(RATIO_POWER))
                .min(parent_radius * MOST_OF_PARENT);
            self.grow_branch(Sprout {
                level: parent.level + 1,
                start: position,
                orientation: child_orientation,
                length,
                radius,
                taper: 1.0,
                segments: level.segments.max(1),
                curve: level.curve,
                curve_back: level.curve_back,
                wander: level.wander,
                tropism: level.tropism,
                from_ground: false,
                parent: Some(index),
                random: child_random,
            });
        }
    }

    /// The leaves along branch `index` (an outermost one).
    fn grow_leaves(&mut self, index: usize, orientations: &[Quat], random: &mut Random) {
        self.grow_some_leaves(index, orientations, random, 1.0);
    }

    /// Leaves along branch `index`, `share` as many as an outermost branch has.
    fn grow_some_leaves(&mut self, index: usize, orientations: &[Quat], random: &mut Random, share: f32) {
        let leaves = &self.species.leaves;
        let length = self.skeleton.branches[index].length();
        let wanted = leaves.per_metre * length * self.params.fullness.clamp(0.0, 4.0) * share;
        let count = (wanted.floor() + if random.chance(wanted.fract()) { 1.0 } else { 0.0 }) as u32;
        let from = if share < 1.0 { 0.6 } else { LEAVES_FROM };
        for _ in 0..count {
            let along = random.range(from, 1.0);
            let (position, orientation, _) = self.point_along(index, orientations, along);
            let branch_direction = orientation * Vec3::Y;
            // Out from the twig at a random angle, then hanging as much as the species' leaves droop.
            let sideways = orientation * Quat::from_rotation_y(random.unit() * TAU) * Vec3::Z;
            let outwards = (branch_direction * 0.6 + sideways).normalize_or(Vec3::Y);
            let droop = (leaves.droop + 0.15 * random.signed()).clamp(0.0, 1.0);
            let along_card = (outwards * (1.0 - droop) + Vec3::NEG_Y * droop).normalize_or(Vec3::NEG_Y);
            // Face a random way round the card's own line.
            let across = along_card.any_orthonormal_vector();
            let facing = Quat::from_axis_angle(along_card, random.unit() * TAU) * across;
            let width = leaves.card_size * random.range(0.8, 1.2);
            self.skeleton.leaves.push(Leaf {
                position,
                along: along_card,
                facing,
                width,
                length: width * leaves.stretch.max(0.1),
                picture: random.between(0, SPRIG_PICTURES - 1),
            });
        }
    }

    /// About how long the longest branches on `level` are, in metres.
    fn longest(&self, level: usize) -> f32 {
        let shares: f32 = self.species.branches.iter().take(level).map(|level| level.length).product();
        (self.height * shares * self.params.spread_growth()).max(0.01)
    }

    /// The point `along` (a share of its length) branch `index`: its
    /// position, orientation and radius there.
    fn point_along(&self, index: usize, orientations: &[Quat], along: f32) -> (Vec3, Quat, f32) {
        let branch = &self.skeleton.branches[index];
        let segments = branch.points.len() - 1;
        let at = along.clamp(0.0, 1.0) * segments as f32;
        let segment = (at.floor() as usize).min(segments - 1);
        let t = at - segment as f32;
        let position = branch.points[segment].lerp(branch.points[segment + 1], t);
        let orientation = orientations[segment + 1];
        let radius = branch.radii[segment] + (branch.radii[segment + 1] - branch.radii[segment]) * t;
        (position, orientation, radius)
    }
}

/// A branch's radius `along` its length (a share), flaring at the base of a
/// stem (after Weber and Penn's flare).
fn radius_at(sprout: &Sprout, along: f32, flare: f32) -> f32 {
    let tapered = sprout.radius * (1.0 - sprout.taper * along);
    let base = (1.0 - 8.0 * along).max(0.0);
    let flared = tapered * (1.0 + flare * (100f32.powf(base) - 1.0) / 100.0);
    flared.max(TIP_RADIUS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::species::{Bark, CrownShape, Leaves, Trunk};

    fn broadleaf() -> Species {
        Species {
            id: "test".to_string(),
            name: "Test".to_string(),
            height: (15.0, 20.0),
            trunk: Trunk { fork_at: 0.4, ..Trunk::default() },
            crown: CrownShape::Spherical,
            branches: vec![BranchLevel::default(), BranchLevel { count: 8.0, ..BranchLevel::default() }],
            leaves: Leaves::default(),
            bark: Bark::default(),
        }
    }

    #[test]
    fn the_same_seed_grows_the_same_tree() {
        let species = broadleaf();
        let params = TreeParams { seed: 5, ..TreeParams::default() };
        let (a, b) = (grow(&species, &params), grow(&species, &params));
        assert_eq!(a.branches.len(), b.branches.len());
        assert_eq!(a.leaves.len(), b.leaves.len());
        assert_eq!(a.branches.last().unwrap().points, b.branches.last().unwrap().points);
        let other = grow(&species, &TreeParams { seed: 6, ..params });
        assert_ne!(a.branches.last().unwrap().points, other.branches.last().unwrap().points);
    }

    #[test]
    fn trees_are_about_as_tall_as_asked_and_grow_with_age() {
        let species = broadleaf();
        let top =
            |tree: &Skeleton| tree.branches.iter().flat_map(|b| b.points.iter()).map(|p| p.y).fold(f32::MIN, f32::max);
        let mature = grow(&species, &TreeParams::default());
        assert!((12.0..22.0).contains(&top(&mature)), "{}", top(&mature));
        let sapling = grow(&species, &TreeParams { age: 0.1, ..TreeParams::default() });
        assert!(top(&sapling) < top(&mature) / 3.0);
        assert!(sapling.branches.len() < mature.branches.len());
        assert!(!mature.leaves.is_empty());
    }

    #[test]
    fn branches_are_never_thicker_than_where_they_grow() {
        let tree = grow(&broadleaf(), &TreeParams::default());
        for branch in &tree.branches {
            let Some(parent) = branch.parent else { continue };
            let thickest_parent = tree.branches[parent].radii.iter().copied().fold(0.0, f32::max);
            assert!(branch.radii[0] <= thickest_parent + 1e-4);
        }
    }
}
