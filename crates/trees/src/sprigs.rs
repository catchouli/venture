//! Painting sprigs: the pictures on leaf cards, each a small branching twig
//! with its leaves.
//!
//! A sprig is a twig up from the bottom middle of its picture, with side
//! twigs fanning out from it (fewer and shorter towards the top), and leaves
//! along all of them, alternating side to side, with one at each tip. So a
//! card is mostly covered, which is what makes a crown look full. Willow's
//! long hanging cards are a single strand instead.
//!
//! Leaves are life-size: a card is [`Leaves::card_size`] metres across. They
//! are either photographs from a leaf set ([`crate::photos`]), or painted
//! from the species' leaf shape. Pine needles are always painted, in tufts.
//!
//! Pictures are drawn at twice the size and scaled down, so their edges are
//! soft (good for alpha-to-coverage). Then the see-through pixels take the
//! leaves' average colour, so mipmaps don't darken the edges.

use std::f32::consts::PI;

use glam::Vec2;
use image::{Rgba, RgbaImage};

use crate::photos::LeafPhotos;
use crate::random::Random;
use crate::skeleton::SPRIG_PICTURES;
use crate::species::{Colour, LeafShape, Leaves};

/// A sprig picture's size, in pixels (the atlas is two of them across).
pub const SPRIG_SIZE: u32 = 256;
/// How much bigger sprigs are drawn before they're scaled down.
const OVERSAMPLE: u32 = 2;
/// How far apart leaves are along a twig, as a share of a leaf's length.
const LEAF_SPACING: f32 = 0.42;
/// Side twigs a sprig has, from the fewest to the most.
const SIDE_TWIGS: (u32, u32) = (5, 7);

/// Paints a species' sprig atlas: [`SPRIG_PICTURES`] sprigs, two across.
pub fn paint_sprigs(leaves: &Leaves, photos: Option<&LeafPhotos>, seed: u64) -> RgbaImage {
    let columns = SPRIG_PICTURES.div_ceil(2);
    let atlas_size = SPRIG_SIZE * columns;
    let mut atlas = RgbaImage::new(atlas_size, atlas_size);
    let (mut colour_sum, mut coverage_sum) = ([0.0f32; 3], 0.0f32);
    let mut cells = Vec::new();
    for picture in 0..SPRIG_PICTURES {
        let mut canvas = Canvas::new(SPRIG_SIZE * OVERSAMPLE, leaves.stretch.max(0.1));
        paint_sprig(&mut canvas, leaves, photos, Random::new(seed).child(u64::from(picture)));
        let cell = canvas.shrunk(OVERSAMPLE);
        for pixel in &cell.pixels {
            for channel in 0..3 {
                colour_sum[channel] += pixel[channel] * pixel[3];
            }
            coverage_sum += pixel[3];
        }
        cells.push(cell);
    }
    // See-through pixels take the average leaf colour (so mipmaps keep the edges green).
    let average = colour_sum.map(|sum| sum / coverage_sum.max(1e-6));
    for (picture, cell) in cells.iter().enumerate() {
        let (cell_x, cell_y) = ((picture as u32 % columns) * SPRIG_SIZE, (picture as u32 / columns) * SPRIG_SIZE);
        for y in 0..SPRIG_SIZE {
            for x in 0..SPRIG_SIZE {
                let [r, g, b, a] = cell.pixels[(y * SPRIG_SIZE + x) as usize];
                let colour = if a > 0.0 { Colour(r, g, b) } else { Colour(average[0], average[1], average[2]) };
                let [r, g, b] = colour.to_bytes();
                atlas.put_pixel(cell_x + x, cell_y + y, Rgba([r, g, b, (a * 255.0).round() as u8]));
            }
        }
    }
    atlas
}

