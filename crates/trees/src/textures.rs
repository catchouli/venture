//! A species' textures: its bark, and its sprig atlas for the leaf cards
//! (painted by [`crate::sprigs`]).
//!
//! Bark is a photograph if the species names one (see [`crate::photos`]).
//! Otherwise it's painted as grey detail: 1 where the bark's surface is,
//! darker in its cracks. The bark mesh's vertex colours then give it its
//! colour (see [`crate::mesh`]). Painted bark tiles both ways. Each style
//! is built from periodic value noise (Perlin, "An Image Synthesizer",
//! 1985, with a lattice that wraps), and Voronoi cells for plates (Worley,
//! "A Cellular Texture Basis Function", 1996).

use std::f32::consts::TAU;

use image::{Rgba, RgbaImage};

use crate::photos::Photos;
use crate::random::Random;
use crate::species::{BarkStyle, Colour, Species};
use crate::sprigs::paint_sprigs;

/// The painted bark texture's size, in pixels.
pub const BARK_SIZE: u32 = 256;

/// A species' textures.
#[derive(Clone, Debug)]
pub struct TreeTextures {
    /// The bark (tiling): a photograph, or grey detail for the vertex colours to colour.
    pub bark: RgbaImage,
    /// How tall the bark picture is for its width (so it isn't stretched).
    pub bark_aspect: f32,
    /// The sprig atlas.
    pub leaves: RgbaImage,
}

/// Makes a species' textures, from its photographs if it has any. The same
/// species (and photographs) always gets the same ones.
pub fn paint(species: &Species, photos: &Photos) -> TreeTextures {
    let seed = species
        .id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let bark = photos.bark.clone().unwrap_or_else(|| paint_bark(species, seed));
    let bark_aspect = bark.height() as f32 / bark.width().max(1) as f32;
    TreeTextures { bark, bark_aspect, leaves: paint_sprigs(&species.leaves, photos.leaves.as_ref(), seed) }
}

// --- Bark ---

/// A hash of a lattice point, from 0 to 1.
fn lattice(x: i32, y: i32, seed: u64) -> f32 {
    let mut random = Random::new(seed ^ ((x as u32 as u64) << 32 | y as u32 as u64).wrapping_mul(0x9e37_79b9));
    random.unit()
}

/// Smooth value noise at `(x, y)` that repeats every `(period_x, period_y)` lattice cells.
fn periodic_noise(x: f32, y: f32, period_x: i32, period_y: i32, seed: u64) -> f32 {
    let (cell_x, cell_y) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - cell_x as f32, y - cell_y as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |dx: i32, dy: i32| lattice((cell_x + dx).rem_euclid(period_x), (cell_y + dy).rem_euclid(period_y), seed);
    let top = at(0, 0) * (1.0 - sx) + at(1, 0) * sx;
    let bottom = at(0, 1) * (1.0 - sx) + at(1, 1) * sx;
    top * (1.0 - sy) + bottom * sy
}

/// Several octaves of periodic noise at `(u, v)` (0 to 1 across the
/// texture), starting from `(cells_x, cells_y)` cells across it.
fn fbm(u: f32, v: f32, cells_x: i32, cells_y: i32, octaves: u32, seed: u64) -> f32 {
    let (mut total, mut weight, mut sum) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        let scale = 1 << octave;
        let (px, py) = (cells_x * scale, cells_y * scale);
        total += weight * periodic_noise(u * px as f32, v * py as f32, px, py, seed + u64::from(octave));
        sum += weight;
        weight *= 0.5;
    }
    total / sum
}

/// The distances to the nearest and second-nearest of `cells_x × cells_y`
/// jittered points (repeating), at `(u, v)`, in cells.
fn voronoi(u: f32, v: f32, cells_x: i32, cells_y: i32, seed: u64) -> (f32, f32) {
    let (x, y) = (u * cells_x as f32, v * cells_y as f32);
    let (cell_x, cell_y) = (x.floor() as i32, y.floor() as i32);
    let (mut nearest, mut second) = (f32::MAX, f32::MAX);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (cell_x + dx, cell_y + dy);
            let (wx, wy) = (cx.rem_euclid(cells_x), cy.rem_euclid(cells_y));
            let point_x = cx as f32 + 0.1 + 0.8 * lattice(wx, wy, seed);
            let point_y = cy as f32 + 0.1 + 0.8 * lattice(wx, wy, seed + 7);
            let distance = (point_x - x).hypot(point_y - y);
            if distance < nearest {
                second = nearest;
                nearest = distance;
            } else if distance < second {
                second = distance;
            }
        }
    }
    (nearest, second)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn luminance(colour: Colour) -> f32 {
    0.2126 * colour.0 + 0.7152 * colour.1 + 0.0722 * colour.2
}

