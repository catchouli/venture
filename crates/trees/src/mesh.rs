//! Turning a skeleton into meshes: bark and leaves (y up, metres, the base
//! of the trunk at the origin).
//!
//! - **Bark:** each branch is a generalised cylinder (a tube) along its
//!   points, its rings kept from twisting with parallel-transport frames
//!   (Bishop, "There is more than one way to frame a curve", 1975). Thick
//!   branches get more sides. Stems flare at the ground and can be fluted.
//!   The texture keeps the same size on thick and thin branches. The
//!   painted bark is grey detail, coloured by the vertex colours (see
//!   [`crate::textures`]), which is how a Scots pine turns orange up its
//!   trunk.
//! - **Leaves:** each leaf card is a quad from its twig outwards, showing
//!   one of the sprig pictures. Its normal points out from the middle of
//!   the crown (blended a little with the card's own), so the crown is lit
//!   as one soft volume. Its colour darkens towards the crown's heart and
//!   underside, a cheap stand-in for the shade of the leaves around it.
//!
//! Every vertex also has a **sway**: how much wind moves it, from 0 (the
//! trunk's base) to 1 (a leaf's tip), for a wind shader later.

use glam::Vec3;

use crate::skeleton::{SPRIG_PICTURES, Skeleton};
use crate::species::Species;

/// The most sides a branch's rings have.
const MOST_SIDES: u32 = 14;
/// Rings get a side for each this many metres round them.
const SIDE_WIDTH: f32 = 0.09;
/// Branches thinner than this at their base get no bark (a sprig picture shows them), in metres.
const THINNEST_BARK: f32 = 0.006;
/// The most bark triangles a tree has, close up (a quarter of that further away).
pub const MOST_BARK_TRIANGLES: usize = 16_000;
/// How much a leaf card's normal follows the crown (the rest is its own).
const CROWN_NORMALS: f32 = 0.8;
/// How dark the heart of the crown is (1 is as light as the outside).
const CROWN_HEART: f32 = 0.45;

/// How much detail to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Everything (close up).
    Full,
    /// Without the thinnest branches' bark, and half as many leaf cards, each
    /// bigger (for further away).
    Reduced,
}

/// A mesh: triangles with a position, normal, texture coordinate, colour
/// and sway at each vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshPart {
    /// Positions, in metres.
    pub positions: Vec<[f32; 3]>,
    /// Normals (unit vectors).
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates (v down, as in glTF).
    pub uvs: Vec<[f32; 2]>,
    /// Colours (linear), multiplying the texture.
    pub colours: Vec<[f32; 4]>,
    /// How much wind moves each vertex, from 0 to 1.
    pub sway: Vec<f32>,
    /// Three vertex indices a triangle.
    pub indices: Vec<u32>,
}

impl MeshPart {
    /// How many triangles it has.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// The corners of the box around it, or `None` if it's empty.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.positions.iter().map(|&p| Vec3::from(p));
        let first = points.next()?;
        Some(points.fold((first, first), |(low, high), p| (low.min(p), high.max(p))))
    }

    fn push(&mut self, position: Vec3, normal: Vec3, uv: [f32; 2], colour: [f32; 4], sway: f32) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position.to_array());
        self.normals.push(normal.to_array());
        self.uvs.push(uv);
        self.colours.push(colour);
        self.sway.push(sway);
        index
    }
}

/// A tree's meshes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeMeshes {
    /// The bark.
    pub bark: MeshPart,
    /// The leaves.
    pub leaves: MeshPart,
}

/// Builds a skeleton's meshes, for a bark picture `bark_aspect` times as tall as it's wide.
pub fn build(species: &Species, skeleton: &Skeleton, detail: Detail, bark_aspect: f32) -> TreeMeshes {
    TreeMeshes { bark: bark(species, skeleton, detail, bark_aspect), leaves: leaves(skeleton, detail) }
}

