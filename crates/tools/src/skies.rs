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
//! - The sun is the brightest point of a blurred copy of the panorama. If
//!   it's really there (far brighter than the sky, not just a bright
//!   cloud or star), it's painted out, from the picture and the light: the
//!   game draws its own sun where the time of day puts it (see
//!   `docs/plan-day-night.md`). The patch is filled from the sky just
//!   outside it, along each ray from the sun.
//! - Anything else to paint out (a meteor trail frozen in a night sky) is
//!   listed in `tools/skies/<panorama folder>.ron` ([`SkyImport`]), and
//!   filled from the sky around it.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{Rgb32FImage, RgbImage};
use serde::Deserialize;
use venture_content::sky::{SkyDef, SkyLight, rgb9e5};

/// Width of each sky picture face, in pixels.
const SKY_FACE: u32 = 1024;
/// JPEG quality of the sky picture (high enough to keep faint stars).
const SKY_QUALITY: u8 = 92;
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
/// How bright (on the sky's scale) the sun must be to be painted out: a
/// real sun is hundreds of times the bright sky.
const SUN_PEAK: f32 = 20.0;
/// The widest the sun's patch may be, in degrees (radius).
const MOST_SUN_RADIUS: f32 = 16.0;

/// What to do when importing one panorama, besides the usual: from
/// `tools/skies/<panorama folder>.ron`.
///
/// ```ron
/// (paint_out: [(points: [(3752, 412), (3610, 560)], width: 12)])
/// ```
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkyImport {
    /// Things to paint out (such as meteor trails): lines through pixels of
    /// the panorama (as it is, before anything's done to it), as wide as
    /// `width` pixels.
    pub paint_out: Vec<Stroke>,
}

/// A line to paint out of a panorama.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    /// Pixels it runs through, in order.
    pub points: Vec<(f32, f32)>,
    /// How wide it is, in pixels.
    pub width: f32,
}

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
    /// How wide (radius, in degrees) the sun painted out was, if there was one.
    pub sun_removed: Option<f32>,
    /// The sky picture: six faces stacked top to bottom.
    pub picture: RgbImage,
    /// The light it casts.
    pub light: SkyLight,
    /// Its sun and horizon.
    pub def: SkyDef,
}

