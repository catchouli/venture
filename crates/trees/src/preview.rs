//! Pictures of trees without a GPU: a small orthographic software
//! rasteriser (edge functions and a depth buffer), for previews and
//! galleries, and later for the pictures on far-away trees' impostor cards.
//!
//! Lighting is simple on purpose: a sun and a sky (ambient) light, with
//! leaves lit by wrapped diffuse lighting since light passes through them.
//! Textures are sampled bilinearly, and alpha-tested like the game does.
//! Everything is drawn at twice the size and scaled down, to smooth edges.

use glam::{Vec2, Vec3};
use image::{Rgba, RgbaImage};

use crate::mesh::MeshPart;

/// How much bigger pictures are drawn before they're scaled down.
const OVERSAMPLE: u32 = 2;
/// Texture coverage below this is see-through.
const ALPHA_CUTOFF: f32 = 0.5;
/// Where the sunlight comes from (towards the sun).
const SUN: Vec3 = Vec3::new(-0.45, 0.75, 0.5);
/// How bright the sun and the sky are.
const SUN_LIGHT: f32 = 0.85;
const SKY_LIGHT: f32 = 0.4;

/// Something to draw: a mesh, with its texture if it has one, moved by `offset`.
pub struct Drawable<'a> {
    /// The mesh.
    pub mesh: &'a MeshPart,
    /// Its texture (sRGB), if any; otherwise the vertex colours alone.
    pub texture: Option<&'a RgbaImage>,
    /// Whether the texture repeats (bark) or not (sprigs).
    pub repeats: bool,
    /// Whether it's leaves (lit as though light passes through).
    pub leaves: bool,
    /// Where it stands, in metres.
    pub offset: Vec3,
}

/// Where a picture is seen from: straight along a direction, at a scale.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The point in the middle of the picture, in metres.
    pub centre: Vec3,
    /// Which way the view looks (from the viewer, a unit vector).
    pub looking: Vec3,
    /// How many metres a pixel covers.
    pub metres_per_pixel: f32,
}

impl View {
    /// The ways right and up in the picture.
    fn axes(&self) -> (Vec3, Vec3) {
        let right = self.looking.cross(Vec3::Y).normalize_or(Vec3::X);
        (right, right.cross(self.looking))
    }
}

/// Draws a picture `width` × `height` pixels: `things` over a sky, seen from `view`.
pub fn render(things: &[Drawable], view: View, width: u32, height: u32) -> RgbaImage {
    let (big_width, big_height) = (width * OVERSAMPLE, height * OVERSAMPLE);
    let mut picture = Picture::new(big_width, big_height);
    let (right, up) = view.axes();
    let scale = 1.0 / (view.metres_per_pixel / OVERSAMPLE as f32);
    let to_screen = |p: Vec3| {
        let relative = p - view.centre;
        Vec3::new(
            big_width as f32 / 2.0 + relative.dot(right) * scale,
            big_height as f32 / 2.0 - relative.dot(up) * scale,
            relative.dot(view.looking),
        )
    };
    for thing in things {
        let mesh = thing.mesh;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let corners = [triangle[0] as usize, triangle[1] as usize, triangle[2] as usize];
            let screen = corners.map(|i| to_screen(Vec3::from(mesh.positions[i]) + thing.offset));
            picture.triangle(thing, screen, corners, view.looking);
        }
    }
    picture.finish(width, height)
}

/// A picture being drawn, at the oversampled size.
struct Picture {
    width: u32,
    height: u32,
    colours: Vec<Vec3>,
    depths: Vec<f32>,
}

impl Picture {
    fn new(width: u32, height: u32) -> Self {
        // The sky: pale at the horizon, bluer above.
        let colours = (0..height)
            .flat_map(|y| {
                let t = y as f32 / height as f32;
                let sky = Vec3::new(0.35, 0.5, 0.75).lerp(Vec3::new(0.75, 0.8, 0.85), t);
                std::iter::repeat_n(sky, width as usize)
            })
            .collect();
        Self { width, height, colours, depths: vec![f32::INFINITY; (width * height) as usize] }
    }

    fn triangle(&mut self, thing: &Drawable, screen: [Vec3; 3], corners: [usize; 3], looking: Vec3) {
        let [a, b, c] = screen;
        let area = edge(a, b, c);
        if area.abs() < 1e-8 {
            return;
        }
        let low = a.min(b).min(c);
        let high = a.max(b).max(c);
        let x0 = low.x.floor().max(0.0) as u32;
        let y0 = low.y.floor().max(0.0) as u32;
        let x1 = (high.x.ceil().max(0.0) as u32).min(self.width);
        let y1 = (high.y.ceil().max(0.0) as u32).min(self.height);
        let mesh = thing.mesh;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, 0.0);
                let (w0, w1, w2) = (edge(b, c, p) / area, edge(c, a, p) / area, edge(a, b, p) / area);
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let depth = a.z * w0 + b.z * w1 + c.z * w2;
                let index = (y * self.width + x) as usize;
                if depth >= self.depths[index] {
                    continue;
                }
                let blend = |values: [[f32; 4]; 3]| {
                    let v = values.map(glam::Vec4::from);
                    v[0] * w0 + v[1] * w1 + v[2] * w2
                };
                let colour = blend(corners.map(|i| mesh.colours[i]));
                let mut albedo = colour.truncate();
                if let Some(texture) = thing.texture {
                    let uvs = corners.map(|i| Vec2::from(mesh.uvs[i]));
                    let uv = uvs[0] * w0 + uvs[1] * w1 + uvs[2] * w2;
                    let (texel, alpha) = sample(texture, uv, thing.repeats);
                    if alpha < ALPHA_CUTOFF {
                        continue;
                    }
                    albedo *= texel;
                }
                let normals = corners.map(|i| Vec3::from(mesh.normals[i]));
                let mut normal = (normals[0] * w0 + normals[1] * w1 + normals[2] * w2).normalize_or(Vec3::Y);
                let sun = SUN.normalize();
                let lit = if thing.leaves {
                    // Light through the leaves: wrapped, and never quite dark.
                    (normal.dot(sun) * 0.6 + 0.4).max(0.1)
                } else {
                    if normal.dot(looking) > 0.0 {
                        normal = -normal;
                    }
                    normal.dot(sun).max(0.0)
                };
                let sky = 0.6 + 0.4 * normal.y;
                self.colours[index] = albedo * (SUN_LIGHT * lit + SKY_LIGHT * sky);
                self.depths[index] = depth;
            }
        }
    }

    /// Scales it down to `width` × `height` and turns it into sRGB.
    fn finish(self, width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let mut sum = Vec3::ZERO;
            for sy in 0..OVERSAMPLE {
                for sx in 0..OVERSAMPLE {
                    sum += self.colours[((y * OVERSAMPLE + sy) * self.width + x * OVERSAMPLE + sx) as usize];
                }
            }
            let colour = sum / (OVERSAMPLE * OVERSAMPLE) as f32;
            Rgba([to_srgb(colour.x), to_srgb(colour.y), to_srgb(colour.z), 255])
        })
    }
}

