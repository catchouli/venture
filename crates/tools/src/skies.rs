//! Turning an HDR sky panorama (an equirectangular EXR, as ambientCG
//! publishes them) into a game sky: see `venture_content::sky` for what's
//! made.
//!
//! - The panorama is scaled so its bright sky (the 98th percentile, which
//!   leaves out the sun) is 1: the sky's brightness scale.
//! - The sky picture is that, clipped at 1, as six 1024 × 1024 cube faces.
//! - The light is baked from it with the sun clipped out (the game's sun is
//!   a directional light, and a second sun in the sky light would add
//!   another highlight): irradiance by integrating over the panorama, and
//!   radiance for each roughness by GGX importance sampling, reading blurrier
//!   versions of the panorama for less likely directions ("filtered
//!   importance sampling"), which keeps rough mipmaps smooth with few samples.
//! - The sun is the brightest point of a blurred copy of the panorama.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{Rgb32FImage, RgbImage};
use venture_content::sky::{SkyDef, SkyLight, rgb9e5};

/// Width of each sky picture face, in pixels.
const SKY_FACE: u32 = 1024;
/// JPEG quality of the sky picture.
const SKY_QUALITY: u8 = 88;
/// Width of the irradiance cube's faces.
const IRRADIANCE_FACE: u32 = 32;
/// Width of the radiance cube's largest faces.
const RADIANCE_FACE: u32 = 128;
/// GGX samples per radiance texel.
const RADIANCE_SAMPLES: u32 = 128;
/// The brightest the baked light may be, on the sky's brightness scale
/// (clipping the sun out of it).
const LIGHT_CLIP: f32 = 4.0;
/// The share of the panorama at or below the brightness that becomes 1.
const WHITE_PERCENTILE: f32 = 0.98;

type Vec3 = [f32; 3];

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn normalize(a: Vec3) -> Vec3 {
    scale(a, 1.0 / dot(a, a).sqrt())
}
fn luminance(c: Vec3) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// An equirectangular panorama, with blurrier half-size copies of itself.
struct Panorama {
    /// Each level: width, height and pixels (linear RGB), largest first.
    levels: Vec<(u32, u32, Vec<Vec3>)>,
}

impl Panorama {
    fn new(width: u32, height: u32, pixels: Vec<Vec3>) -> Self {
        let mut levels = vec![(width, height, pixels)];
        while levels.last().is_some_and(|(w, h, _)| *w > 4 && *h > 2) {
            let (w, h, pixels) = levels.last().expect("a level");
            let (nw, nh) = (w / 2, h / 2);
            let at = |x: u32, y: u32| pixels[(y.min(h - 1) * w + x.min(w - 1)) as usize];
            let next = (0..nh)
                .flat_map(|y| (0..nw).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let sum = add(
                        add(at(2 * x, 2 * y), at(2 * x + 1, 2 * y)),
                        add(at(2 * x, 2 * y + 1), at(2 * x + 1, 2 * y + 1)),
                    );
                    scale(sum, 0.25)
                })
                .collect();
            levels.push((nw, nh, next));
        }
        Self { levels }
    }

    /// The colour towards a world direction (x east, y up, z south), from a
    /// level (fractional levels blend two), filtered.
    fn sample(&self, direction: Vec3, level: f32) -> Vec3 {
        let level = level.clamp(0.0, (self.levels.len() - 1) as f32);
        let low = level.floor() as usize;
        let high = (low + 1).min(self.levels.len() - 1);
        let t = level - low as f32;
        add(scale(self.sample_level(direction, low), 1.0 - t), scale(self.sample_level(direction, high), t))
    }

    fn sample_level(&self, direction: Vec3, level: usize) -> Vec3 {
        let (w, h, pixels) = &self.levels[level];
        let (u, v) = to_panorama(direction);
        let (x, y) = (u * *w as f32 - 0.5, (v * *h as f32 - 0.5).clamp(0.0, *h as f32 - 1.0));
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let at = |xi: i64, yi: i64| {
            let xi = xi.rem_euclid(*w as i64) as u32;
            let yi = yi.clamp(0, *h as i64 - 1) as u32;
            pixels[(yi * w + xi) as usize]
        };
        let (xi, yi) = (x0 as i64, y0 as i64);
        let top = add(scale(at(xi, yi), 1.0 - tx), scale(at(xi + 1, yi), tx));
        let bottom = add(scale(at(xi, yi + 1), 1.0 - tx), scale(at(xi + 1, yi + 1), tx));
        add(scale(top, 1.0 - ty), scale(bottom, ty))
    }

    /// The solid angle one texel of the largest level covers (on average).
    fn texel_solid_angle(&self) -> f32 {
        let (w, h, _) = &self.levels[0];
        4.0 * PI / (*w as f32 * *h as f32)
    }
}