/// Bakes a sky from a panorama's pixels (linear RGB).
pub fn bake(panorama: &Rgb32FImage, options: &SkyImport) -> BakedSky {
    let (width, height) = panorama.dimensions();
    let mut pixels: Vec<Vec3> = panorama.pixels().map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect();
    paint_out(&mut pixels, width, height, &options.paint_out);

    // The brightness scale: the bright sky (not the sun) is 1.
    let mut brightness: Vec<f32> = pixels.iter().map(|&c| luminance(c)).collect();
    brightness.sort_by(f32::total_cmp);
    let white = brightness[((brightness.len() - 1) as f32 * WHITE_PERCENTILE) as usize].max(1e-6);
    let pixels: Vec<Vec3> = pixels.iter().map(|&c| scale(c, 1.0 / white)).collect();

    let full = Panorama::new(width, height, pixels.clone());
    let sun = find_sun(&full);
    let (full, sun_removed) = match remove_sun(&full, sun) {
        Some((pixels, radius)) => (Panorama::new(width, height, pixels), Some(radius)),
        None => (full, None),
    };
    let horizon = horizon_colour(&full);
    let picture = sky_picture(&full);

    // The light, from a smaller copy with the sun clipped out.
    let small = full.levels.iter().find(|(w, _, _)| *w <= 512).unwrap_or(&full.levels[0]);
    let clipped: Vec<Vec3> = small.2.iter().map(|&c| clip(c)).collect();
    let lit = Panorama::new(small.0, small.1, clipped);
    let irradiance = bake_irradiance(&lit);
    let (radiance, radiance_mips) = bake_radiance(&lit);
    BakedSky {
        sun_removed,
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

/// Paints things out of a panorama's pixels, within each stroke's corridor:
///
/// 1. **Thin bright lines** (trails): pixels much brighter than those around
///    them, joined into long runs (stars are only a few pixels across), are
///    found and filled like a clone stamp, from clean sky nearby.
/// 2. **Glows** (round a meteor's head, say): the corridor's blurry part is
///    swapped for the sky round about's, keeping its fine detail (the
///    stars), so there's no starless patch.
fn paint_out(pixels: &mut [Vec3], width: u32, height: u32, strokes: &[Stroke]) {
    if strokes.is_empty() {
        return;
    }
    let (w, h) = (width as usize, height as usize);
    // How much each pixel is in a corridor (1 inside, fading at its edges).
    let mut inside = vec![0.0f32; pixels.len()];
    for stroke in strokes {
        let half = stroke.width / 2.0;
        let feather = (half * 0.3).clamp(2.0, 10.0);
        for pair in stroke.points.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            let (x0, x1) = ((ax.min(bx) - half).floor() as i64, (ax.max(bx) + half).ceil() as i64);
            let (y0, y1) = ((ay.min(by) - half).floor() as i64, (ay.max(by) + half).ceil() as i64);
            for y in y0.max(0)..=y1.min(h as i64 - 1) {
                for x in x0..=x1 {
                    let distance = distance_to_segment((x as f32 + 0.5, y as f32 + 0.5), pair[0], pair[1]);
                    let weight = smoothstep(half, half - feather, distance);
                    let index = y as usize * w + x.rem_euclid(w as i64) as usize;
                    inside[index] = inside[index].max(weight);
                }
            }
        }
    }

    // 1. Lines: pixels well above their neighbourhood's middle, in runs.
    let brightness: Vec<f32> = pixels.iter().map(|&c| luminance(c)).collect();
    let typical = {
        let mut sample: Vec<f32> = brightness.iter().step_by(97).copied().collect();
        sample.sort_by(f32::total_cmp);
        sample[sample.len() / 2]
    };
    let at = |x: i64, y: i64| y.clamp(0, h as i64 - 1) as usize * w + x.rem_euclid(w as i64) as usize;
    let local_middle = |x: i64, y: i64, radius: i64| {
        let mut around: Vec<f32> = (-radius..=radius)
            .flat_map(|dy| (-radius..=radius).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| brightness[at(x + dx, y + dy)])
            .collect();
        let middle = around.len() / 2;
        *around.select_nth_unstable_by(middle, f32::total_cmp).1
    };
    let mut bright = vec![false; pixels.len()];
    for index in (0..pixels.len()).filter(|&index| inside[index] > 0.0) {
        let (x, y) = ((index % w) as i64, (index / w) as i64);
        bright[index] = brightness[index] > local_middle(x, y, 9) * 1.3 + typical * 0.05;
    }
    // (Joined across one-pixel gaps, so a faint trail's bright pixels make
    // one long run rather than many short ones.)
    let joined: Vec<bool> = (0..pixels.len())
        .map(|index| {
            let (x, y) = ((index % w) as i64, (index / w) as i64);
            inside[index] > 0.0
                && (-1..=1).any(|dy| (-1..=1).any(|dx| (0..h as i64).contains(&(y + dy)) && bright[at(x + dx, y + dy)]))
        })
        .collect();
    let mut line = vec![false; pixels.len()];
    let mut seen = vec![false; pixels.len()];
    for start in 0..pixels.len() {
        if !joined[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut run = vec![start];
        let mut next = 0;
        while next < run.len() {
            let (x, y) = ((run[next] % w) as i64, (run[next] / w) as i64);
            next += 1;
            for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                if !(0..h as i64).contains(&(y + dy)) {
                    continue;
                }
                let neighbour = at(x + dx, y + dy);
                if joined[neighbour] && !seen[neighbour] {
                    seen[neighbour] = true;
                    run.push(neighbour);
                }
            }
        }
        let xs = run.iter().map(|&i| i % w);
        let ys = run.iter().map(|&i| i / w);
        let across = xs.clone().max().unwrap_or(0) - xs.min().unwrap_or(0);
        let down = ys.clone().max().unwrap_or(0) - ys.min().unwrap_or(0);
        if across.max(down) >= 24 {
            // (With a pixel's margin, for its soft edges.)
            for &index in &run {
                let (x, y) = ((index % w) as i64, (index / w) as i64);
                for dy in -3..=3 {
                    for dx in -3..=3 {
                        if (0..h as i64).contains(&(y + dy)) {
                            line[at(x + dx, y + dy)] = true;
                        }
                    }
                }
            }
        }
    }
    // Filled like a clone stamp: each pixel from a clean spot a little way
    // off, stars and all (the glow step below then matches the brightness
    // of the sky round about).
    let original = pixels.to_vec();
    let offsets = [(0, 40), (40, 0), (0, -40), (-40, 0), (0, 70), (70, 0), (0, -70), (-70, 0)];
    for index in (0..pixels.len()).filter(|&index| line[index]) {
        let (x, y) = ((index % w) as i64, (index / w) as i64);
        let clean = offsets
            .iter()
            .map(|&(dx, dy)| (x + dx, y + dy))
            .find(|&(cx, cy)| (0..h as i64).contains(&cy) && !line[at(cx, cy)] && inside[at(cx, cy)] == 0.0);
        if let Some((cx, cy)) = clean {
            pixels[index] = original[at(cx, cy)];
            continue;
        }
        // (Nowhere clean nearby: the middle of the pixels round about.)
        let mut around: Vec<Vec3> = (-10..=10)
            .flat_map(|dy| (-10..=10).map(move |dx| (dx, dy)))
            .filter(|&(_, dy)| (0..h as i64).contains(&(y + dy)))
            .map(|(dx, dy)| at(x + dx, y + dy))
            .filter(|&neighbour| !line[neighbour])
            .map(|neighbour| original[neighbour])
            .collect();
        if !around.is_empty() {
            let middle = around.len() / 2;
            around.select_nth_unstable_by(middle, |a, b| luminance(*a).total_cmp(&luminance(*b)));
            pixels[index] = around[middle];
        }
    }

    // 2. Glows: swap the corridors' blur for the surroundings'.
    let blurred = box_blur(pixels, w, h, 6);
    let outside: Vec<f32> = inside.iter().map(|&weight| if weight > 0.0 { 0.0 } else { 1.0 }).collect();
    let masked: Vec<Vec3> = blurred.iter().zip(&outside).map(|(&c, &keep)| scale(c, keep)).collect();
    let spread = box_blur(&masked, w, h, 48);
    let coverage = box_blur(&outside.iter().map(|&keep| [keep; 3]).collect::<Vec<_>>(), w, h, 48);
    for index in (0..pixels.len()).filter(|&index| inside[index] > 0.0) {
        let share = coverage[index][0];
        if share < 0.05 {
            continue;
        }
        let surroundings = scale(spread[index], 1.0 / share);
        let change = scale(add(surroundings, scale(blurred[index], -1.0)), inside[index]);
        // (Never darker than most of the sky round about: a glow's peak is
        // sharper than the blur, and taking it all away would leave a hole.)
        let floor = scale(surroundings, 0.7);
        pixels[index] = add(pixels[index], change)
            .iter()
            .zip(floor)
            .map(|(&channel, least)| {
                channel.max(least.min(pixels[index][0].max(pixels[index][1]).max(pixels[index][2])))
            })
            .collect::<Vec<f32>>()
            .try_into()
            .expect("three channels");
    }
}

/// A box blur of a picture (wrapping round east to west), `radius` pixels.
fn box_blur(pixels: &[Vec3], w: usize, h: usize, radius: usize) -> Vec<Vec3> {
    let across = |source: &[Vec3], length: usize, count: usize, index: &dyn Fn(usize, usize) -> usize, wrap: bool| {
        let mut out = vec![[0.0; 3]; source.len()];
        let span = (2 * radius + 1) as f32;
        for line in 0..count {
            let get = |i: i64| {
                let i = if wrap { i.rem_euclid(length as i64) } else { i.clamp(0, length as i64 - 1) } as usize;
                source[index(line, i)]
            };
            let mut sum = [0.0; 3];
            for i in -(radius as i64)..=radius as i64 {
                sum = add(sum, get(i));
            }
            for i in 0..length {
                out[index(line, i)] = scale(sum, 1.0 / span);
                sum = add(sum, add(get(i as i64 + radius as i64 + 1), scale(get(i as i64 - radius as i64), -1.0)));
            }
        }
        out
    };
    let rows = across(pixels, w, h, &|row, x| row * w + x, true);
    across(&rows, h, w, &|column, y| y * w + column, false)
}

/// How far a point is from a line segment.
fn distance_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let length = abx * abx + aby * aby;
    let t = if length > 0.0 { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / length).clamp(0.0, 1.0) } else { 0.0 };
    ((p.0 - a.0 - t * abx).powi(2) + (p.1 - a.1 - t * aby).powi(2)).sqrt()
}