/// sRGB (0 to 1) to linear.
fn linear(value: f32) -> f32 {
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn bark(species: &Species, skeleton: &Skeleton, detail: Detail, bark_aspect: f32) -> MeshPart {
    let mut mesh = MeshPart::default();
    let deepest = skeleton.branches.iter().map(|branch| branch.level).max().unwrap_or(0);
    let bark = &species.bark;
    let tile = bark.scale.max(0.05);
    let (lobes, lobe_depth) = species.trunk.lobes;
    let height = skeleton.height.max(0.01);
    let sides_for = |radius: f32| ((std::f32::consts::TAU * radius / SIDE_WIDTH).round() as u32).clamp(3, MOST_SIDES);
    // Within the budget, thickest first: the thinnest go when there are too many.
    let budget = match detail {
        Detail::Full => MOST_BARK_TRIANGLES,
        Detail::Reduced => MOST_BARK_TRIANGLES / 4,
    };
    let mut by_thickness: Vec<usize> = (0..skeleton.branches.len()).collect();
    by_thickness.sort_by(|&a, &b| skeleton.branches[b].radii[0].total_cmp(&skeleton.branches[a].radii[0]));
    let mut spent = 0;
    let mut barked = vec![false; skeleton.branches.len()];
    for index in by_thickness {
        let branch = &skeleton.branches[index];
        if branch.radii[0] < THINNEST_BARK || (detail == Detail::Reduced && branch.level == deepest && deepest > 0) {
            continue;
        }
        let cost = 2 * sides_for(branch.radii[0]) as usize * (branch.points.len() - 1);
        if spent + cost > budget {
            continue;
        }
        spent += cost;
        barked[index] = true;
    }
    for (index, branch) in skeleton.branches.iter().enumerate() {
        if !barked[index] {
            continue;
        }
        let base_radius = branch.radii[0];
        let sides = sides_for(base_radius);
        let (sides, lobes) = if branch.from_ground && lobes > 0 { (sides.max(lobes * 3), lobes) } else { (sides, 0) };
        // The texture wraps round a whole number of times, about its own size each.
        let wraps = ((std::f32::consts::TAU * base_radius / tile).round()).max(1.0);
        let count = branch.points.len();
        let tangent = |i: usize| {
            let (a, b) = (branch.points[i.saturating_sub(1)], branch.points[(i + 1).min(count - 1)]);
            (b - a).normalize_or(Vec3::Y)
        };
        let mut normal = tangent(0).any_orthonormal_vector();
        let mut previous_tangent = tangent(0);
        let mut travelled = 0.0;
        let first = mesh.positions.len() as u32;
        for i in 0..count {
            let along_tangent = tangent(i);
            // Parallel transport: turn last ring's frame by the curve's turn.
            let turn = glam::Quat::from_rotation_arc(previous_tangent, along_tangent);
            normal = (turn * normal).reject_from(along_tangent).normalize_or(along_tangent.any_orthonormal_vector());
            previous_tangent = along_tangent;
            let binormal = along_tangent.cross(normal);
            if i > 0 {
                travelled += branch.points[i].distance(branch.points[i - 1]);
            }
            let point = branch.points[i];
            let up_the_tree = (point.y / height).clamp(0.0, 1.0);
            let colour = bark_colour(species, up_the_tree);
            let fluting = if lobes > 0 { lobe_depth * (1.0 - up_the_tree * 2.0).max(0.0) } else { 0.0 };
            let sway = (branch.level as f32 * 0.25 + up_the_tree * 0.2).min(0.8);
            for side in 0..=sides {
                let angle = std::f32::consts::TAU * side as f32 / sides as f32;
                let out = normal * angle.cos() + binormal * angle.sin();
                let radius = branch.radii[i] * (1.0 + fluting * (angle * lobes as f32).cos());
                let uv = [wraps * side as f32 / sides as f32, travelled / (tile * bark_aspect.max(0.1))];
                mesh.push(point + out * radius, out, uv, colour, sway);
            }
        }
        let ring = sides + 1;
        for i in 0..count as u32 - 1 {
            for side in 0..sides {
                let a = first + i * ring + side;
                let b = a + ring;
                // Counter-clockwise seen from outside (so the outside is the front).
                mesh.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
            }
        }
    }
    mesh
}

/// The bark's colour (linear) `up_the_tree` (0 at the ground, 1 at the top),
/// for painted bark; photographed bark has its own colour.
fn bark_colour(species: &Species, up_the_tree: f32) -> [f32; 4] {
    let bark = &species.bark;
    if bark.texture.is_some() {
        return [1.0; 4];
    }
    let colour = match bark.top_colour {
        Some(top) => bark.colours.0.mix(top, smoothstep(0.3, 0.6, up_the_tree)),
        None => bark.colours.0,
    };
    [linear(colour.0), linear(colour.1), linear(colour.2), 1.0]
}

fn leaves(skeleton: &Skeleton, detail: Detail) -> MeshPart {
    let mut mesh = MeshPart::default();
    let Some((low, high)) = crown_bounds(skeleton) else { return mesh };
    let centre = (low + high) / 2.0;
    let half = ((high - low) / 2.0).max(Vec3::splat(0.1));
    for (index, leaf) in skeleton.leaves.iter().enumerate() {
        let (scale, keep) = match detail {
            Detail::Full => (1.0, true),
            Detail::Reduced => (std::f32::consts::SQRT_2, index % 2 == 0),
        };
        if !keep {
            continue;
        }
        let (width, length) = (leaf.width * scale, leaf.length * scale);
        let right = leaf.along.cross(leaf.facing).normalize_or(Vec3::X);
        let facing = right.cross(leaf.along);
        // The card's own normal, then turned out from the crown.
        let from_centre = (leaf.position + leaf.along * length / 2.0 - centre) / half;
        let crown_normal = from_centre.normalize_or(Vec3::Y);
        let own = if facing.dot(crown_normal) < 0.0 { -facing } else { facing };
        let normal = (crown_normal * CROWN_NORMALS + own * (1.0 - CROWN_NORMALS)).normalize_or(Vec3::Y);
        // Shade: darker in the heart and underneath.
        let depth = smoothstep(0.0, 1.0, from_centre.length());
        let underneath = 0.8 + 0.2 * ((leaf.position.y - low.y) / (high.y - low.y).max(0.1));
        let shade = (CROWN_HEART + (1.0 - CROWN_HEART) * depth) * underneath;
        let colour = [shade, shade, shade, 1.0];
        // The sprig's picture: a cell of the atlas, the twig's base at the bottom middle.
        let [u0, u1, v_top, v_bottom] = sprig_cell(leaf.picture);
        let base = leaf.position;
        let tip = base + leaf.along * length;
        let half_width = right * width / 2.0;
        let a = mesh.push(base - half_width, normal, [u0, v_bottom], colour, 0.85);
        mesh.push(base + half_width, normal, [u1, v_bottom], colour, 0.85);
        mesh.push(tip + half_width, normal, [u1, v_top], colour, 1.0);
        mesh.push(tip - half_width, normal, [u0, v_top], colour, 1.0);
        mesh.indices.extend([a, a + 1, a + 2, a, a + 2, a + 3]);
    }
    mesh
}

/// Where sprig picture `picture` (0 to [`SPRIG_PICTURES`] - 1) is in a
/// species' leaf atlas: `[left, right, top, bottom]` texture coordinates.
/// The twig's base is at the bottom middle.
pub fn sprig_cell(picture: u32) -> [f32; 4] {
    let columns = SPRIG_PICTURES.div_ceil(2);
    let picture = picture % SPRIG_PICTURES;
    let (column, row) = (picture % columns, picture / columns);
    let cell = 1.0 / columns as f32;
    [column as f32 * cell, (column + 1) as f32 * cell, row as f32 * cell, (row + 1) as f32 * cell]
}

/// The box round every leaf.
fn crown_bounds(skeleton: &Skeleton) -> Option<(Vec3, Vec3)> {
    let mut points = skeleton.leaves.iter().map(|leaf| leaf.position);
    let first = points.next()?;
    Some(points.fold((first, first), |(low, high), p| (low.min(p), high.max(p))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::TreeParams;
    use crate::skeleton::grow;
    use crate::species::{Bark, BranchLevel, CrownShape, Leaves, Trunk};

    fn species() -> Species {
        Species {
            id: "test".to_string(),
            name: "Test".to_string(),
            height: (15.0, 20.0),
            trunk: Trunk::default(),
            crown: CrownShape::Spherical,
            branches: vec![BranchLevel::default(), BranchLevel { count: 8.0, ..BranchLevel::default() }],
            leaves: Leaves::default(),
            bark: Bark::default(),
        }
    }

    #[test]
    fn meshes_are_whole_and_reduced_ones_are_smaller() {
        let species = species();
        let skeleton = grow(&species, &TreeParams::default());
        let full = build(&species, &skeleton, Detail::Full, 1.0);
        let reduced = build(&species, &skeleton, Detail::Reduced, 1.0);
        for part in [&full.bark, &full.leaves] {
            let vertices = part.positions.len();
            assert!(part.triangles() > 0);
            assert!(part.indices.iter().all(|&i| (i as usize) < vertices));
            assert_eq!(part.normals.len(), vertices);
            assert!(part.normals.iter().all(|n| (Vec3::from(*n).length() - 1.0).abs() < 1e-3));
        }
        assert!(reduced.bark.triangles() < full.bark.triangles());
        assert!(reduced.leaves.triangles() < full.leaves.triangles());
    }

    #[test]
    fn bark_faces_outwards() {
        let species = species();
        let bark = build(&species, &grow(&species, &TreeParams::default()), Detail::Full, 1.0).bark;
        let at = |i: u32| Vec3::from(bark.positions[i as usize]);
        let mut facing_out = 0;
        let triangles = bark.indices.as_chunks::<3>().0;
        for &[a, b, c] in triangles {
            let face = (at(b) - at(a)).cross(at(c) - at(a));
            let normal = Vec3::from(bark.normals[a as usize]);
            if face.length() > 1e-9 && face.dot(normal) > 0.0 {
                facing_out += 1;
            }
        }
        // (Degenerate slivers at branch tips aside, every triangle faces out.)
        assert!(facing_out as f32 > triangles.len() as f32 * 0.95, "{facing_out} of {}", triangles.len());
    }
}
