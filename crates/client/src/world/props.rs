//! Meshes for props that aren't grown trees: rocks, and dead trees (trees
//! and bushes are grown by the tree generator: see [`crate::world::trees`]).
//!
//! Each kind of prop has a template built once from simple shapes. A chunk's
//! props are merged into a single mesh, so they cost one draw call.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use venture_world::{Biome, CHUNK_SIZE, Chunk, PropKind};

use crate::common::coords::to_bevy;
use crate::common::mesh_data::MeshData;
use crate::common::palette::cell_hash;

/// Ready-made meshes for the props drawn here.
pub struct PropTemplates {
    dead_tree: MeshData,
    boulder: MeshData,
}

impl Default for PropTemplates {
    fn default() -> Self {
        Self::new()
    }
}

impl PropTemplates {
    /// Builds every template.
    pub fn new() -> Self {
        Self { dead_tree: dead_tree(), boulder: boulder() }
    }

    /// The template for a kind of prop, unless it's one the tree generator grows.
    fn template(&self, kind: PropKind) -> Option<&MeshData> {
        match kind {
            PropKind::DeadTree => Some(&self.dead_tree),
            PropKind::Boulder => Some(&self.boulder),
            PropKind::Tree { .. } => None,
        }
    }
}

/// All of a chunk's props merged into one mesh, in chunk-local coordinates,
/// or `None` if the chunk has none.
pub fn props_mesh(chunk: &Chunk, templates: &PropTemplates) -> Option<Mesh> {
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    let mut mesh = MeshData::default();
    for prop in &chunk.props {
        let Some(template) = templates.template(prop.kind) else { continue };
        let (local_x, local_y) = (prop.x - origin_x as f32, prop.y - origin_y as f32);
        let transform = Transform::from_translation(to_bevy(local_x, local_y, prop.ground_height))
            .with_rotation(Quat::from_rotation_y(prop.rotation))
            .with_scale(Vec3::splat(prop.scale));

        // Vary each prop's colour a little; volcanic rocks are black basalt.
        let shade = 0.85 + 0.3 * cell_hash((prop.x * 7.0) as i32, (prop.y * 7.0) as i32);
        let cell = chunk
            .cell((local_x as usize).min(CHUNK_SIZE as usize - 1), (local_y as usize).min(CHUNK_SIZE as usize - 1));
        let tint = if prop.kind == PropKind::Boulder && cell.biome == Biome::Volcanic {
            [0.4 * shade, 0.38 * shade, 0.38 * shade, 1.0]
        } else {
            [shade, shade, shade, 1.0]
        };
        mesh.append_data(template, &transform, tint);
    }
    (!mesh.is_empty()).then(|| mesh.into_mesh())
}

/// An sRGB colour as linear RGBA, for vertex colours.
fn colour(r: f32, g: f32, b: f32) -> [f32; 4] {
    Color::srgb(r, g, b).to_linear().to_f32_array()
}

/// A trunk from the ground up to `height`.
fn trunk(mesh: &mut MeshData, radius: f32, height: f32, bark: [f32; 4]) {
    let cylinder = Cylinder::new(radius, height).mesh().resolution(7).build();
    mesh.append(&cylinder, &Transform::from_xyz(0.0, height / 2.0 - 0.2, 0.0), bark);
}

fn dead_tree() -> MeshData {
    let mut mesh = MeshData::default();
    let bark = colour(0.20, 0.17, 0.15);
    trunk(&mut mesh, 0.28, 5.0, bark);
    for i in 0..3 {
        let angle = i as f32 * TAU / 3.0;
        let branch = Cylinder::new(0.1, 2.2).mesh().resolution(5).build();
        let rotation = Quat::from_rotation_y(angle) * Quat::from_rotation_z(FRAC_PI_2 * 0.6);
        let offset = rotation * Vec3::new(0.0, 1.1, 0.0);
        mesh.append(
            &branch,
            &Transform::from_translation(Vec3::new(0.0, 3.2 + i as f32 * 0.6, 0.0) + offset).with_rotation(rotation),
            bark,
        );
    }
    mesh
}

fn boulder() -> MeshData {
    let mut mesh = MeshData::default();
    let sphere = Sphere::new(1.0).mesh().ico(2).expect("low subdivision is valid");
    mesh.append(
        &sphere,
        &Transform::from_xyz(0.0, 0.35, 0.0).with_scale(Vec3::new(1.2, 0.8, 1.0)),
        colour(0.50, 0.49, 0.46),
    );
    // Lumpy, irregular stone: push each vertex in or out by a smooth function of its direction.
    for position in &mut mesh.positions {
        let p = Vec3::from(*position);
        let lump =
            1.0 + 0.18 * (p.x * 3.1 + p.z * 1.7).sin() * (p.y * 2.3 + p.x * 1.3).cos() + 0.08 * (p.z * 5.0).sin();
        let centre = Vec3::new(0.0, 0.35, 0.0);
        *position = (centre + (p - centre) * lump).to_array();
    }
    mesh
}