/// How much of the surface shows at `(u, v)`: 1 on the surface, 0 in a crack.
fn bark_surface(style: BarkStyle, u: f32, v: f32, seed: u64) -> f32 {
    match style {
        BarkStyle::Fissured => {
            // Long vertical ridges: noise stretched up the trunk, cracks where it crosses the middle.
            let warp = fbm(u, v, 4, 2, 3, seed + 11) - 0.5;
            let ridges = fbm(u + warp * 0.08, v, 10, 2, 4, seed);
            let crack = 1.0 - (2.0 * ridges - 1.0).abs();
            1.0 - smoothstep(0.72, 0.93, crack)
        }
        BarkStyle::Smooth => {
            let mottle = fbm(u, v, 4, 4, 4, seed);
            let line = smoothstep(0.93, 1.0, fbm(u, v, 1, 24, 2, seed + 3)) * 0.4;
            0.9 + 0.1 * mottle - line
        }
        BarkStyle::Papery => {
            // Short dark dashes across the trunk (lenticels), and the odd dark patch.
            let dash = fbm(u, v, 3, 48, 2, seed);
            let lenticel = smoothstep(0.7, 0.8, dash) * smoothstep(0.35, 0.6, fbm(u, v, 8, 8, 2, seed + 5));
            let patch = smoothstep(0.68, 0.8, fbm(u, v, 3, 3, 3, seed + 9));
            1.0 - (lenticel + patch).min(1.0) * 0.9
        }
        BarkStyle::Plated => {
            let warp = fbm(u, v, 3, 3, 2, seed + 13) - 0.5;
            let (nearest, second) = voronoi(u + warp * 0.05, v + warp * 0.05, 6, 4, seed);
            let border = smoothstep(0.02, 0.12, second - nearest);
            let plate = 0.8 + 0.2 * fbm(u, v, 6, 4, 2, seed + 17);
            border * plate
        }
        BarkStyle::Diamond => {
            // Ridges crossing in tall diamonds.
            let warp = (fbm(u, v, 4, 4, 3, seed) - 0.5) * 0.6;
            let a = ((u * 6.0 + v * 2.0) * TAU + warp * TAU).sin();
            let b = ((u * 6.0 - v * 2.0) * TAU + warp * TAU).sin();
            let crack = smoothstep(0.75, 0.97, a.max(b));
            1.0 - crack
        }
    }
}

fn paint_bark(species: &Species, seed: u64) -> RgbaImage {
    let bark = &species.bark;
    // The cracks' darkness, as a share of the surface's.
    let crack = (luminance(bark.colours.1) / luminance(bark.colours.0).max(0.01)).clamp(0.05, 1.0);
    RgbaImage::from_fn(BARK_SIZE, BARK_SIZE, |x, y| {
        let (u, v) = (x as f32 / BARK_SIZE as f32, y as f32 / BARK_SIZE as f32);
        let surface = bark_surface(bark.style, u, v, seed).clamp(0.0, 1.0);
        let grain = 0.88 + 0.12 * fbm(u, v, 16, 32, 2, seed + 21);
        let value = (crack + (1.0 - crack) * surface) * grain;
        let byte = to_srgb_byte(value);
        Rgba([byte, byte, byte, 255])
    })
}

/// A linear value (0 to 1) as an sRGB byte.
fn to_srgb_byte(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 { value * 12.92 } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::species::{Bark, CrownShape, Leaves, Trunk};

    fn species(style: BarkStyle) -> Species {
        Species {
            id: "test".to_string(),
            name: "Test".to_string(),
            height: (15.0, 20.0),
            trunk: Trunk::default(),
            crown: CrownShape::Spherical,
            branches: Vec::new(),
            leaves: Leaves::default(),
            bark: Bark { style, ..Bark::default() },
        }
    }

    #[test]
    fn painted_bark_tiles_in_every_style() {
        for style in [BarkStyle::Fissured, BarkStyle::Smooth, BarkStyle::Papery, BarkStyle::Plated, BarkStyle::Diamond]
        {
            let textures = paint(&species(style), &Photos::default());
            let bark = &textures.bark;
            // The left edge follows on from the right (it tiles).
            let seam: f32 = (0..BARK_SIZE)
                .map(|y| f32::from(bark.get_pixel(0, y)[0]) - f32::from(bark.get_pixel(BARK_SIZE - 1, y)[0]))
                .map(f32::abs)
                .sum::<f32>()
                / BARK_SIZE as f32;
            assert!(seam < 40.0, "{style:?}: the seam differs by {seam} on average");
            assert_eq!(textures.bark_aspect, 1.0);
        }
    }
}