/// Where a world direction falls in a panorama: `u` round the horizon (so
/// the sky isn't mirrored: turning right from north runs rightwards
/// through the picture), `v` from straight up (0) to straight down (1).
fn to_panorama(direction: Vec3) -> (f32, f32) {
    let [x, y, z] = direction;
    let u = 0.5 + x.atan2(-z) / (2.0 * PI);
    let v = 0.5 - y.clamp(-1.0, 1.0).asin() / PI;
    (u.rem_euclid(1.0), v)
}

/// The world direction at a panorama point (the inverse of [`to_panorama`]).
fn from_panorama(u: f32, v: f32) -> Vec3 {
    let longitude = (u - 0.5) * 2.0 * PI;
    let latitude = (0.5 - v) * PI;
    [latitude.cos() * longitude.sin(), latitude.sin(), -latitude.cos() * longitude.cos()]
}

/// The world direction through a cube face texel, for Bevy, which reads
/// cubemaps in the usual (left-handed) layout with the world's z flipped.
fn cube_direction(face: usize, x: u32, y: u32, size: u32) -> Vec3 {
    let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    let [cx, cy, cz] = match face {
        0 => [1.0, -v, -u],
        1 => [-1.0, -v, u],
        2 => [u, 1.0, v],
        3 => [u, -1.0, -v],
        4 => [u, -v, 1.0],
        _ => [-u, -v, -1.0],
    };
    normalize([cx, cy, -cz])
}

/// Everything baked from a panorama.
pub struct BakedSky {
    /// The sky picture: six faces stacked top to bottom.
    pub picture: RgbImage,
    /// The light it casts.
    pub light: SkyLight,
    /// Its sun and horizon.
    pub def: SkyDef,
}

/// Bakes a sky from a panorama's pixels (linear RGB).
pub fn bake(panorama: &Rgb32FImage) -> BakedSky {
    let (width, height) = panorama.dimensions();
    let pixels: Vec<Vec3> = panorama.pixels().map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect();

    // The brightness scale: the bright sky (not the sun) is 1.
    let mut brightness: Vec<f32> = pixels.iter().map(|&c| luminance(c)).collect();
    brightness.sort_by(f32::total_cmp);
    let white = brightness[((brightness.len() - 1) as f32 * WHITE_PERCENTILE) as usize].max(1e-6);
    let pixels: Vec<Vec3> = pixels.iter().map(|&c| scale(c, 1.0 / white)).collect();

    let full = Panorama::new(width, height, pixels.clone());
    let sun = find_sun(&full);
    let horizon = horizon_colour(&full);
    let picture = sky_picture(&full);

    // The light, from a smaller copy with the sun clipped out.
    let small = full.levels.iter().find(|(w, _, _)| *w <= 512).unwrap_or(&full.levels[0]);
    let clipped: Vec<Vec3> = small.2.iter().map(|&c| clip(c)).collect();
    let lit = Panorama::new(small.0, small.1, clipped);
    let irradiance = bake_irradiance(&lit);
    let (radiance, radiance_mips) = bake_radiance(&lit);
    BakedSky {
        picture,
        light: SkyLight {
            irradiance_size: IRRADIANCE_FACE,
            irradiance,
            radiance_size: RADIANCE_FACE,
            radiance_mips,
            radiance,
        },
        def: SkyDef { sun: (sun[0], sun[1], sun[2]), horizon: (horizon[0], horizon[1], horizon[2]) },
    }
}

