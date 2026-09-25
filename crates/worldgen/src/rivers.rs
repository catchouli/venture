//! River geometry: segments between world map points, and how they carve the land.

use crate::WorldGenConfig;
use crate::geometry::closest_on_segment;
use crate::noise_fields::{lerp, smoothstep};

/// One end of a river segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverPoint {
    /// Tile x coordinate.
    pub x: f32,
    /// Tile y coordinate.
    pub y: f32,
    /// Height of the water surface.
    pub surface: f32,
    /// Half the river's width.
    pub half_width: f32,
    /// Depth of the channel below the surface, at the river's centre.
    pub depth: f32,
    /// Width of the flattened land beside the river.
    pub floodplain_width: f32,
}

impl RiverPoint {
    /// A river point sized by how much water flows through it.
    pub fn new(config: &WorldGenConfig, x: f32, y: f32, surface: f32, flow: u32) -> Self {
        let size = river_size(config, flow);
        Self {
            x,
            y,
            surface,
            half_width: lerp(config.river_min_half_width, config.river_max_half_width, size),
            depth: lerp(config.river_min_depth, config.river_max_depth, size),
            floodplain_width: config.floodplain_width * size,
        }
    }
}

/// How big a river is, from `0.0` (just became a river) to `1.0` (full size).
fn river_size(config: &WorldGenConfig, flow: u32) -> f32 {
    let range = config.river_full_flow.saturating_sub(config.river_min_flow).max(1);
    let over = flow.saturating_sub(config.river_min_flow);
    (over as f32 / range as f32).clamp(0.0, 1.0).sqrt()
}

/// A straight stretch of river, flowing from `start` to `end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverSegment {
    /// Upstream end.
    pub start: RiverPoint,
    /// Downstream end.
    pub end: RiverPoint,
}

/// A river's properties at the point on it closest to some position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverSample {
    /// Distance from the position to the river's centre line.
    pub distance: f32,
    /// Height of the water surface.
    pub surface: f32,
    /// Half the river's width.
    pub half_width: f32,
    /// Channel depth at the centre.
    pub depth: f32,
    /// Width of the flattened land beside the river.
    pub floodplain_width: f32,
}

impl RiverSegment {
    /// Samples the river at its closest point to `(x, y)`.
    pub fn sample(&self, x: f32, y: f32) -> RiverSample {
        let (a, b) = (self.start, self.end);
        let (t, distance) = closest_on_segment((a.x, a.y), (b.x, b.y), (x, y));
        RiverSample {
            distance,
            surface: lerp(a.surface, b.surface, t),
            half_width: lerp(a.half_width, b.half_width, t),
            depth: lerp(a.depth, b.depth, t),
            floodplain_width: lerp(a.floodplain_width, b.floodplain_width, t),
        }
    }
}

impl RiverSample {
    /// Distance from the position to the river's edge (negative inside the river).
    pub fn distance_to_edge(&self) -> f32 {
        self.distance - self.half_width
    }

    /// Lowers `land` to form the river channel and its flat floodplain.
    /// Rivers only ever carve: land is never raised.
    pub fn carve(&self, land: f32, bank_height: f32) -> f32 {
        let bank = self.surface + bank_height;

        // Flatten the land towards bank height near the river.
        let plain_end = self.half_width + self.floodplain_width;
        let flatness = 1.0 - smoothstep(self.half_width, plain_end.max(self.half_width + 1.0), self.distance);
        let flattened = if land > bank { lerp(land, bank, flatness) } else { land };
        if self.distance >= self.half_width {
            return flattened;
        }

        // A rounded channel: bank height at the edge, full depth at the centre.
        let across = self.distance / self.half_width;
        let channel = lerp(bank, self.surface - self.depth, 1.0 - across * across);
        flattened.min(channel)
    }
}

/// The river whose edge is closest to `(x, y)`, if any.
///
/// Ties go to the segment that comes first, so callers must pass segments in
/// a consistent order to get identical results everywhere.
pub fn nearest_river(segments: &[RiverSegment], x: f32, y: f32) -> Option<RiverSample> {
    let mut nearest: Option<RiverSample> = None;
    for segment in segments {
        let sample = segment.sample(x, y);
        if nearest.is_none_or(|best| sample.distance_to_edge() < best.distance_to_edge()) {
            nearest = Some(sample);
        }
    }
    nearest
}
