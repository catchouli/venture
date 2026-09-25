//! Overlays: edits applied on top of the procedural terrain.
//!
//! The generator lays down natural terrain; overlays then shape it for
//! places that need to be just so, such as a town's level ground and its
//! square. Overlays come from two sources, applied in this order:
//! 1. **Procedural**: stamps the generator creates itself (for example the
//!    ground each settlement is built on).
//! 2. **Authored**: stamps written by designers, for hand-made places.
//!    Later these will be loaded from data files.
//!
//! Stamps are applied in order, so later stamps win where they overlap.
//! See `docs/world-generation-engine.md`.

use venture_world::Ground;

use crate::geometry::Point;
use crate::noise_fields::{lerp, smoothstep};
use crate::spatial::Buckets;

/// Stamps are grouped into buckets this many metres wide.
const BUCKET_SIZE: f32 = 128.0;

/// One edit to the terrain.
#[derive(Clone, Debug, PartialEq)]
pub enum Stamp {
    /// Levels the ground inside a circle, blending into the surrounding land
    /// over `falloff` metres.
    Flatten {
        /// Centre of the circle, in metres.
        centre: Point,
        /// Radius of the fully level area.
        radius: f32,
        /// Width of the blend into the surrounding land.
        falloff: f32,
        /// Height to level to. `None` uses the natural (coarse) terrain height
        /// at the centre, which disturbs the landscape least.
        height: Option<f32>,
    },
    /// Covers the ground inside a circle with a material.
    Paint {
        /// Centre of the circle, in metres.
        centre: Point,
        /// Radius of the circle.
        radius: f32,
        /// The material to paint.
        ground: Ground,
    },
}

impl Stamp {
    /// The stamp's centre.
    fn centre(&self) -> Point {
        match self {
            Stamp::Flatten { centre, .. } | Stamp::Paint { centre, .. } => *centre,
        }
    }

    /// How far from its centre the stamp has any effect.
    fn reach(&self) -> f32 {
        match self {
            Stamp::Flatten { radius, falloff, .. } => radius + falloff,
            Stamp::Paint { radius, .. } => *radius,
        }
    }
}

/// All the stamps in a world, ready to apply.
pub struct Overlay {
    stamps: Buckets<(usize, Stamp)>,
    max_reach: f32,
}

impl Overlay {
    /// Builds an overlay from stamps, in the order they should apply.
    /// `natural_height` gives the coarse terrain height, for flatten stamps
    /// without a fixed height.
    pub fn new(world_size: f32, stamps: Vec<Stamp>, natural_height: impl Fn(Point) -> f32) -> Self {
        let mut buckets = Buckets::new(world_size, BUCKET_SIZE);
        let mut max_reach = 0.0_f32;
        for (order, mut stamp) in stamps.into_iter().enumerate() {
            if let Stamp::Flatten { centre, height, .. } = &mut stamp {
                height.get_or_insert_with(|| natural_height(*centre));
            }
            max_reach = max_reach.max(stamp.reach());
            buckets.insert(stamp.centre(), (order, stamp));
        }
        Self { stamps: buckets, max_reach }
    }

    /// The stamps that could affect the area from `min` to `max`, in the order they apply.
    pub fn near(&self, min: Point, max: Point) -> LocalOverlay {
        let mut stamps = self.stamps.near(min, max, self.max_reach);
        stamps.sort_by_key(|(order, _)| *order);
        LocalOverlay { stamps: stamps.into_iter().map(|(_, stamp)| stamp).collect() }
    }
}

/// The stamps affecting one area.
pub struct LocalOverlay {
    stamps: Vec<Stamp>,
}

impl LocalOverlay {
    /// Applies every flatten stamp to the terrain height at `(x, y)`.
    pub fn shape_height(&self, x: f32, y: f32, mut height: f32) -> f32 {
        for stamp in &self.stamps {
            if let Stamp::Flatten { centre, radius, falloff, height: Some(target) } = stamp {
                let distance = (x - centre.0).hypot(y - centre.1);
                if distance >= radius + falloff {
                    continue;
                }
                let keep = smoothstep(*radius, radius + falloff.max(0.01), distance);
                height = lerp(*target, height, keep);
            }
        }
        height
    }

    /// The material painted at `(x, y)` by the last paint stamp covering it, if any.
    pub fn painted_ground(&self, x: f32, y: f32) -> Option<Ground> {
        self.stamps.iter().rev().find_map(|stamp| match stamp {
            Stamp::Paint { centre, radius, ground } if (x - centre.0).hypot(y - centre.1) < *radius => Some(*ground),
            _ => None,
        })
    }

    /// Whether any stamp at all affects `(x, y)`: used to keep scattered props
    /// off designed places.
    pub fn covers(&self, x: f32, y: f32) -> bool {
        self.stamps.iter().any(|stamp| {
            let centre = stamp.centre();
            (x - centre.0).hypot(y - centre.1) < stamp.reach()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flatten(centre: Point, height: Option<f32>) -> Stamp {
        Stamp::Flatten { centre, radius: 10.0, falloff: 10.0, height }
    }

    #[test]
    fn flatten_levels_inside_and_leaves_outside() {
        let overlay = Overlay::new(1000.0, vec![flatten((100.0, 100.0), Some(5.0))], |_| 0.0);
        let local = overlay.near((0.0, 0.0), (200.0, 200.0));
        assert_eq!(local.shape_height(105.0, 100.0, 9.0), 5.0);
        assert_eq!(local.shape_height(150.0, 100.0, 9.0), 9.0);
    }

    #[test]
    fn flatten_without_height_uses_natural_height() {
        let overlay = Overlay::new(1000.0, vec![flatten((100.0, 100.0), None)], |_| 3.0);
        let local = overlay.near((0.0, 0.0), (200.0, 200.0));
        assert_eq!(local.shape_height(100.0, 100.0, 9.0), 3.0);
    }

    #[test]
    fn later_paint_wins() {
        let paint = |ground| Stamp::Paint { centre: (50.0, 50.0), radius: 5.0, ground };
        let overlay = Overlay::new(1000.0, vec![paint(Ground::Dirt), paint(Ground::Path)], |_| 0.0);
        let local = overlay.near((0.0, 0.0), (100.0, 100.0));
        assert_eq!(local.painted_ground(50.0, 50.0), Some(Ground::Path));
        assert_eq!(local.painted_ground(70.0, 50.0), None);
    }
}
