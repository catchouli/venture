//! Props: trees, rocks and bushes standing on the terrain.

/// What kind of prop something is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropKind {
    /// A broad, leafy tree.
    Oak,
    /// A slender, pale-barked tree.
    Birch,
    /// A tall conifer.
    Pine,
    /// A bare, dead tree.
    DeadTree,
    /// A large rock.
    Boulder,
    /// A low bush.
    Bush,
}

/// One prop placed in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prop {
    /// What it is.
    pub kind: PropKind,
    /// Position along the east axis, in metres.
    pub x: f32,
    /// Position along the south axis, in metres.
    pub y: f32,
    /// Height of the ground at its base, in metres.
    pub ground_height: f32,
    /// Size relative to the kind's normal size.
    pub scale: f32,
    /// Rotation around the vertical axis, in radians.
    pub rotation: f32,
}