/// A picture being painted: colours (sRGB) and coverage, from 0 to 1.
///
/// Shapes are given in the card's own measure: 1 across and `stretch` down
/// (a willow's long cards squash their picture, so it's drawn squashed the
/// other way to come out right).
struct Canvas {
    size: u32,
    stretch: f32,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(size: u32, stretch: f32) -> Self {
        Self { size, stretch, pixels: vec![[0.0; 4]; (size * size) as usize] }
    }

    /// Paints every pixel in the box `(low, high)` (in the card's measure)
    /// that `paint` gives a colour to.
    fn fill(&mut self, low: Vec2, high: Vec2, mut paint: impl FnMut(Vec2) -> Option<Colour>) {
        let size = self.size as f32;
        let x0 = (low.x * size).floor().max(0.0) as u32;
        let y0 = (low.y / self.stretch * size).floor().max(0.0) as u32;
        let x1 = ((high.x * size).ceil().max(0.0) as u32).min(self.size);
        let y1 = ((high.y / self.stretch * size).ceil().max(0.0) as u32).min(self.size);
        for y in y0..y1 {
            for x in x0..x1 {
                let point = Vec2::new((x as f32 + 0.5) / size, (y as f32 + 0.5) / size * self.stretch);
                if let Some(colour) = paint(point) {
                    self.pixels[(y * self.size + x) as usize] = [colour.0, colour.1, colour.2, 1.0];
                }
            }
        }
    }

    /// A line from `a` to `b`, `width` wide at `a` narrowing to `end_width` at `b`.
    fn line(&mut self, a: Vec2, b: Vec2, width: f32, end_width: f32, colour: Colour) {
        let pad = Vec2::splat(width.max(end_width));
        let (along, length_squared) = (b - a, (b - a).length_squared().max(1e-9));
        self.fill(a.min(b) - pad, a.max(b) + pad, |point| {
            let t = ((point - a).dot(along) / length_squared).clamp(0.0, 1.0);
            let half = (width + (end_width - width) * t) / 2.0;
            (point.distance(a + along * t) <= half).then_some(colour)
        });
    }

    /// Paints a leaf `length` long from `base` towards `way` (a unit
    /// vector), as `look` says at each point: given how far along it is (0
    /// at the stalk to 1 at the tip) and how far across (a share of its
    /// length, either side).
    fn leaf(&mut self, base: Vec2, way: Vec2, length: f32, mut look: impl FnMut(f32, f32) -> Option<Colour>) {
        let tip = base + way * length;
        let pad = Vec2::splat(length * 0.6);
        let across = way.perp();
        self.fill(base.min(tip) - pad, base.max(tip) + pad, |point| {
            let relative = point - base;
            let s = relative.dot(way) / length;
            if !(0.0..=1.0).contains(&s) {
                return None;
            }
            look(s, relative.dot(across) / length)
        });
    }

    /// Scales it down by `factor`, averaging (colours weighted by coverage).
    fn shrunk(&self, factor: u32) -> Canvas {
        let size = self.size / factor;
        let mut smaller = Canvas::new(size, self.stretch);
        for y in 0..size {
            for x in 0..size {
                let mut sum = [0.0; 4];
                for sy in 0..factor {
                    for sx in 0..factor {
                        let pixel = self.pixels[((y * factor + sy) * self.size + x * factor + sx) as usize];
                        for channel in 0..3 {
                            sum[channel] += pixel[channel] * pixel[3];
                        }
                        sum[3] += pixel[3];
                    }
                }
                let coverage = sum[3];
                let samples = (factor * factor) as f32;
                smaller.pixels[(y * size + x) as usize] = if coverage > 0.0 {
                    [sum[0] / coverage, sum[1] / coverage, sum[2] / coverage, coverage / samples]
                } else {
                    [0.0; 4]
                };
            }
        }
        smaller
    }
}