/// The panorama's pixels with the sun painted out, and how wide (radius,
/// in degrees) the patch was, if there's really a sun there: the patch
/// reaches as far as the sun brightens the sky, and is filled smoothly from
/// the colours round its edge, each weighted by how near it is (softly, at
/// the patch's edge).
fn remove_sun(panorama: &Panorama, sun: Vec3) -> Option<(Vec<Vec3>, f32)> {
    let peak = luminance(panorama.sample(sun, 0.0));
    if peak < SUN_PEAK {
        return None;
    }
    // A direction `angle` from the sun, round it by `turn`.
    let side = normalize(cross(sun, if sun[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] }));
    let up = cross(side, sun);
    let at = |angle: f32, turn: f32| {
        let across = add(scale(side, turn.cos()), scale(up, turn.sin()));
        add(scale(sun, angle.cos()), scale(across, angle.sin()))
    };
    let ring = |angle: f32| {
        (0..64).map(|i| luminance(panorama.sample(at(angle, i as f32 * PI / 32.0), 1.0))).sum::<f32>() / 64.0
    };
    let far = ring((MOST_SUN_RADIUS + 8.0).to_radians());
    let reach = (2..=MOST_SUN_RADIUS as u32 * 2)
        .map(|half_degrees| half_degrees as f32 / 2.0)
        .find(|&degrees| ring(degrees.to_radians()) < far * 1.25)
        .unwrap_or(MOST_SUN_RADIUS);
    let radius = (reach + 1.5).clamp(3.0, MOST_SUN_RADIUS).to_radians();
    // The colours round the patch's edge (from a blurred copy, so small
    // details there don't streak), to fill it from.
    let (w, h, pixels) = &panorama.levels[0];
    let texel = 2.0 * PI / *w as f32;
    let blur = (radius / texel / 8.0).max(1.0).log2();
    let edge: Vec<(Vec3, Vec3)> = (0..128)
        .map(|i| {
            let direction = at(radius, i as f32 * PI / 64.0);
            let colour = panorama.sample(direction, blur);
            let brightness = luminance(colour);
            // (No brighter than the sky round about: the least of the sun
            // left in it would still glare.)
            (direction, if brightness > far * 3.0 { scale(colour, far * 3.0 / brightness) } else { colour })
        })
        .collect();
    let mut painted = pixels.clone();
    for y in 0..*h {
        for x in 0..*w {
            let direction = from_panorama((x as f32 + 0.5) / *w as f32, (y as f32 + 0.5) / *h as f32);
            let angle = dot(direction, sun).clamp(-1.0, 1.0).acos();
            if angle >= radius {
                continue;
            }
            // Each edge colour, weighted by how near it is.
            let (mut sum, mut weights) = ([0.0; 3], 0.0);
            for &(point, colour) in &edge {
                let apart = dot(direction, point).clamp(-1.0, 1.0).acos();
                let weight = 1.0 / (apart * apart + 1e-6);
                sum = add(sum, scale(colour, weight));
                weights += weight;
            }
            let fill = scale(sum, 1.0 / weights);
            let keep = smoothstep(radius - 1.5f32.to_radians(), radius, angle);
            let index = (y * w + x) as usize;
            painted[index] = add(scale(fill, 1.0 - keep), scale(pixels[index], keep));
        }
    }
    Some((painted, radius.to_degrees()))
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
/// folder, or the file itself) into `out`, with `options`. Returns what was
/// baked, and the files written.
pub fn import(source: &Path, out: &Path, options: &SkyImport) -> Result<(BakedSky, Vec<PathBuf>), String> {
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
    let baked = bake(&panorama, options);
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
    Ok((baked, vec![picture_path, light_path, def_path]))
}