/// Twice the signed area of the triangle `a`, `b`, `p` (on the screen).
fn edge(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// A texture's colour (linear) and coverage at `uv`, bilinearly filtered.
fn sample(texture: &RgbaImage, uv: Vec2, repeats: bool) -> (Vec3, f32) {
    let (width, height) = (texture.width() as i32, texture.height() as i32);
    let (x, y) = (uv.x * width as f32 - 0.5, uv.y * height as f32 - 0.5);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let texel = |tx: i32, ty: i32| {
        let (tx, ty) = if repeats {
            (tx.rem_euclid(width), ty.rem_euclid(height))
        } else {
            (tx.clamp(0, width - 1), ty.clamp(0, height - 1))
        };
        let pixel = texture.get_pixel(tx as u32, ty as u32);
        let linear = |byte: u8| from_srgb(byte);
        (Vec3::new(linear(pixel[0]), linear(pixel[1]), linear(pixel[2])), f32::from(pixel[3]) / 255.0)
    };
    let (a, b, c, d) = (texel(x0, y0), texel(x0 + 1, y0), texel(x0, y0 + 1), texel(x0 + 1, y0 + 1));
    let top = (a.0.lerp(b.0, fx), a.1 + (b.1 - a.1) * fx);
    let bottom = (c.0.lerp(d.0, fx), c.1 + (d.1 - c.1) * fx);
    (top.0.lerp(bottom.0, fy), top.1 + (bottom.1 - top.1) * fy)
}

fn from_srgb(byte: u8) -> f32 {
    let value = f32::from(byte) / 255.0;
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 { value * 12.92 } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 };
    (encoded * 255.0).round() as u8
}

/// A flat, coloured box (for the ground, or a person to show the scale),
/// from `low` to `high`.
pub fn block(low: Vec3, high: Vec3, colour: [f32; 3]) -> MeshPart {
    let mut mesh = MeshPart::default();
    let faces: [(Vec3, [Vec3; 4]); 6] = [
        (Vec3::Y, [Vec3::new(0., 1., 0.), Vec3::new(1., 1., 0.), Vec3::new(1., 1., 1.), Vec3::new(0., 1., 1.)]),
        (Vec3::NEG_Y, [Vec3::new(0., 0., 0.), Vec3::new(0., 0., 1.), Vec3::new(1., 0., 1.), Vec3::new(1., 0., 0.)]),
        (Vec3::X, [Vec3::new(1., 0., 0.), Vec3::new(1., 0., 1.), Vec3::new(1., 1., 1.), Vec3::new(1., 1., 0.)]),
        (Vec3::NEG_X, [Vec3::new(0., 0., 0.), Vec3::new(0., 1., 0.), Vec3::new(0., 1., 1.), Vec3::new(0., 0., 1.)]),
        (Vec3::Z, [Vec3::new(0., 0., 1.), Vec3::new(0., 1., 1.), Vec3::new(1., 1., 1.), Vec3::new(1., 0., 1.)]),
        (Vec3::NEG_Z, [Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(1., 1., 0.), Vec3::new(0., 1., 0.)]),
    ];
    let size = high - low;
    for (normal, corners) in faces {
        let first = mesh.positions.len() as u32;
        for corner in corners {
            mesh.positions.push((low + corner * size).to_array());
            mesh.normals.push(normal.to_array());
            mesh.uvs.push([0.0, 0.0]);
            mesh.colours.push([colour[0], colour[1], colour[2], 1.0]);
            mesh.sway.push(0.0);
        }
        mesh.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_in_front_of_the_sky_shows() {
        let red = block(Vec3::splat(-1.0), Vec3::splat(1.0), [1.0, 0.0, 0.0]);
        let thing = Drawable { mesh: &red, texture: None, repeats: false, leaves: false, offset: Vec3::ZERO };
        let view = View { centre: Vec3::ZERO, looking: Vec3::new(0.0, -0.3, -1.0).normalize(), metres_per_pixel: 0.1 };
        let picture = render(&[thing], view, 64, 64);
        let middle = picture.get_pixel(32, 32);
        assert!(middle[0] > 100 && middle[1] < 30, "{middle:?}");
        let corner = picture.get_pixel(1, 1);
        assert!(corner[2] > corner[0], "sky in the corner: {corner:?}");
    }
}
