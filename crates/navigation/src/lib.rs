//! Where characters can walk, and how they get from one place to another.
//!
//! - [`ChunkObstacles`] records which terrain cells of a chunk are blocked
//!   (deep water, lava, steep slopes, tree trunks and rocks).
//! - [`find_path`] finds a route between two points with A* search over the
//!   cells, then smooths it by skipping every waypoint that has a clear line of
//!   sight past it, so characters run in straight lines instead of zigzags.
//!
//! This crate has no engine dependencies, so the server can use it too.
//! See `docs/world-model.md` for how this will grow into a multi-layer
//! navigation grid.

mod obstacles;
mod path;

pub use obstacles::{ChunkObstacles, WalkingLimits};
pub use path::{Point, find_path};