/// A colour no brighter than [`LIGHT_CLIP`] (keeping its hue).
fn clip(colour: Vec3) -> Vec3 {
    let largest = colour[0].max(colour[1]).max(colour[2]);
    if largest > LIGHT_CLIP { scale(colour, LIGHT_CLIP / largest) } else { colour }
}

/// The direction of the brightest point of a blurred copy (so one bright
/// pixel can't fool it).
fn find_sun(panorama: &Panorama) -> Vec3 {
    let (w, h, pixels) =
        panorama.levels.iter().find(|(w, _, _)| *w <= 256).unwrap_or(panorama.levels.last().expect("a level"));
    let (index, _) = pixels
        .iter()
        .enumerate()
        .take((w * h / 2) as usize) // (only above the horizon)
        .max_by(|a, b| luminance(*a.1).total_cmp(&luminance(*b.1)))
        .expect("pixels");
    let (x, y) = (index as u32 % w, index as u32 / w);
    from_panorama((x as f32 + 0.5) / *w as f32, (y as f32 + 0.5) / *h as f32)
}

/// The average colour of the band just above the horizon (the sky distant
/// haze should fade into).
fn horizon_colour(panorama: &Panorama) -> Vec3 {
    let steps = 256;
    let mut sum = [0.0; 3];
    for i in 0..steps {
        for elevation in [0.02f32, 0.05, 0.08] {
            let direction = from_panorama(i as f32 / steps as f32, 0.5 - elevation / PI);
            sum = add(sum, clip(panorama.sample(direction, 2.0)));
        }
    }
    scale(sum, 1.0 / (steps * 3) as f32)
}

/// The sky picture: each face, clipped to 1 and made sRGB, stacked.
fn sky_picture(panorama: &Panorama) -> RgbImage {
    let mut picture = RgbImage::new(SKY_FACE, SKY_FACE * 6);
    for face in 0..6 {
        for y in 0..SKY_FACE {
            for x in 0..SKY_FACE {
                let colour = panorama.sample(cube_direction(face, x, y, SKY_FACE), 0.0);
                let pixel = colour.map(|channel| (to_srgb(channel.clamp(0.0, 1.0)) * 255.0).round() as u8);
                picture.put_pixel(x, face as u32 * SKY_FACE + y, image::Rgb(pixel));
            }
        }
    }
    picture
}

fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// The irradiance cube: for each direction, the light arriving on a surface
/// facing it (cosine-weighted, over π, so a white matte surface shows it
/// as it is).
fn bake_irradiance(panorama: &Panorama) -> Vec<u32> {
    let (w, h, pixels) =
        panorama.levels.iter().find(|(w, _, _)| *w <= 64).unwrap_or(panorama.levels.last().expect("a level"));
    // Each source texel's direction, colour and solid angle.
    let sources: Vec<(Vec3, Vec3, f32)> = (0..*h)
        .flat_map(|y| (0..*w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (u, v) = ((x as f32 + 0.5) / *w as f32, (y as f32 + 0.5) / *h as f32);
            let latitude = (0.5 - v) * PI;
            let solid_angle = (2.0 * PI / *w as f32) * (PI / *h as f32) * latitude.cos();
            (from_panorama(u, v), pixels[(y * w + x) as usize], solid_angle)
        })
        .collect();
    let mut texels = Vec::new();
    for face in 0..6 {
        for y in 0..IRRADIANCE_FACE {
            for x in 0..IRRADIANCE_FACE {
                let normal = cube_direction(face, x, y, IRRADIANCE_FACE);
                let mut sum = [0.0; 3];
                for (direction, colour, solid_angle) in &sources {
                    let cosine = dot(normal, *direction);
                    if cosine > 0.0 {
                        sum = add(sum, scale(*colour, cosine * solid_angle));
                    }
                }
                texels.push(rgb9e5(scale(sum, 1.0 / PI)));
            }
        }
    }
    texels
}

