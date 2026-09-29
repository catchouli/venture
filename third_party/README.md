# Vendored crates

Copies of published crates with fixes we need before they're released.
Each is used through `[patch.crates-io]` in the workspace's `Cargo.toml`.

## rerecast 0.4.0

The Rust port of Recast that bakes navigation meshes. Its licences are in its folder.

We changed one thing, in `src/poly_mesh.rs` in `triangulate`. The squared length of each candidate ear's diagonal was
cast to `u16` (twice). That overflows for tiles more than about 180 cells across, so the ear-clipping picked the wrong
ears and dropped most of a tile's polygons. Our tiles are 410 cells across. The C++ original uses `int`, and the lengths
are now `u32`.

Once a fixed release is out, remove this copy and the patch.