/// How wide a simple leaf is at `s` (0 at its stalk to 1 at its tip), as a
/// share of its length, for a leaf `width` (a share) at its widest.
fn leaf_outline(shape: LeafShape, width: f32, s: f32) -> f32 {
    let half = width / 2.0;
    match shape {
        LeafShape::Oval => half * (PI * s.powf(0.9)).sin().max(0.0).powf(0.6),
        LeafShape::Lanceolate => half * (PI * s).sin().max(0.0).powf(1.2),
        LeafShape::Triangular => {
            let body = if s < 0.35 { (s / 0.35).powf(0.7) } else { ((1.0 - s) / 0.65).powf(1.1) };
            half * body * (1.0 + 0.1 * (s * 38.0).sin())
        }
        LeafShape::Lobed(lobes) => {
            let body = (PI * s.powf(0.85)).sin().max(0.0).powf(0.5);
            let lobing = 0.5 + 0.5 * (std::f32::consts::TAU * lobes as f32 * s + PI).cos();
            half * body * (0.45 + 0.55 * lobing)
        }
        _ => half * (PI * s).sin().max(0.0),
    }
}

/// A twig in a sprig: its points from its base, and its width at the base.
struct Twig {
    points: Vec<Vec2>,
    width: f32,
}

impl Twig {
    /// The point and way along it `t` of the way (0 to 1).
    fn at(&self, t: f32) -> (Vec2, Vec2) {
        let segments = self.points.len() - 1;
        let at = t.clamp(0.0, 1.0) * segments as f32;
        let i = (at.floor() as usize).min(segments - 1);
        let (a, b) = (self.points[i], self.points[i + 1]);
        (a.lerp(b, at - i as f32), (b - a).normalize_or(Vec2::NEG_Y))
    }

    fn length(&self) -> f32 {
        self.points.windows(2).map(|pair| pair[0].distance(pair[1])).sum()
    }
}

/// A twig from `base` heading `way`, `length` long, bending `bend` (radians,
/// over its length) and wandering a little.
fn twig(base: Vec2, way: Vec2, length: f32, bend: f32, width: f32, random: &mut Random) -> Twig {
    const STEPS: usize = 6;
    let mut points = vec![base];
    let mut heading = way;
    for _ in 0..STEPS {
        heading = Vec2::from_angle(bend / STEPS as f32 + random.signed() * 0.08).rotate(heading);
        points.push(points[points.len() - 1] + heading * length / STEPS as f32);
    }
    Twig { points, width }
}

/// Lays out a sprig's twigs: a main one up the middle, with side twigs
/// fanning out (none on hanging strands).
fn lay_out_twigs(tall: f32, random: &mut Random) -> Vec<Twig> {
    let lean = random.signed() * 0.12;
    let main = twig(
        Vec2::new(0.5, tall * 0.99),
        Vec2::from_angle(lean).rotate(Vec2::NEG_Y),
        tall * 0.88,
        random.signed() * 0.2,
        0.02,
        random,
    );
    let mut twigs = Vec::new();
    if tall <= 1.5 {
        let sides = random.between(SIDE_TWIGS.0, SIDE_TWIGS.1);
        for i in 0..sides {
            let t = 0.15 + 0.62 * i as f32 / (sides - 1).max(1) as f32 + 0.04 * random.signed();
            let (base, way) = main.at(t);
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let angle = side * random.range(0.55, 0.95);
            let length = (0.42 - 0.3 * t) * random.range(0.85, 1.15);
            // Side twigs curve back up towards the light.
            let bend = -side * random.range(0.2, 0.5);
            twigs.push(twig(base, Vec2::from_angle(angle).rotate(way), length, bend, 0.012, random));
        }
    }
    twigs.insert(0, main);
    twigs
}

