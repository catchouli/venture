//! What a species of tree is like: its shape, how it branches, its leaves
//! and bark, as plain data (`content/trees/species.ron`). The parameters
//! follow Weber and Penn's parametric trees (see [`crate::skeleton`]),
//! named for what they look like rather than by their paper's symbols.
//!
//! Every size is for a mature tree of the species, in metres; age scales
//! them (see [`crate::TreeParams`]).

use serde::{Deserialize, Serialize};

/// A species of tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Species {
    /// The species' id (such as `oak`).
    pub id: String,
    /// Its name, for people (such as "Oak").
    pub name: String,
    /// How tall mature trees grow, from the shortest to the tallest, in metres.
    pub height: (f32, f32),
    /// The trunk (or stems).
    pub trunk: Trunk,
    /// The crown's outline, which sets how long the main branches are along the trunk.
    pub crown: CrownShape,
    /// The levels of branches, from the ones on the trunk outwards (up to
    /// three). The last level carries the leaves.
    pub branches: Vec<BranchLevel>,
    /// The leaves.
    pub leaves: Leaves,
    /// The bark.
    pub bark: Bark,
}

/// The trunk, or the stems of a tree with several.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct Trunk {
    /// How many stems grow from the ground, from the fewest to the most (1
    /// for a single trunk; hazel has many).
    pub stems: (u32, u32),
    /// How far stems lean out from upright, when there are several, in degrees.
    pub stem_spread: f32,
    /// The trunk's radius at the ground (above the flare), as a share of the tree's height.
    pub radius: f32,
    /// How much the trunk narrows to its top: 0 not at all, 1 to a point.
    pub taper: f32,
    /// How much wider the trunk spreads at the ground, into its roots (0 none, 1 about twice as wide).
    pub flare: f32,
    /// Ridges down the trunk (fluting, as on old yews): how many, and how deep (a share of the radius).
    pub lobes: (u32, f32),
    /// The share of the trunk's height with no branches (a clear stem).
    pub clear: f32,
    /// How much the trunk wanders as it grows, in degrees a segment.
    pub wander: f32,
    /// The most a trunk leans, in degrees.
    pub lean: f32,
    /// Where the trunk splits into big limbs, as a share of its height (0: it doesn't).
    pub fork_at: f32,
    /// How many limbs it splits into, from the fewest to the most.
    pub fork_limbs: (u32, u32),
    /// How far limbs spread from the trunk's line, in degrees.
    pub fork_angle: f32,
    /// How many segments make the trunk (more is smoother).
    pub segments: u32,
}

impl Default for Trunk {
    fn default() -> Self {
        Self {
            stems: (1, 1),
            stem_spread: 20.0,
            radius: 0.025,
            taper: 0.9,
            flare: 0.4,
            lobes: (0, 0.0),
            clear: 0.3,
            wander: 3.0,
            lean: 4.0,
            fork_at: 0.0,
            fork_limbs: (2, 3),
            fork_angle: 30.0,
            segments: 12,
        }
    }
}

/// A crown's outline, seen from the side: how long the branches on the
/// trunk are at each height (after Weber and Penn's shapes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum CrownShape {
    /// Widest at the bottom, to a point at the top (young conifers).
    Conical,
    /// Round.
    Spherical,
    /// A dome, widest low down.
    Hemispherical,
    /// The same width all the way up.
    Cylindrical,
    /// Narrowing a little towards the top.
    TaperedCylindrical,
    /// Widest two-thirds of the way up, like a candle flame.
    Flame,
    /// Widest at the top (flat-topped old pines, parasols).
    InverseConical,
    /// A flame, but fuller low down.
    TendFlame,
}