/// The radiance cube: for each mipmap level (perceptual roughness from 0 at
/// the top to 1 at the smallest, as Bevy reads it), the light reflected
/// towards the viewer by a surface that rough (GGX, with the view along the
/// normal). Face by face, each with all its levels.
fn bake_radiance(panorama: &Panorama) -> (Vec<u32>, u32) {
    let mips = RADIANCE_FACE.ilog2() + 1;
    let texel_angle = panorama.texel_solid_angle();
    let mut texels = Vec::new();
    for face in 0..6 {
        for mip in 0..mips {
            let size = RADIANCE_FACE >> mip;
            let roughness = mip as f32 / (mips - 1) as f32;
            let alpha = (roughness * roughness).max(1e-4);
            for y in 0..size {
                for x in 0..size {
                    let normal = cube_direction(face, x, y, size);
                    let colour = if mip == 0 {
                        panorama.sample(normal, 0.0)
                    } else {
                        prefilter(panorama, normal, alpha, texel_angle)
                    };
                    texels.push(rgb9e5(colour));
                }
            }
        }
    }
    (texels, mips)
}

/// GGX-filtered radiance around a direction (the view along the normal).
fn prefilter(panorama: &Panorama, normal: Vec3, alpha: f32, texel_angle: f32) -> Vec3 {
    // A frame around the normal.
    let up = if normal[1].abs() < 0.999 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let tangent = normalize(cross(up, normal));
    let bitangent = cross(normal, tangent);
    let mut sum = [0.0; 3];
    let mut weight = 0.0;
    for i in 0..RADIANCE_SAMPLES {
        // A Hammersley point, turned into a GGX-distributed half vector.
        let (e1, e2) = (i as f32 / RADIANCE_SAMPLES as f32, radical_inverse(i));
        let phi = 2.0 * PI * e1;
        let cos_theta = ((1.0 - e2) / (1.0 + (alpha * alpha - 1.0) * e2)).sqrt();
        let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
        let half = normalize(add(
            add(scale(tangent, sin_theta * phi.cos()), scale(bitangent, sin_theta * phi.sin())),
            scale(normal, cos_theta),
        ));
        // Reflect the view (the normal) about it.
        let light = add(scale(half, 2.0 * dot(normal, half)), scale(normal, -1.0));
        let n_dot_l = dot(normal, light);
        if n_dot_l <= 0.0 {
            continue;
        }
        // Read a blurrier copy for less likely directions (each sample
        // stands for more of the sky).
        let n_dot_h = cos_theta;
        let d = alpha * alpha / (PI * ((n_dot_h * n_dot_h) * (alpha * alpha - 1.0) + 1.0).powi(2));
        let pdf = d / 4.0; // (with the view along the normal, N·H / (4 V·H) is 1/4)
        let sample_angle = 1.0 / (RADIANCE_SAMPLES as f32 * pdf + 1e-4);
        let level = (0.5 * (sample_angle / texel_angle).log2() + 1.0).max(0.0);
        sum = add(sum, scale(panorama.sample(light, level), n_dot_l));
        weight += n_dot_l;
    }
    scale(sum, 1.0 / weight.max(1e-6))
}

