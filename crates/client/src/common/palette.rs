//! Colours for the world. The ground is vertex-coloured by material, then
//! multiplied by a fine detail texture.

use bevy::color::{Color, LinearRgba};
use bevy::math::Vec4;
use venture_world::{Ground, TerrainCell};

/// Sky and distance haze: a warm, slightly green-gold mist.
pub const HAZE: Color = Color::srgb(0.78, 0.80, 0.68);

/// Water surface colour (translucent).
pub const WATER: Color = Color::srgba(0.20, 0.36, 0.42, 0.72);

/// Lava surface colour.
pub const LAVA: Color = Color::srgb(0.25, 0.04, 0.01);

/// How brightly lava glows.
pub const LAVA_GLOW: LinearRgba = LinearRgba::rgb(0.9, 0.16, 0.01);

/// The debug camera's target marker.
pub const TARGET_MARKER: Color = Color::srgb(0.85, 0.12, 0.1);

/// How much each cell's brightness may randomly differ.
const CELL_VARIATION: f32 = 0.06;

/// The colour of a ground material.
fn ground_base(ground: Ground) -> Color {
    match ground {
        Ground::Grass => Color::srgb(0.36, 0.44, 0.21),
        Ground::MeadowGrass => Color::srgb(0.40, 0.52, 0.23),
        Ground::Dirt => Color::srgb(0.45, 0.36, 0.24),
        Ground::Sand => Color::srgb(0.76, 0.68, 0.50),
        Ground::Rock => Color::srgb(0.52, 0.50, 0.46),
        Ground::Snow => Color::srgb(0.92, 0.93, 0.95),
        Ground::Path => Color::srgb(0.50, 0.42, 0.31),
        Ground::Ash => Color::srgb(0.33, 0.31, 0.30),
        Ground::Basalt => Color::srgb(0.17, 0.16, 0.16),
    }
}

/// The colour of a terrain cell at world cell `(x, y)`, as linear RGBA.
pub fn ground_colour(cell: &TerrainCell, x: i32, y: i32) -> Vec4 {
    let brightness = 1.0 + (cell_hash(x, y) * 2.0 - 1.0) * CELL_VARIATION;
    let linear = ground_base(cell.ground).to_linear();
    Vec4::new(linear.red * brightness, linear.green * brightness, linear.blue * brightness, 1.0)
}

/// A repeatable pseudo-random number in `0.0..1.0` for integer coordinates.
pub fn cell_hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    (h & 0xffff) as f32 / 65536.0
}
