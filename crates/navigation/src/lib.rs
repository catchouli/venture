//! Where characters can walk, and how they get from one place to another,
//! on navigation meshes (see `docs/plan-collision.md`):
//!
//! - [`build_tile`] makes a chunk's navigation mesh tile from the world's
//!   solid shapes, with Recast (at bake time).
//! - [`NavWorld`] joins the loaded chunks' tiles, and finds routes over them
//!   with landmass: any-angle, round walls and through doorways, preferring
//!   dry ground to shallow water.
//!
//! This crate has no engine dependencies, so the server and the bake use it too.

pub mod navmesh;
mod navworld;

pub use navmesh::build_tile;
pub use navworld::{NavWorld, Point3};

/// A position in metres: `(x, y)` on the horizontal plane.
pub type Point = (f32, f32);
