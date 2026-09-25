//! Meshes for props (trees, rocks, bushes).
//!
//! Each kind of prop has a template built once from simple shapes. A chunk's
//! props are merged into a single mesh, so a whole wood costs one draw call.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use venture_world::{Biome, CHUNK_SIZE, Chunk, PropKind};

use crate::coords::to_bevy;
use crate::mesh_data::MeshData;
use crate::palette::cell_hash;

/// Ready-made meshes for every kind of prop.
pub struct PropTemplates {
    oak: MeshData,
    birch: MeshData,
    pine: MeshData,
    dead_tree: MeshData,
    boulder: MeshData,
    bush: MeshData,
}

impl PropTemplates {
    /// Builds every template.
    pub fn new() -> Self {
        Self { oak: oak(), birch: birch(), pine: pine(), dead_tree: dead_tree(), boulder: boulder(), bush: bush() }
    }

    fn template(&self, kind: PropKind) -> &MeshData {
        match kind {
            PropKind::Oak => &self.oak,
            PropKind::Birch => &self.birch,
            PropKind::Pine => &self.pine,
            PropKind::DeadTree => &self.dead_tree,
            PropKind::Boulder => &self.boulder,
            PropKind::Bush => &self.bush,
        }
    }
}

/// All of a chunk's props merged into one mesh, in chunk-local coordinates,
/// or `None` if the chunk has none.
pub fn props_mesh(chunk: &Chunk, templates: &PropTemplates) -> Option<Mesh> {
    let (origin_x, origin_y) = chunk.pos.origin_cell();
    let mut mesh = MeshData::default();
    for prop in &chunk.props {
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
        mesh.append_data(templates.template(prop.kind), &transform, tint);
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

/// A leafy blob of foliage.
fn blob(mesh: &mut MeshData, centre: Vec3, radius: Vec3, leaves: [f32; 4]) {
    let sphere = Sphere::new(1.0).mesh().ico(1).expect("low subdivision is valid");
    mesh.append(&sphere, &Transform::from_translation(centre).with_scale(radius), leaves);
}

fn oak() -> MeshData {
    let mut mesh = MeshData::default();
    trunk(&mut mesh, 0.35, 4.5, colour(0.33, 0.25, 0.18));
    let leaves = [colour(0.23, 0.36, 0.13), colour(0.27, 0.40, 0.15), colour(0.20, 0.32, 0.12)];
    let blobs = [
        (Vec3::new(0.0, 5.6, 0.0), 2.4),
        (Vec3::new(1.4, 5.0, 0.6), 1.8),
        (Vec3::new(-1.3, 5.1, -0.5), 1.9),
        (Vec3::new(0.4, 4.8, -1.4), 1.7),
        (Vec3::new(-0.5, 6.6, 0.8), 1.6),
    ];
    for (i, (centre, radius)) in blobs.into_iter().enumerate() {
        blob(&mut mesh, centre, Vec3::new(radius, radius * 0.8, radius), leaves[i % leaves.len()]);
    }
    mesh
}

fn birch() -> MeshData {
    let mut mesh = MeshData::default();
    trunk(&mut mesh, 0.18, 6.5, colour(0.80, 0.78, 0.72));
    let leaves = [colour(0.40, 0.55, 0.20), colour(0.46, 0.60, 0.24)];
    let blobs = [(Vec3::new(0.0, 6.2, 0.0), 1.4), (Vec3::new(0.6, 5.0, 0.3), 1.2), (Vec3::new(-0.5, 4.2, -0.3), 1.1)];
    for (i, (centre, radius)) in blobs.into_iter().enumerate() {
        blob(&mut mesh, centre, Vec3::new(radius, radius * 1.5, radius), leaves[i % leaves.len()]);
    }
    mesh
}

fn pine() -> MeshData {
    let mut mesh = MeshData::default();
    trunk(&mut mesh, 0.25, 3.0, colour(0.30, 0.22, 0.16));
    let needles = [colour(0.13, 0.25, 0.14), colour(0.15, 0.28, 0.15), colour(0.17, 0.31, 0.17)];
    let tiers = [(2.4, 3.6, 2.2), (1.8, 3.0, 4.2), (1.2, 2.6, 6.0), (0.6, 1.8, 7.6)];
    for (i, (radius, height, base)) in tiers.into_iter().enumerate() {
        let cone = Cone { radius, height }.mesh().resolution(8).build();
        mesh.append(&cone, &Transform::from_xyz(0.0, base + height / 2.0, 0.0), needles[i % needles.len()]);
    }
    mesh
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

fn bush() -> MeshData {
    let mut mesh = MeshData::default();
    let leaves = [colour(0.24, 0.36, 0.14), colour(0.28, 0.40, 0.16)];
    let blobs = [(Vec3::new(0.0, 0.5, 0.0), 0.8), (Vec3::new(0.6, 0.4, 0.3), 0.6), (Vec3::new(-0.5, 0.4, -0.2), 0.65)];
    for (i, (centre, radius)) in blobs.into_iter().enumerate() {
        blob(&mut mesh, centre, Vec3::new(radius, radius * 0.75, radius), leaves[i % leaves.len()]);
    }
    mesh
}
