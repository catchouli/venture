//! Mipmaps: the smaller copies of a picture the GPU uses for things far away.
//! Bevy doesn't make them for PNG or JPEG pictures, and a tiling texture
//! without them shimmers in the distance, so the client makes them itself
//! when a texture loads (a few milliseconds for a 1024 × 1024 picture).

use bevy::image::Image;
use bevy::render::render_resource::TextureFormat;

/// Adds a full chain of mipmaps to an 8-bit RGBA picture (each level half the
/// size of the last, down to 1 × 1), averaging sRGB pictures in linear light.
/// Returns whether it could (other formats, or pictures that already have
/// them, are left alone).
pub fn add_mipmaps(image: &mut Image) -> bool {
    let format = image.texture_descriptor.format;
    let srgb = match format {
        TextureFormat::Rgba8UnormSrgb => true,
        TextureFormat::Rgba8Unorm => false,
        _ => return false,
    };
    if image.texture_descriptor.mip_level_count > 1 || image.texture_descriptor.size.depth_or_array_layers != 1 {
        return false;
    }
    let Some(data) = image.data.take() else { return false };
    let (width, height) = (image.texture_descriptor.size.width, image.texture_descriptor.size.height);
    let (chain, levels) = mip_chain(data, width, height, srgb);
    image.data = Some(chain);
    image.texture_descriptor.mip_level_count = levels;
    true
}

/// Adds mipmaps to an alpha-tested picture (leaves), keeping how much of
/// each level passes the test (alpha at least `cutoff`, 0 to 1) the same as
/// the full-size picture. Plain averaging lets coverage fall away level by
/// level, so leaves thin out into nothing in the distance. Each level's
/// alpha is scaled to make up for it (Castaño, "Computing Alpha Mipmaps",
/// 2010). Returns whether it could (see [`add_mipmaps`]).
pub fn add_mipmaps_keeping_coverage(image: &mut Image, cutoff: f32) -> bool {
    let (width, height) = (image.texture_descriptor.size.width, image.texture_descriptor.size.height);
    let threshold = (cutoff.clamp(0.0, 1.0) * 255.0).round() as u8;
    let wanted = image.data.as_ref().map_or(0.0, |data| coverage(data, threshold, 1.0));
    if !add_mipmaps(image) {
        return false;
    }
    let Some(data) = image.data.as_mut() else { return false };
    let (mut offset, mut level_width, mut level_height) = (0, width, height);
    while level_width > 1 || level_height > 1 {
        offset += (level_width * level_height * 4) as usize;
        (level_width, level_height) = ((level_width / 2).max(1), (level_height / 2).max(1));
        let level = &mut data[offset..offset + (level_width * level_height * 4) as usize];
        // The scale that brings coverage closest to the full picture's (by bisection).
        let (mut low, mut high) = (0.0f32, 8.0f32);
        for _ in 0..12 {
            let middle = (low + high) / 2.0;
            if coverage(level, threshold, middle) < wanted { low = middle } else { high = middle }
        }
        for pixel in level.as_chunks_mut::<4>().0 {
            pixel[3] = (f32::from(pixel[3]) * high).round().min(255.0) as u8;
        }
    }
    true
}

/// The share of pixels whose alpha, scaled by `scale`, reaches `threshold`.
fn coverage(pixels: &[u8], threshold: u8, scale: f32) -> f32 {
    let count = pixels.len() / 4;
    let covered =
        pixels.as_chunks::<4>().0.iter().filter(|pixel| f32::from(pixel[3]) * scale >= f32::from(threshold)).count();
    covered as f32 / count.max(1) as f32
}

/// The average colour of a picture with mipmaps (its smallest level), as
/// stored (sRGB for sRGB pictures).
pub fn average_colour(image: &Image) -> Option<[u8; 4]> {
    let data = image.data.as_ref()?;
    (image.texture_descriptor.mip_level_count > 1).then_some(())?;
    data.get(data.len() - 4..).and_then(|last| last.try_into().ok())
}