/// The options for importing a panorama: `tools/skies/<name>.ron` (under
/// `folder`), where the name is the panorama's folder (or file) name, if
/// there's such a file.
pub fn import_options(folder: &Path, source: &Path) -> Result<SkyImport, String> {
    let name = source.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let path = folder.join(format!("{name}.ron"));
    match std::fs::read_to_string(&path) {
        Ok(text) => ron::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
        Err(_) => Ok(SkyImport::default()),
    }
}

#[cfg(test)]
mod tests {
    use venture_content::sky::from_rgb9e5;

    use super::*;

    #[test]
    fn strokes_are_painted_out_from_around_them() {
        // A dark sky with a bright line across it.
        let (width, height) = (64u32, 32u32);
        let mut pixels: Vec<Vec3> = (0..width * height)
            .map(|i| if i / width == 10 && (10..50).contains(&(i % width)) { [5.0; 3] } else { [0.1; 3] })
            .collect();
        let stroke = Stroke { points: vec![(10.0, 10.5), (50.0, 10.5)], width: 12.0 };
        paint_out(&mut pixels, width, height, &[stroke]);
        assert!(pixels.iter().all(|pixel| pixel[0] < 0.2), "the line's gone");
    }

    #[test]
    fn a_dim_sky_keeps_its_brightest_point() {
        // No sun: the brightest point (a star, a bright cloud) stays.
        let panorama = Rgb32FImage::from_fn(128, 64, |x, _| image::Rgb(if x == 30 { [3.0; 3] } else { [0.5; 3] }));
        assert!(bake(&panorama, &SkyImport::default()).sun_removed.is_none());
    }

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
        let baked = bake(&panorama, &SkyImport::default());
        assert!(baked.sun_removed.is_some(), "a sun that bright is painted out");
        let (sx, sy, sz) = baked.def.sun;
        assert!(dot([sx, sy, sz], normalize([1.0, 1.0, 0.0])) > 0.99, "sun at {:?}", baked.def.sun);
        // Upward-facing surfaces see mostly sky: bluish, and not blown out by the sun.
        let up = from_rgb9e5(
            baked.light.irradiance[(2 * IRRADIANCE_FACE * IRRADIANCE_FACE + IRRADIANCE_FACE * 16 + 16) as usize],
        );
        assert!(up[2] > up[0] && up[2] < 2.0, "irradiance facing up: {up:?}");
        assert_eq!(baked.picture.dimensions(), (SKY_FACE, SKY_FACE * 6));
        // No sun left in the picture: nothing white (the sky clips blue, not red).
        let white = baked.picture.pixels().filter(|pixel| pixel.0.iter().all(|&channel| channel > 240)).count();
        assert_eq!(white, 0, "the sun's still there");
        assert_eq!(baked.light.radiance_mips, RADIANCE_FACE.ilog2() + 1);
    }
}
