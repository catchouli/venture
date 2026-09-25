//! Building small images in code (for placeholder art such as the move
//! marker and cursors), so the game needs no image files yet.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// A `size` × `size` sRGB image whose pixels come from `paint(u, v)`, which
/// returns linear-ish RGBA in `0.0..=1.0` for `u` and `v` in `-1.0..=1.0`
/// (the centre of the image is `(0, 0)`; `v` grows downwards).
pub fn paint_image(size: u32, usage: RenderAssetUsages, paint: impl Fn(f32, f32) -> [f32; 4]) -> Image {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            pixels.extend(paint(u, v).map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        usage,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// Distance from point `p` to the line segment from `a` to `b`.
pub fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_squared()).clamp(0.0, 1.0);
    p.distance(a + along * t)
}

/// Whether point `p` is inside the polygon `corners` (even-odd rule).
pub fn inside_polygon(p: Vec2, corners: &[Vec2]) -> bool {
    let mut inside = false;
    for i in 0..corners.len() {
        let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Distance from point `p` to the edge of the polygon `corners`.
pub fn distance_to_polygon_edge(p: Vec2, corners: &[Vec2]) -> f32 {
    (0..corners.len())
        .map(|i| distance_to_segment(p, corners[i], corners[(i + 1) % corners.len()]))
        .fold(f32::INFINITY, f32::min)
}