/// Every level, one after another, and how many there are.
///
/// Kept cheap for the browser, where this runs in WebAssembly: colour
/// channels go through lookup tables (to 16-bit linear light and back) and
/// are averaged as integers, with no floating point per pixel.
fn mip_chain(level: Vec<u8>, mut width: u32, mut height: u32, srgb: bool) -> (Vec<u8>, u32) {
    let to_linear: [u16; 256] =
        std::array::from_fn(|value| (channel_to_linear(value as u8, srgb) * 65535.0).round() as u16);
    // From 12-bit linear (the top bits of the 16) back to how it's stored.
    let from_linear: Vec<u8> =
        (0..ENCODE_STEPS).map(|step| channel_from_linear(step as f32 / (ENCODE_STEPS - 1) as f32, srgb)).collect();
    let mut chain = level.clone();
    let mut current = level;
    let mut levels = 1;
    while width > 1 || height > 1 {
        let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
        let mut next = vec![0u8; (next_width * next_height * 4) as usize];
        let row = (width * 4) as usize;
        for y in 0..next_height as usize {
            let top = &current[(y * 2).min(height as usize - 1) * row..][..row];
            let bottom = &current[(y * 2 + 1).min(height as usize - 1) * row..][..row];
            let out = &mut next[y * next_width as usize * 4..][..next_width as usize * 4];
            for x in 0..next_width as usize {
                let (left, right) = (x * 8, ((x * 2 + 1).min(width as usize - 1)) * 4);
                for channel in 0..4 {
                    let samples =
                        [top[left + channel], top[right + channel], bottom[left + channel], bottom[right + channel]];
                    // (Only sRGB colour needs averaging in linear light.)
                    out[x * 4 + channel] = if srgb && channel < 3 {
                        let sum: u32 = samples.iter().map(|&s| to_linear[s as usize] as u32).sum();
                        from_linear[(((sum / 4) * (ENCODE_STEPS as u32 - 1) + 32767) / 65535) as usize]
                    } else {
                        ((samples.iter().map(|&s| s as u32).sum::<u32>() + 2) / 4) as u8
                    };
                }
            }
        }
        chain.extend_from_slice(&next);
        current = next;
        (width, height) = (next_width, next_height);
        levels += 1;
    }
    (chain, levels)
}

/// How many steps of linear light the table back to stored values has.
const ENCODE_STEPS: usize = 4096;

fn channel_to_linear(value: u8, srgb: bool) -> f32 {
    let c = value as f32 / 255.0;
    if !srgb {
        c
    } else if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn channel_from_linear(c: f32, srgb: bool) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let encoded = if !srgb {
        c
    } else if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    use super::*;

    fn picture(width: u32, height: u32, pixels: impl Fn(u32, u32) -> [u8; 4], format: TextureFormat) -> Image {
        let pixels = &pixels;
        let data = (0..height).flat_map(|y| (0..width).flat_map(move |x| pixels(x, y))).collect();
        let size = Extent3d { width, height, depth_or_array_layers: 1 };
        Image::new(size, TextureDimension::D2, data, format, RenderAssetUsages::default())
    }

    #[test]
    fn every_level_is_made_down_to_one_pixel() {
        let mut image = picture(8, 4, |_, _| [10, 20, 30, 255], TextureFormat::Rgba8Unorm);
        assert!(add_mipmaps(&mut image));
        // 8×4, 4×2, 2×1, 1×1.
        assert_eq!(image.texture_descriptor.mip_level_count, 4);
        assert_eq!(image.data.as_ref().unwrap().len(), (32 + 8 + 2 + 1) * 4);
        assert_eq!(average_colour(&image), Some([10, 20, 30, 255]));
        assert!(!add_mipmaps(&mut image), "only once");
    }

    #[test]
    fn leaves_keep_their_coverage_down_the_mipmaps() {
        // A quarter of the picture solid, in scattered specks that plain averaging would fade.
        let specks =
            |x: u32, y: u32| if x.is_multiple_of(2) && y.is_multiple_of(2) { [0, 200, 0, 255] } else { [0, 200, 0, 0] };
        let mut image = picture(16, 16, specks, TextureFormat::Rgba8UnormSrgb);
        assert!(add_mipmaps_keeping_coverage(&mut image, 0.5));
        let data = image.data.as_ref().unwrap();
        // The second level (8 × 8): averaged, every pixel is a quarter covered (alpha 64);
        // scaled, about a quarter of it passes.
        let second = &data[16 * 16 * 4..16 * 16 * 4 + 8 * 8 * 4];
        let share = coverage(second, 128, 1.0);
        assert!(share > 0.2, "coverage kept: {share}");
    }

    #[test]
    fn srgb_pictures_average_in_linear_light() {
        // Half black, half white: the average is half the light, which in
        // sRGB is 187.5 (so 187 or 188), not 128.
        let checks = |x: u32, _| if x.is_multiple_of(2) { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
        let mut image = picture(2, 2, checks, TextureFormat::Rgba8UnormSrgb);
        add_mipmaps(&mut image);
        assert!((187..=188).contains(&average_colour(&image).unwrap()[0]));
        let mut linear = picture(2, 2, checks, TextureFormat::Rgba8Unorm);
        add_mipmaps(&mut linear);
        assert_eq!(average_colour(&linear).unwrap()[0], 128);
    }
}
