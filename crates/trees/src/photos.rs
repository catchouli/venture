//! Photographed bark and leaves (from the content: see
//! `docs/plan-trees.md`), and finding each leaf in a leaf set.
//!
//! A leaf set is a picture of several separate leaves on a see-through
//! background. Each leaf is a connected blob of opaque pixels, found by
//! flood fill on a coarse grid. Its long axis comes from the blob's second
//! moments (the principal axis of its pixels). Which end is the tip comes
//! from the species' data ([`LeafPhoto::tips`]), since every leaf in a set
//! lies the same way.

use std::path::Path;

use glam::Vec2;
use image::RgbaImage;

use crate::species::{Colour, Direction, Species};

/// Coverage above this counts as part of a leaf.
const OPAQUE: u8 = 100;
/// The grid leaves are found on is this many times coarser than the picture.
const GRID: u32 = 4;
/// Blobs smaller than this share of the picture aren't leaves (specks, stray twigs).
const SMALLEST_LEAF: f32 = 0.002;

/// A species' photographs, if it has any.
#[derive(Clone, Debug, Default)]
pub struct Photos {
    /// The bark (tiling), if the species names one.
    pub bark: Option<RgbaImage>,
    /// The leaves, if the species names a leaf set.
    pub leaves: Option<LeafPhotos>,
}

impl Photos {
    /// Loads a species' photographs from the content folder.
    pub fn load(species: &Species, content: &Path) -> Result<Photos, String> {
        let read = |file: &str| {
            let path = content.join(file);
            image::open(&path).map(|image| image.to_rgba8()).map_err(|error| format!("{}: {error}", path.display()))
        };
        let bark = species.bark.texture.as_deref().map(read).transpose()?;
        let leaves = match &species.leaves.photo {
            Some(photo) => Some(LeafPhotos::find(read(&photo.file)?, photo.tips)),
            None => None,
        };
        Ok(Photos { bark, leaves })
    }
}

/// A leaf set, and where each leaf is in it.
#[derive(Clone, Debug)]
pub struct LeafPhotos {
    /// The picture (sRGB, with coverage).
    pub image: RgbaImage,
    /// Each leaf.
    pub leaves: Vec<PhotoLeaf>,
    /// The leaves' average colour (sRGB, 0 to 1).
    pub average: Colour,
}

/// Where a leaf is in a leaf set, in pixels.
#[derive(Clone, Copy, Debug)]
pub struct PhotoLeaf {
    /// Its stalk's end.
    pub base: Vec2,
    /// The way from its stalk to its tip (a unit vector).
    pub along: Vec2,
    /// How long it is.
    pub length: f32,
}

impl LeafPhotos {
    /// Finds the leaves in a leaf set, whose tips point `tips` in the picture.
    pub fn find(image: RgbaImage, tips: Direction) -> LeafPhotos {
        let (width, height) = (image.width() / GRID, image.height() / GRID);
        let opaque = |x: u32, y: u32| image.get_pixel(x * GRID + GRID / 2, y * GRID + GRID / 2)[3] > OPAQUE;
        let mut seen = vec![false; (width * height) as usize];
        let mut leaves = Vec::new();
        let smallest = (SMALLEST_LEAF * (width * height) as f32) as usize;
        let way = match tips {
            Direction::Up => Vec2::NEG_Y,
            Direction::Down => Vec2::Y,
            Direction::Left => Vec2::NEG_X,
            Direction::Right => Vec2::X,
        };
        for start_y in 0..height {
            for start_x in 0..width {
                if seen[(start_y * width + start_x) as usize] || !opaque(start_x, start_y) {
                    continue;
                }
                // Flood fill the blob.
                let mut blob = Vec::new();
                let mut stack = vec![(start_x, start_y)];
                seen[(start_y * width + start_x) as usize] = true;
                while let Some((x, y)) = stack.pop() {
                    blob.push(Vec2::new(x as f32, y as f32));
                    let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                    for (nx, ny) in neighbours {
                        if nx < width && ny < height && !seen[(ny * width + nx) as usize] && opaque(nx, ny) {
                            seen[(ny * width + nx) as usize] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
                if blob.len() >= smallest {
                    leaves.push(leaf_from_blob(&blob, way));
                }
            }
        }
        let (mut sum, mut count) = ([0.0f32; 3], 0.0f32);
        for pixel in image.pixels().filter(|pixel| pixel[3] > OPAQUE) {
            for channel in 0..3 {
                sum[channel] += f32::from(pixel[channel]) / 255.0;
            }
            count += 1.0;
        }
        let count = count.max(1.0);
        let average = Colour(sum[0] / count, sum[1] / count, sum[2] / count);
        LeafPhotos { image, leaves, average }
    }
}

/// A leaf from its blob of grid cells: its long axis (turned to point
/// `way`), and its extent along it.
fn leaf_from_blob(blob: &[Vec2], way: Vec2) -> PhotoLeaf {
    let count = blob.len() as f32;
    let centre = blob.iter().copied().sum::<Vec2>() / count;
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for point in blob {
        let d = *point - centre;
        xx += d.x * d.x;
        xy += d.x * d.y;
        yy += d.y * d.y;
    }
    // The principal axis of the covariance matrix.
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    let mut along = Vec2::new(angle.cos(), angle.sin());
    if along.dot(way) < 0.0 {
        along = -along;
    }
    let (low, high) = blob.iter().fold((f32::MAX, f32::MIN), |(low, high), point| {
        let t = (*point - centre).dot(along);
        (low.min(t), high.max(t))
    });
    let scale = GRID as f32;
    let base = (centre + along * (low - 0.5)) * scale + Vec2::splat(scale / 2.0);
    PhotoLeaf { base, along, length: (high - low + 1.0) * scale }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn leaves_are_found_with_their_tips_the_right_way() {
        // Two upright "leaves" (tall rectangles) on a clear background.
        let mut image = RgbaImage::new(128, 128);
        for (left, top) in [(20, 20), (80, 30)] {
            for y in top..top + 60 {
                for x in left..left + 14 {
                    image.put_pixel(x, y, Rgba([0, 200, 0, 255]));
                }
            }
        }
        let found = LeafPhotos::find(image, Direction::Up);
        assert_eq!(found.leaves.len(), 2);
        let first = found.leaves[0];
        assert!(first.along.y < -0.99, "points up: {:?}", first.along);
        assert!((first.length - 60.0).abs() <= 8.0, "{}", first.length);
        assert!((first.base.y - 80.0).abs() <= 6.0, "the stalk end is at the bottom: {:?}", first.base);
    }
}
