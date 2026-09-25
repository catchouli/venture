# Code style

The goal is code that a person can pick up and understand quickly, and change
safely, years from now.

## Structure

- **Small modules with one job each.** A module's first lines (`//!`) say what
  it is for. If that's hard to write in a sentence or two, split the module.
- **Data and plain functions first.** Add a trait only when there is a real
  second implementation.
- **Respect the crate boundaries** in [architecture.md](architecture.md). The
  world crates never depend on the engine.
- **Pipelines read top to bottom.** When something is built in stages (like a
  terrain height), write the stages in order in one function, with a comment
  listing them. See `corner_height` in `crates/worldgen/src/chunk_gen.rs`.

## Naming and comments

- Use full, plain words: `river_bank_width`, not `rbw`.
- Every public item has a doc comment. Say what something *is* or *means*, and
  give units (metres, radians, cells) wherever a number has them.
- Comments explain *why*. The code already says *what*.
- Name the well-known algorithms you use (Priority-Flood, A*, Chaikin,
  chamfer distance transform) and cite them in the module docs, so readers can
  look them up.

## Numbers

- **No magic numbers.** World generation tunables go in `WorldGenConfig`, with
  a doc comment each. Constants that belong to one module are named `const`s at
  the top of that module.
- Units: one unit is one metre, everywhere.

## Determinism

World generation must give identical results on every machine, in every run,
and in any chunk order. See "Determinism rules" in
[world-generation-engine.md](world-generation-engine.md).

## Checks

Before committing:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Engine-independent logic has unit tests. Tests describe behaviour in their
names (`neighbouring_chunks_share_edge_heights`).
