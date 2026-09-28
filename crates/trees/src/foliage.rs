//! Foliage textures: a square of a species' own sprigs (from its leaf
//! atlas), densely overlapping at every angle, for surfaces covered in
//! leaves, such as clipped hedges. It tiles: sprigs running off one edge come
//! back on the other.
//!
//! Sprigs are laid in layers, the first (inner) ones darker. Each pixel
//! keeps how far out its leaf is (its layer, rising towards the sprig's
//! tip), which gives a normal map (the leaves' relief) and ambient
//! occlusion (dark down between them, where the light doesn't reach).

use glam::Vec2;
use image::{Rgba, RgbaImage};

use crate::mesh::sprig_cell;
use crate::random::Random;
use crate::skeleton::SPRIG_PICTURES;

/// A foliage texture's pictures, in the game's layout (see `docs/textures.md`).
pub struct Foliage {
    /// Its colour (sRGB).
    pub colour: RgbaImage,
    /// Its normal map (OpenGL's way up: green is up the picture).
    pub normal: RgbaImage,
    /// Ambient occlusion in red, roughness in green.
    pub surface: RgbaImage,
}

/// How to make a foliage texture.
#[derive(Clone, Copy, Debug)]
pub struct FoliageSettings {
    /// Its size, in pixels across.
    pub size: u32,
    /// How many metres it covers.
    pub metres: f32,
    /// How long a sprig is, in metres.
    pub sprig: f32,
    /// How many times over the sprigs cover it.
    pub cover: f32,
    /// The seed.
    pub seed: u64,
}

impl Default for FoliageSettings {
    fn default() -> Self {
        Self { size: 2048, metres: 2.0, sprig: 0.6, cover: 10.0, seed: 1 }
    }
}

/// How much of a sprig's card its leaves cover, roughly (to know how many cover the texture).
const SPRIG_COVERAGE: f32 = 0.4;
/// How bright the innermost sprigs are, next to the outermost.
const INNER_LIGHT: f32 = 0.5;
/// How steep the relief is, for the normal map.
const RELIEF: f32 = 5.0;
/// The leaves' roughness (0 to 1).
const ROUGHNESS: f32 = 0.9;

