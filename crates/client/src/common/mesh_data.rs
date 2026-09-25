//! A simple growable mesh: vertex lists plus triangle indices, turned into a
//! Bevy [`Mesh`] at the end. Shared by the terrain, liquid and prop builders.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// Vertices and triangles being built up.
#[derive(Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colours: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Adds a vertex and returns its index.
    pub fn vertex(&mut self, position: Vec3, normal: Vec3, colour: [f32; 4], uv: Vec2) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.colours.push(colour);
        self.uvs.push(uv.to_array());
        index
    }

    /// Adds a triangle. Corners must be counter-clockwise seen from the front.
    pub fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }

    /// Appends a Bevy mesh (such as a primitive shape), transformed and tinted.
    /// The mesh must have positions, normals and indices.
    pub fn append(&mut self, mesh: &Mesh, transform: &Transform, colour: [f32; 4]) {
        let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Float32x3(normals))) =
            (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL))
        else {
            return;
        };
        let first = self.positions.len() as u32;
        for (&position, &normal) in positions.iter().zip(normals) {
            let position = transform.transform_point(Vec3::from(position));
            let normal = (transform.rotation * (Vec3::from(normal) / transform.scale)).normalize_or_zero();
            self.vertex(position, normal, colour, Vec2::ZERO);
        }
        if let Some(indices) = mesh.indices() {
            self.indices.extend(indices.iter().map(|index| first + index as u32));
        }
    }

    /// Appends another `MeshData`, transformed and with its colours multiplied by `tint`.
    pub fn append_data(&mut self, other: &MeshData, transform: &Transform, tint: [f32; 4]) {
        let first = self.positions.len() as u32;
        for i in 0..other.positions.len() {
            let position = transform.transform_point(Vec3::from(other.positions[i]));
            let normal = (transform.rotation * (Vec3::from(other.normals[i]) / transform.scale)).normalize_or_zero();
            let colour = std::array::from_fn(|c| other.colours[i][c] * tint[c]);
            self.vertex(position, normal, colour, Vec2::from(other.uvs[i]));
        }
        self.indices.extend(other.indices.iter().map(|index| first + index));
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colours)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}
