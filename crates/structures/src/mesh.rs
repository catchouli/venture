//! Mesh data (y up, metres) and the shapes structures are built from.

use glam::{Quat, Vec3};

/// A mesh: triangles with a position, normal, texture coordinate and colour
/// at each vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshPart {
    /// Positions, in metres.
    pub positions: Vec<[f32; 3]>,
    /// Normals (unit vectors).
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates (a unit a metre, so textures keep their size).
    pub uvs: Vec<[f32; 2]>,
    /// Colours (linear), multiplying the texture.
    pub colours: Vec<[f32; 4]>,
    /// Three vertex indices a triangle.
    pub indices: Vec<u32>,
}

impl MeshPart {
    /// How many triangles it has.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether it has nothing in it.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Adds a flat quad (corners anticlockwise seen from the front).
    pub fn quad(&mut self, corners: [Vec3; 4], uvs: [[f32; 2]; 4], colour: [f32; 4]) {
        let normal = (corners[1] - corners[0]).cross(corners[3] - corners[0]).normalize_or(Vec3::Y);
        let first = self.positions.len() as u32;
        for (corner, uv) in corners.iter().zip(uvs) {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv);
            self.colours.push(colour);
        }
        self.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    /// Adds a box (a stone, a post, a rail): `size` across (x), up (y) and
    /// along (z) its own axes, turned by `rotation` and centred on `centre`.
    /// Each corner is pushed out or in by `lumps` (a share of the size,
    /// per corner: irregular stones), and the texture is laid on each face
    /// from `uv_offset`, a unit a metre.
    pub fn block(
        &mut self,
        centre: Vec3,
        rotation: Quat,
        size: Vec3,
        lumps: [f32; 8],
        uv_offset: [f32; 2],
        colour: [f32; 4],
    ) {
        let half = size / 2.0;
        let corner = |i: usize| {
            let sign = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            );
            centre + rotation * (sign * half * (1.0 + lumps[i]))
        };
        let c: [Vec3; 8] = std::array::from_fn(corner);
        // Each face: its corners (anticlockwise from outside), and its size for the texture.
        let faces = [
            ([c[1], c[3], c[7], c[5]], size.z, size.y), // +x
            ([c[4], c[6], c[2], c[0]], size.z, size.y), // -x
            ([c[2], c[6], c[7], c[3]], size.x, size.z), // +y
            ([c[0], c[1], c[5], c[4]], size.x, size.z), // -y
            ([c[4], c[5], c[7], c[6]], size.x, size.y), // +z
            ([c[0], c[2], c[3], c[1]], size.x, size.y), // -z
        ];
        let [u, v] = uv_offset;
        for (corners, width, height) in faces {
            let uvs = [[u, v + height], [u + width, v + height], [u + width, v], [u, v]];
            self.quad(corners, uvs, colour);
        }
    }
}

/// An sRGB colour (0 to 1) as linear, with full coverage.
pub fn linear([r, g, b]: [f32; 3]) -> [f32; 4] {
    let channel = |value: f32| {
        if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    [channel(r), channel(g), channel(b), 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_face_outwards() {
        let mut mesh = MeshPart::default();
        mesh.block(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE, [0.0; 8], [0.0; 2], [1.0; 4]);
        assert_eq!(mesh.triangles(), 12);
        for (position, normal) in mesh.positions.iter().zip(&mesh.normals) {
            // On a centred box, each face's normal points the same way as its corners.
            assert!(Vec3::from(*position).dot(Vec3::from(*normal)) > 0.0);
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| Vec3::from(mesh.positions[i as usize]));
            let face = (b - a).cross(c - a);
            let normal = Vec3::from(mesh.normals[triangle[0] as usize]);
            assert!(face.dot(normal) > 0.0, "wound anticlockwise from outside");
        }
    }
}