/// Makes a foliage texture from a species' leaf atlas (see
/// [`crate::textures::TreeTextures::leaves`]), with `shade` (sRGB) showing
/// between the leaves.
pub fn foliage(atlas: &RgbaImage, shade: [u8; 3], settings: FoliageSettings) -> Foliage {
    let size = settings.size.max(16);
    let pixels = (size * size) as usize;
    let mut random = Random::new(settings.seed);
    let sprig = (settings.sprig / settings.metres * size as f32).max(4.0);
    let count = ((settings.cover * (size * size) as f32) / (SPRIG_COVERAGE * sprig * sprig)).ceil() as usize;

    // What's between the leaves, then every sprig in turn, inner to outer.
    let mut colour = vec![[shade[0] as f32, shade[1] as f32, shade[2] as f32]; pixels];
    let mut height = vec![0.0f32; pixels];
    for index in 0..count {
        let layer = (index as f32 + 1.0) / count as f32;
        let centre = Vec2::new(random.unit(), random.unit()) * size as f32;
        let angle = random.unit() * std::f32::consts::TAU;
        let length = sprig * (0.75 + 0.5 * random.unit());
        let [u0, u1, v_top, v_bottom] = sprig_cell((random.next_u64() % u64::from(SPRIG_PICTURES)) as u32);
        let light = INNER_LIGHT + (1.0 - INNER_LIGHT) * layer.sqrt();
        let tint = [1.0 + random.signed() * 0.06, 1.0 + random.signed() * 0.05, 1.0 + random.signed() * 0.06];
        let (sin, cos) = angle.sin_cos();
        let reach = (length * 0.75).ceil() as i32;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                // Where this pixel is on the sprig's card: across (-0.5 to 0.5) and along (0 at its base, 1 at its tip).
                let (x, y) = (dx as f32, dy as f32);
                let across = (x * cos + y * sin) / length;
                let along = (-x * sin + y * cos) / length + 0.5;
                if !(-0.5..0.5).contains(&across) || !(0.0..1.0).contains(&along) {
                    continue;
                }
                let u = u0 + (across + 0.5) * (u1 - u0);
                let v = v_bottom - along * (v_bottom - v_top);
                let sample = sample(atlas, u, v);
                if sample[3] < 0.5 {
                    continue;
                }
                let px = (centre.x as i32 + dx).rem_euclid(size as i32) as usize;
                let py = (centre.y as i32 + dy).rem_euclid(size as i32) as usize;
                let at = py * size as usize + px;
                colour[at] = [0, 1, 2].map(|c| sample[c] * 255.0 * light * tint[c]);
                // Further out towards the tip, as a sprig stands out of the hedge.
                height[at] = layer * 0.8 + along * 0.2;
            }
        }
    }

    let mut foliage = Foliage {
        colour: RgbaImage::new(size, size),
        normal: RgbaImage::new(size, size),
        surface: RgbaImage::new(size, size),
    };
    let wrap = |x: i64, y: i64| {
        let (x, y) = (x.rem_euclid(i64::from(size)), y.rem_euclid(i64::from(size)));
        height[(y * i64::from(size) + x) as usize]
    };
    for y in 0..size {
        for x in 0..size {
            let at = (y * size + x) as usize;
            let [r, g, b] = colour[at].map(|c| c.clamp(0.0, 255.0) as u8);
            foliage.colour.put_pixel(x, y, Rgba([r, g, b, 255]));
            // The relief's slope (the picture's rows go down; the normal map's green goes up).
            let (x, y) = (i64::from(x), i64::from(y));
            let slope_x = (wrap(x + 1, y) - wrap(x - 1, y)) * 0.5 * RELIEF;
            let slope_y = (wrap(x, y + 1) - wrap(x, y - 1)) * 0.5 * RELIEF;
            let normal = glam::Vec3::new(-slope_x, slope_y, 1.0).normalize();
            let [nx, ny, nz] = normal.to_array().map(|n| ((n * 0.5 + 0.5) * 255.0).round() as u8);
            foliage.normal.put_pixel(x as u32, y as u32, Rgba([nx, ny, nz, 255]));
            let occlusion = ((0.3 + 0.7 * height[at].sqrt()) * 255.0) as u8;
            foliage.surface.put_pixel(x as u32, y as u32, Rgba([occlusion, (ROUGHNESS * 255.0) as u8, 0, 255]));
        }
    }
    foliage
}

/// The atlas's colour (0 to 1, straight alpha) at `(u, v)`, nearest pixel.
fn sample(atlas: &RgbaImage, u: f32, v: f32) -> [f32; 4] {
    let x = ((u * atlas.width() as f32) as u32).min(atlas.width() - 1);
    let y = ((v * atlas.height() as f32) as u32).min(atlas.height() - 1);
    atlas.get_pixel(x, y).0.map(|c| c as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An atlas whose every sprig is a solid green card.
    fn solid_atlas() -> RgbaImage {
        RgbaImage::from_pixel(64, 64, Rgba([60, 120, 40, 255]))
    }

    #[test]
    fn foliage_covers_the_texture_and_tiles() {
        let settings = FoliageSettings { size: 128, ..FoliageSettings::default() };
        let foliage = foliage(&solid_atlas(), [5, 10, 3], settings);
        let shade = foliage.colour.pixels().filter(|pixel| pixel.0[..3] == [5, 10, 3]).count();
        assert!(shade < 128 * 128 / 50, "leaves cover nearly all of it ({shade} bare pixels)");
        // The same seed makes the same texture.
        assert_eq!(foliage.colour, super::foliage(&solid_atlas(), [5, 10, 3], settings).colour);
        // Its normals point out of it.
        assert!(foliage.normal.pixels().all(|pixel| pixel.0[2] > 128));
    }
}
