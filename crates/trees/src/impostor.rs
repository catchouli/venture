//! Impostors: what a far-away tree is drawn as. Two crossed cards, each
//! showing a picture of the tree (from the front and from the side), drawn
//! by [`crate::preview`] as cut-outs of its unlit colours. The game lights
//! them itself: their normals point out from the middle of the crown, like
//! the leaf cards' (see [`crate::mesh`]), so near and far trees are lit
//! alike.

use glam::Vec3;
use image::RgbaImage;

use crate::mesh::MeshPart;
use crate::preview::{Drawable, View, render_cutout};
use crate::{Tree, TreeTextures};

/// Each picture's size, in pixels (the texture is two side by side).
pub const PICTURE_SIZE: u32 = 256;

/// A tree's impostor.
#[derive(Clone, Debug)]
pub struct Impostor {
    /// Its cards (four triangles).
    pub mesh: MeshPart,
    /// The pictures: the front on the left, the side on the right.
    pub texture: RgbaImage,
}

/// Makes a tree's impostor.
pub fn impostor(tree: &Tree, textures: &TreeTextures) -> Impostor {
    // A square that holds the whole tree, standing on the ground.
    let size = (tree.crown_radius * 2.0).max(tree.height).max(0.5);
    let things = [
        Drawable {
            mesh: &tree.meshes.bark,
            texture: Some(&textures.bark),
            repeats: true,
            leaves: false,
            offset: Vec3::ZERO,
        },
        Drawable {
            mesh: &tree.meshes.leaves,
            texture: Some(&textures.leaves),
            repeats: false,
            leaves: true,
            offset: Vec3::ZERO,
        },
    ];
    let centre = Vec3::new(0.0, size / 2.0, 0.0);
    let metres_per_pixel = size / PICTURE_SIZE as f32;
    let front =
        render_cutout(&things, View { centre, looking: Vec3::NEG_Z, metres_per_pixel }, PICTURE_SIZE, PICTURE_SIZE);
    let side =
        render_cutout(&things, View { centre, looking: Vec3::NEG_X, metres_per_pixel }, PICTURE_SIZE, PICTURE_SIZE);
    let mut texture = RgbaImage::new(PICTURE_SIZE * 2, PICTURE_SIZE);
    image::imageops::replace(&mut texture, &front, 0, 0);
    image::imageops::replace(&mut texture, &side, i64::from(PICTURE_SIZE), 0);

    // The front picture's right is the world's +x; the side picture's is -z.
    let mut mesh = MeshPart::default();
    let half = size / 2.0;
    let crown = Vec3::new(0.0, tree.height * 0.6, 0.0);
    // Each card: the world's way along its picture's right, and where its picture starts in the texture.
    let cards = [(Vec3::X, 0.0), (Vec3::NEG_Z, 0.5)];
    for (right, u_start) in cards {
        let first = mesh.positions.len() as u32;
        for (across, up, u, v) in
            [(-half, 0.0, 0.0, 1.0), (half, 0.0, 0.5, 1.0), (half, size, 0.5, 0.0), (-half, size, 0.0, 0.0)]
        {
            let position = right * across + Vec3::Y * up;
            let normal = (position - crown).normalize_or(Vec3::Y).lerp(Vec3::Y, 0.3).normalize();
            mesh.positions.push(position.to_array());
            mesh.normals.push(normal.to_array());
            mesh.uvs.push([u_start + u, v]);
            mesh.colours.push([1.0; 4]);
            mesh.sway.push(up / size);
        }
        mesh.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    Impostor { mesh, texture }
}