/// Paints one sprig.
fn paint_sprig(canvas: &mut Canvas, leaves: &Leaves, photos: Option<&LeafPhotos>, mut random: Random) {
    let tall = leaves.stretch.max(0.1);
    let leaf_length = (leaves.size / leaves.card_size.max(0.01)).clamp(0.03, 0.9);
    let twigs = lay_out_twigs(tall, &mut random);
    for twig in &twigs {
        let count = twig.points.len() - 1;
        for (i, pair) in twig.points.windows(2).enumerate() {
            let (t0, t1) = (i as f32 / count as f32, (i + 1) as f32 / count as f32);
            canvas.line(pair[0], pair[1], twig.width * (1.0 - 0.6 * t0), twig.width * (1.0 - 0.6 * t1), leaves.twig);
        }
    }
    let photos = photos.filter(|photos| !photos.leaves.is_empty() && leaves.shape != LeafShape::Needles);
    let mut painter = LeafPainter { leaves, photos, random: random.child(9) };
    for twig in &twigs {
        // Along the twig, alternating sides, and one at the tip.
        let spacing = match leaves.shape {
            LeafShape::Pinnate(_) | LeafShape::Palmate(_) => leaf_length * 1.3,
            LeafShape::Needles => leaf_length * 0.5,
            _ => leaf_length * LEAF_SPACING,
        };
        let length = twig.length();
        let count = ((length * 0.85 / spacing).floor() as u32).max(1);
        for i in 0..count {
            let t = 0.15 + 0.85 * (i as f32 + 0.5) / count as f32;
            let (base, way) = twig.at(t);
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            // Leaves near the tip point more along the twig.
            let angle = side * random.range(0.6, 1.2) * (1.0 - 0.4 * t);
            let size = leaf_length * random.range(0.8, 1.1) * (0.8 + 0.2 * t);
            painter.paint(canvas, base, Vec2::from_angle(angle).rotate(way), size);
        }
        let (tip, way) = twig.at(1.0);
        painter.paint(canvas, tip, way, leaf_length);
    }
}

/// Paints leaves of one species.
struct LeafPainter<'a> {
    leaves: &'a Leaves,
    photos: Option<&'a LeafPhotos>,
    random: Random,
}