impl CrownShape {
    /// How long a branch is (a share of the longest) at `height`, from 0 at
    /// the bottom of the crown to 1 at the top.
    pub fn ratio(self, height: f32) -> f32 {
        let height = height.clamp(0.0, 1.0);
        match self {
            CrownShape::Conical => 0.2 + 0.8 * (1.0 - height),
            CrownShape::Spherical => 0.2 + 0.8 * (std::f32::consts::PI * height).sin(),
            CrownShape::Hemispherical => 0.2 + 0.8 * (std::f32::consts::FRAC_PI_2 * (1.0 - height)).sin(),
            CrownShape::Cylindrical => 1.0,
            CrownShape::TaperedCylindrical => 0.5 + 0.5 * (1.0 - height),
            CrownShape::Flame if height <= 0.7 => height / 0.7,
            CrownShape::Flame => (1.0 - height) / 0.3,
            CrownShape::InverseConical => 1.0 - 0.8 * (1.0 - height),
            CrownShape::TendFlame if height <= 0.7 => 0.5 + 0.5 * height / 0.7,
            CrownShape::TendFlame => 0.5 + 0.5 * (1.0 - height) / 0.3,
        }
    }
}

/// How branches sit around their parent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Arrangement {
    /// One at a time, each turned by the golden angle (137.5°) from the last
    /// (oak, beech, birch...).
    Spiral,
    /// In opposite pairs, each pair turned a right angle from the last (ash, sycamore).
    Opposite,
    /// In rings of this many (pines).
    Whorled(u32),
}

/// One level of branches: those growing from the level before (the trunk,
/// for the first).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct BranchLevel {
    /// How many branches grow from each parent (on a whole, mature parent).
    pub count: f32,
    /// How they sit around the parent.
    pub arrangement: Arrangement,
    /// Where along the parent they start, as a share of its length (the
    /// trunk's clear stem comes first anyway).
    pub start: f32,
    /// The angle between a branch and its parent where it grows, in degrees.
    pub angle: f32,
    /// How much that angle varies, in degrees either way.
    pub angle_variation: f32,
    /// The angle for branches at the top of the parent, in degrees (the
    /// angle changes smoothly from `angle` at the bottom to this).
    pub top_angle: f32,
    /// How long branches are, as a share of the parent's length.
    pub length: f32,
    /// How much their length varies, as a share either way.
    pub length_variation: f32,
    /// How much a branch curves over its length, in degrees (positive curves up and in).
    pub curve: f32,
    /// How much it curves back again over its outer half, in degrees (an S-bend).
    pub curve_back: f32,
    /// How much a branch wanders, in degrees a segment.
    pub wander: f32,
    /// How much a branch bends towards the sky (positive) or the ground
    /// (negative) as it grows: strongly negative for weeping shoots.
    pub tropism: f32,
    /// How many segments make each branch.
    pub segments: u32,
}

impl Default for BranchLevel {
    fn default() -> Self {
        Self {
            count: 20.0,
            arrangement: Arrangement::Spiral,
            start: 0.0,
            angle: 50.0,
            angle_variation: 10.0,
            top_angle: 40.0,
            length: 0.4,
            length_variation: 0.2,
            curve: 0.0,
            curve_back: 0.0,
            wander: 8.0,
            tropism: 0.0,
            segments: 6,
        }
    }
}

/// The shape of a single leaf (or needle), for painting sprigs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum LeafShape {
    /// Rounded lobes down each side (oak, hawthorn: how many a side).
    Lobed(u32),
    /// A compound leaf: pairs of leaflets along a stalk (ash: how many pairs).
    Pinnate(u32),
    /// An oval (beech, hazel).
    Oval,
    /// A hand of pointed lobes (sycamore: how many).
    Palmate(u32),
    /// Long and narrow, pointed at both ends (willow).
    Lanceolate,
    /// Like a spade, broad at the base and pointed (birch).
    Triangular,
    /// Long needles in bundles (pines).
    Needles,
    /// Short, flat needles in two rows along the twig (yew).
    FlatNeedles,
}

/// An sRGB colour, each part from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Colour(pub f32, pub f32, pub f32);

impl Colour {
    /// Mixed with another, `t` of the way to it.
    pub fn mix(self, other: Colour, t: f32) -> Colour {
        Colour(self.0 * (1.0 - t) + other.0 * t, self.1 * (1.0 - t) + other.1 * t, self.2 * (1.0 - t) + other.2 * t)
    }

    /// Made lighter or darker (1 is unchanged).
    pub fn scaled(self, factor: f32) -> Colour {
        Colour(self.0 * factor, self.1 * factor, self.2 * factor)
    }

    /// As 8-bit sRGB.
    pub fn to_bytes(self) -> [u8; 3] {
        let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(self.0), byte(self.1), byte(self.2)]
    }
}