/// The van der Corput sequence (bits reversed), for Hammersley points.
fn radical_inverse(mut bits: u32) -> f32 {
    bits = bits.rotate_right(16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    bits as f32 * 2.328_306_4e-10
}

/// Imports a sky from an HDR panorama (the `*_HDR.exr` in an ambientCG sky
/// folder, or the file itself) into `out`. Returns the files written.
pub fn import(source: &Path, out: &Path) -> Result<Vec<PathBuf>, String> {
    let file = if source.is_dir() {
        std::fs::read_dir(source)
            .map_err(|error| format!("can't read {}: {error}", source.display()))?
            .filter_map(|entry| Some(entry.ok()?.path()))
            .find(|path| path.extension().is_some_and(|extension| extension == "exr"))
            .ok_or("there's no .exr panorama there")?
    } else {
        source.to_path_buf()
    };
    let panorama = image::open(&file).map_err(|error| format!("can't open {}: {error}", file.display()))?.to_rgb32f();
    let baked = bake(&panorama);
    std::fs::create_dir_all(out).map_err(|error| format!("can't make {}: {error}", out.display()))?;
    let write_error = |path: &Path, error: &dyn std::fmt::Display| format!("can't write {}: {error}", path.display());
    let picture_path = out.join("sky.jpg");
    let picture_file = std::fs::File::create(&picture_path).map_err(|error| write_error(&picture_path, &error))?;
    JpegEncoder::new_with_quality(std::io::BufWriter::new(picture_file), SKY_QUALITY)
        .encode_image(&baked.picture)
        .map_err(|error| write_error(&picture_path, &error))?;
    let light_path = out.join("light.bin");
    std::fs::write(&light_path, baked.light.to_bytes()).map_err(|error| write_error(&light_path, &error))?;
    let def_path = out.join("sky.ron");
    let text = ron::ser::to_string_pretty(&baked.def, ron::ser::PrettyConfig::default())
        .map_err(|error| write_error(&def_path, &error))?;
    std::fs::write(&def_path, text + "\n").map_err(|error| write_error(&def_path, &error))?;
    Ok(vec![picture_path, light_path, def_path])
}

#[cfg(test)]
mod tests {
    use venture_content::sky::from_rgb9e5;

    use super::*;

    #[test]
    fn panorama_directions_go_there_and_back() {
        for direction in [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], normalize([0.3, 0.5, 0.2]), normalize([-0.7, -0.2, 0.4])] {
            let (u, v) = to_panorama(direction);
            let back = from_panorama(u, v);
            assert!(dot(direction, back) > 0.9999, "{direction:?} came back as {back:?}");
        }
    }

    #[test]
    fn cube_faces_look_where_bevy_expects() {
        // The middle of each face, as Bevy reads them (the world's z flipped).
        let middles: Vec<Vec3> = (0..6).map(|face| cube_direction(face, 8, 8, 16)).collect();
        let expected =
            [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]];
        for (middle, expected) in middles.iter().zip(expected) {
            assert!(dot(*middle, expected) > 0.99, "{middle:?} should face {expected:?}");
        }
    }

    #[test]
    fn a_plain_sky_bakes_to_plain_light_and_finds_its_sun() {
        // Blue sky above, darker ground below, and a small, very bright sun
        // high in the east.
        let (width, height) = (128, 64);
        let panorama = Rgb32FImage::from_fn(width, height, |x, y| {
            let direction = from_panorama((x as f32 + 0.5) / width as f32, (y as f32 + 0.5) / height as f32);
            let sun = normalize([1.0, 1.0, 0.0]);
            if dot(direction, sun) > 0.995 {
                image::Rgb([500.0, 480.0, 450.0])
            } else if direction[1] > 0.0 {
                image::Rgb([0.4, 0.6, 1.0])
            } else {
                image::Rgb([0.2, 0.2, 0.2])
            }
        });
        let baked = bake(&panorama);
        let (sx, sy, sz) = baked.def.sun;
        assert!(dot([sx, sy, sz], normalize([1.0, 1.0, 0.0])) > 0.99, "sun at {:?}", baked.def.sun);
        // Upward-facing surfaces see mostly sky: bluish, and not blown out by the sun.
        let up = from_rgb9e5(
            baked.light.irradiance[(2 * IRRADIANCE_FACE * IRRADIANCE_FACE + IRRADIANCE_FACE * 16 + 16) as usize],
        );
        assert!(up[2] > up[0] && up[2] < 2.0, "irradiance facing up: {up:?}");
        assert_eq!(baked.picture.dimensions(), (SKY_FACE, SKY_FACE * 6));
        assert_eq!(baked.light.radiance_mips, RADIANCE_FACE.ilog2() + 1);
    }
}
