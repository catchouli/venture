//! The tree generator: realistic procedural trees from a species (plain
//! data: see [`Species`]) and a few choices for each tree (see
//! [`TreeParams`]: age, height, branchiness, fullness, lean, seed).
//!
//! It knows nothing of the game, the world or Bevy. The game grows its
//! trees with it, and so does the `tree` tool, which writes glTF models and
//! preview pictures (see `docs/plan-trees.md`).
//!
//! A tree is made in steps:
//! 1. [`skeleton`]: its branches, grown after Weber and Penn's parametric
//!    trees, and where its leaf cards go.
//! 2. [`mesh`]: the bark and leaves as meshes, in full or reduced detail.
//! 3. [`textures`]: the species' bark and sprig pictures, painted, or
//!    made from photographs ([`photos`]).
//!
//! Then [`gltf`] writes it as a `.glb`, [`preview`] draws it, and
//! [`impostor`] makes the crossed cards it's drawn as far away.

pub mod gltf;
pub mod impostor;
pub mod mesh;
pub mod params;
pub mod photos;
pub mod preview;
mod random;
pub mod skeleton;
pub mod species;
mod sprigs;
pub mod textures;

use glam::Vec3;

pub use mesh::{Detail, MeshPart, TreeMeshes};
pub use params::TreeParams;
pub use photos::Photos;
pub use skeleton::Skeleton;
pub use species::{Species, parse_species};
pub use textures::TreeTextures;

/// A grown tree: its meshes, and the facts the world needs about it.
#[derive(Clone, Debug)]
pub struct Tree {
    /// Its meshes.
    pub meshes: TreeMeshes,
    /// How tall it is, from the ground to its highest point, in metres.
    pub height: f32,
    /// Its trunk's radius at the ground, in metres (what blocks movement).
    pub trunk_radius: f32,
    /// How far its crown reaches from its trunk, in metres (for spacing trees).
    pub crown_radius: f32,
}

/// Grows a tree of `species` with `params`, in `detail`, for its `textures`.
pub fn grow(species: &Species, params: &TreeParams, detail: Detail, textures: &TreeTextures) -> Tree {
    let skeleton = skeleton::grow(species, params);
    let meshes = mesh::build(species, &skeleton, detail, textures.bark_aspect);
    let (low, high) = [&meshes.bark, &meshes.leaves]
        .into_iter()
        .filter_map(MeshPart::bounds)
        .reduce(|(a_low, a_high), (b_low, b_high)| (a_low.min(b_low), a_high.max(b_high)))
        .unwrap_or((Vec3::ZERO, Vec3::ZERO));
    let crown_radius = [low.x, low.z, high.x, high.z].into_iter().map(f32::abs).fold(0.0, f32::max);
    Tree { meshes, height: high.y.max(0.0), trunk_radius: skeleton.trunk_radius, crown_radius }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../content");

    /// The species in the game's content.
    fn content_species() -> Vec<Species> {
        let path = std::path::Path::new(CONTENT).join("trees/species.ron");
        parse_species(&std::fs::read_to_string(path).expect("the species file")).expect("valid species")
    }

    #[test]
    fn every_species_grows_within_its_height_and_budget() {
        for species in content_species() {
            let photos = Photos::load(&species, std::path::Path::new(CONTENT)).expect("its photographs");
            let textures = textures::paint(&species, &photos);
            for age in [0.2, 1.0, 2.0] {
                let params = TreeParams { age, height: 1.0, seed: 3, ..TreeParams::default() };
                let tree = grow(&species, &params, Detail::Full, &textures);
                let tallest = species.height.1 * 1.4;
                assert!(tree.height > 0.5 && tree.height < tallest, "{} at {age}: {} m", species.id, tree.height);
                let triangles = tree.meshes.bark.triangles() + tree.meshes.leaves.triangles();
                assert!(triangles < 60_000, "{} at {age} has {triangles} triangles", species.id);
                let leafy = species.leaves.per_metre > 0.0;
                assert_eq!(!tree.meshes.leaves.indices.is_empty(), leafy, "{} at {age}: leaves?", species.id);
            }
        }
    }
}