/// The leaves, drawn as cards carrying a painted sprig (a twig with its leaves).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct Leaves {
    /// Each leaf's shape.
    pub shape: LeafShape,
    /// How long a leaf is, in metres (a leaflet's parent leaf, for pinnate ones).
    pub size: f32,
    /// How wide a leaf is, as a share of its length.
    pub width: f32,
    /// How big a card (a sprig) is, in metres.
    pub card_size: f32,
    /// How many cards a metre of the outermost branches carries.
    pub per_metre: f32,
    /// How much cards hang down, from 0 (every way) to 1 (straight down, as willow's curtains).
    pub droop: f32,
    /// How long hanging cards are compared with their width (1 for square
    /// sprigs; longer for willow's strands).
    pub stretch: f32,
    /// The leaves' colours, from the lightest to the darkest.
    pub colours: (Colour, Colour),
    /// The twigs' colour in the sprigs.
    pub twig: Colour,
    /// A photographed leaf set to draw the leaves with, instead of painting
    /// them from their shape, if any. (Pine needles are always painted.)
    pub photo: Option<LeafPhoto>,
}

/// A photographed leaf set: a picture of separate leaves on a see-through
/// background (see [`crate::photos`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct LeafPhoto {
    /// The picture, in the content (such as `trees/leaves/LeafSet016.png`).
    pub file: String,
    /// Which way the leaves' tips point in the picture.
    pub tips: Direction,
}

/// A way in a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Direction {
    /// Towards the top.
    Up,
    /// Towards the bottom.
    Down,
    /// Towards the left.
    Left,
    /// Towards the right.
    Right,
}

impl Default for Leaves {
    fn default() -> Self {
        Self {
            shape: LeafShape::Oval,
            size: 0.08,
            width: 0.5,
            card_size: 0.6,
            per_metre: 6.0,
            droop: 0.2,
            stretch: 1.0,
            colours: (Colour(0.38, 0.52, 0.2), Colour(0.2, 0.33, 0.1)),
            twig: Colour(0.35, 0.28, 0.2),
            photo: None,
        }
    }
}

/// The bark's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum BarkStyle {
    /// Deep vertical cracks between ridges (oak, old ash, willow).
    Fissured,
    /// Smooth, faintly mottled (beech, hazel).
    Smooth,
    /// Pale and papery, with dark bands and diamonds (birch).
    Papery,
    /// Broken into flakes and plates (sycamore, pine, yew).
    Plated,
    /// Criss-crossing ridges in diamonds (old ash, willow).
    Diamond,
}

/// The bark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct Bark {
    /// Its look.
    pub style: BarkStyle,
    /// Its colours, the surface and the cracks.
    pub colours: (Colour, Colour),
    /// How far the texture spans, in metres (bigger for coarse bark).
    pub scale: f32,
    /// A colour the bark turns towards up the tree (Scots pine's orange), if any.
    pub top_colour: Option<Colour>,
    /// A photographed bark to use instead of painting one: its picture in
    /// the content (such as `trees/bark/Bark001.jpg`), if any.
    pub texture: Option<String>,
}

impl Default for Bark {
    fn default() -> Self {
        Self {
            style: BarkStyle::Fissured,
            colours: (Colour(0.42, 0.37, 0.32), Colour(0.16, 0.13, 0.11)),
            scale: 1.0,
            top_colour: None,
            texture: None,
        }
    }
}

/// Reads species from RON (a list of them).
pub fn parse_species(text: &str) -> Result<Vec<Species>, String> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(text)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crown_shapes_stay_between_nothing_and_whole() {
        let shapes = [
            CrownShape::Conical,
            CrownShape::Spherical,
            CrownShape::Hemispherical,
            CrownShape::Cylindrical,
            CrownShape::TaperedCylindrical,
            CrownShape::Flame,
            CrownShape::InverseConical,
            CrownShape::TendFlame,
        ];
        for shape in shapes {
            for step in 0..=20 {
                let ratio = shape.ratio(step as f32 / 20.0);
                assert!((0.0..=1.0 + 1e-5).contains(&ratio), "{shape:?} at {step}: {ratio}");
            }
        }
    }
}