impl LeafPainter<'_> {
    /// Paints a leaf (or its species' equivalent: a tuft of needles, a
    /// compound leaf, a hand of lobes) at `base`, towards `way`, `length` long.
    fn paint(&mut self, canvas: &mut Canvas, base: Vec2, way: Vec2, length: f32) {
        let leaves = self.leaves;
        match leaves.shape {
            LeafShape::Needles => {
                for _ in 0..8 {
                    let turn = Vec2::from_angle(self.random.signed() * 0.9).rotate(way);
                    let colour = self.colour();
                    let needle = length * self.random.range(0.7, 1.0);
                    canvas.line(base, base + turn * needle, 0.008, 0.004, colour);
                }
            }
            LeafShape::Pinnate(pairs) => {
                // A stalk with pairs of leaflets and one at its tip.
                let stalk = length * 1.4;
                let tip = base + way * stalk;
                canvas.line(base, tip, 0.007, 0.003, leaves.twig.mix(leaves.colours.1, 0.5));
                let leaflet = length * 0.5;
                for pair in 0..pairs.max(1) {
                    let at = base.lerp(tip, 0.25 + 0.75 * pair as f32 / pairs.max(1) as f32);
                    for flank in [-1.0f32, 1.0] {
                        self.one_leaf(canvas, at, Vec2::from_angle(flank * 1.1).rotate(way), leaflet);
                    }
                }
                self.one_leaf(canvas, tip, way, leaflet);
            }
            LeafShape::Palmate(lobes) if self.photos.is_none() => {
                let colour = self.colour();
                let lobes = lobes.max(1);
                for lobe in 0..lobes {
                    let spread = (lobe as f32 / (lobes - 1).max(1) as f32 - 0.5) * 2.6;
                    let lobe_length = length * (1.0 - 0.25 * spread.abs() / 1.3);
                    let lobe_way = Vec2::from_angle(spread).rotate(way);
                    let width = leaves.width / 2.0;
                    canvas.leaf(base, lobe_way, lobe_length, |s, d| {
                        let half = width * (PI * s.powf(0.7)).sin().max(0.0) * (1.0 - s * 0.3);
                        (d.abs() <= half).then(|| colour.scaled(0.92 + 0.12 * d.abs() / half.max(1e-4)))
                    });
                }
            }
            _ => self.one_leaf(canvas, base, way, length),
        }
    }

    /// Paints a single leaf: a photographed one if the species has them,
    /// otherwise painted from its outline.
    fn one_leaf(&mut self, canvas: &mut Canvas, base: Vec2, way: Vec2, length: f32) {
        if let Some(photos) = self.photos {
            let leaf = photos.leaves[self.random.between(0, photos.leaves.len() as u32 - 1) as usize];
            // Coloured so its average is the species' colour (the photograph gives the detail).
            let wanted = self.colour();
            let average = photos.average;
            let tint =
                Colour(wanted.0 / average.0.max(0.01), wanted.1 / average.1.max(0.01), wanted.2 / average.2.max(0.01));
            // Mirrored half the time, for variety.
            let flip = if self.random.chance(0.5) { -1.0 } else { 1.0 };
            let image = &photos.image;
            canvas.leaf(base, way, length, |s, d| {
                let source = leaf.base + leaf.along * (s * leaf.length) + leaf.along.perp() * (d * flip * leaf.length);
                let (colour, coverage) = sample(image, source);
                (coverage >= 0.5).then_some(Colour(colour.0 * tint.0, colour.1 * tint.1, colour.2 * tint.2))
            });
            return;
        }
        let colour = self.colour();
        let (shape, width) = match self.leaves.shape {
            LeafShape::Pinnate(_) => (LeafShape::Lanceolate, self.leaves.width),
            LeafShape::FlatNeedles => (LeafShape::Lanceolate, self.leaves.width * 0.5),
            shape => (shape, self.leaves.width),
        };
        let vein = colour.scaled(0.8);
        canvas.leaf(base, way, length, |s, d| {
            let half = leaf_outline(shape, width, s);
            if d.abs() > half {
                return None;
            }
            // A darker midrib, and lighter towards the edges.
            if d.abs() < 0.012 && s < 0.9 {
                return Some(vein);
            }
            Some(colour.scaled(0.92 + 0.12 * d.abs() / half.max(1e-4)))
        });
    }

    /// A leaf colour, somewhere between the species' two.
    fn colour(&mut self) -> Colour {
        let leaves = self.leaves;
        leaves.colours.0.mix(leaves.colours.1, self.random.unit()).scaled(self.random.range(0.92, 1.08))
    }
}

/// A photograph's colour (sRGB, 0 to 1) and coverage at a point in pixels (nearest pixel).
fn sample(image: &RgbaImage, at: Vec2) -> (Colour, f32) {
    let (x, y) = (at.x.round() as i64, at.y.round() as i64);
    if x < 0 || y < 0 || x >= i64::from(image.width()) || y >= i64::from(image.height()) {
        return (Colour(0.0, 0.0, 0.0), 0.0);
    }
    let [r, g, b, a] = image.get_pixel(x as u32, y as u32).0;
    let unit = |byte: u8| f32::from(byte) / 255.0;
    (Colour(unit(r), unit(g), unit(b)), unit(a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::species::Leaves;

    #[test]
    fn every_shape_paints_a_well_covered_sprig() {
        let shapes = [
            LeafShape::Lobed(4),
            LeafShape::Pinnate(4),
            LeafShape::Oval,
            LeafShape::Palmate(5),
            LeafShape::Lanceolate,
            LeafShape::Triangular,
            LeafShape::Needles,
            LeafShape::FlatNeedles,
        ];
        for shape in shapes {
            let leaves = Leaves { shape, size: 0.12, card_size: 0.6, ..Leaves::default() };
            let atlas = paint_sprigs(&leaves, None, 1);
            let covered = atlas.pixels().filter(|pixel| pixel[3] > 128).count();
            let share = covered as f32 / (atlas.width() * atlas.height()) as f32;
            assert!((0.12..0.9).contains(&share), "{shape:?} covers {share}");
        }
    }
}
