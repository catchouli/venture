//! Visibility ranges (levels of detail by distance) that work everywhere.
//!
//! A range that crossfades (its margins aren't empty) is drawn with a
//! dither, whose shader reads the visibility ranges' buffer. On WebGL2 Bevy
//! 0.19 lays that buffer out too small for the shader (a 16-byte binding for
//! a 64-element uniform array), so drawing anything that crossfades fails
//! there. On the web, ranges switch at the middle of their margins instead.

use bevy::camera::visibility::VisibilityRange;

/// A range that works on this platform: as it is on the desktop, and
/// without its crossfades on the web.
pub fn web_safe(range: VisibilityRange) -> VisibilityRange {
    #[cfg(target_arch = "wasm32")]
    {
        let middle = |margin: &std::ops::Range<f32>| (margin.start + margin.end) / 2.0;
        let (start, end) = (middle(&range.start_margin), middle(&range.end_margin));
        VisibilityRange { start_margin: start..start, end_margin: end..end, ..range }
    }
    #[cfg(not(target_arch = "wasm32"))]
    range
}
