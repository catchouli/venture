//! Conversions between world coordinates and Bevy's 3D space.
//!
//! World positions are in metres: `x` east, `y` south, plus a height. In
//! Bevy, world `x` is the X axis, world `y` is the Z axis, and height is the Y
//! (up) axis. This is the only place that mapping is written down.

use bevy::math::Vec3;

/// The Bevy position of world point `(x, y)` at `height`.
pub fn to_bevy(x: f32, y: f32, height: f32) -> Vec3 {
    Vec3::new(x, height, y)
}

/// The horizontal world coordinates `(x, y)` of a Bevy position.
pub fn from_bevy(position: Vec3) -> (f32, f32) {
    (position.x, position.z)
}
