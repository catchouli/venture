# World model

How the world is represented as data. The types live in `crates/world`.

## Units and axes

- **One unit is one metre.** Heights are in metres too. The default character
  is 1.8 m tall.
- Horizontal positions are `(x, y)`: `x` grows to the east and `y` to the south
  (matching image and map coordinates). Height is separate.
- In the client, world `x` is Bevy's X axis, world `y` is Bevy's Z axis, and
  height is Bevy's Y (up) axis (Bevy is Y-up and right-handed). This mapping lives only in
  `crates/client/src/coords.rs`.
- **Nothing is locked to a grid.** Characters, monsters and objects have
  continuous positions. Grids are only how the terrain is stored.

## Terrain cells and chunks

- The ground is a **heightfield** sampled every metre. Cell `(x, y)`
  (`CellPos`) covers the square from `(x, y)` to `(x + 1, y + 1)`.
- Heights are stored at cell **corners**. Each cell also has a `TerrainCell`:
  its ground material (`Ground`), any liquid over it (`Fluid`: water or lava,
  with a surface height) and its `Biome`.
- The world is split into **chunks** of 64 × 64 cells (`ChunkPos`). A `Chunk`
  holds 65 × 65 corner heights, 64 × 64 cells, and the props (trees, rocks,
  bushes) standing on it. Corners on a chunk's edge are shared with, and
  identical to, the neighbouring chunk's.
- Chunks are the unit of generation, streaming, meshing and (later) networking.

## Layers or full 3D?

Early on, the world was planned as tile layers (`z = 0` ground, `z = 1` upper
floors, `z = -1` caves). Now that the world is continuous and
higher-fidelity, here are the options and the decision.

### The options

1. **Fixed layers.** Each position has numbered floors. This is simple, but
   it can't describe overhangs, slopes between floors, bridges over a
   valley, or caves at different depths. Every building and cave has to fit
   the fixed floor heights.
2. **Full 3D volume (voxels).** The whole world is a density field, meshed
   with marching cubes or similar. It allows anything, including destructible
   terrain. But it costs far more memory and bandwidth, meshing and level of
   detail are much harder, the surfaces look blobby unless you do a lot of
   extra work, and pathfinding and server checks get complicated. It's only
   worth it if players dig and reshape the world, which isn't part of this game.
3. **Heightfield terrain + 3D structures + a multi-layer navigation grid.**
   The ground is a heightfield (fast, compact, easy to generate, edit and
   stream). Anything the heightfield can't express is a **3D structure**
   placed in the world: buildings, bridges, cliff overhangs, cave entrances,
   and cave and dungeon interiors. Walking is handled by a separate
   **navigation grid** where each cell has a *list* of walkable surfaces at
   different heights.

### Decision: option 3

This is how most MMOs of Lineage 2's era and since are built. Lineage 2's own
"geodata" is exactly this kind of multi-layer grid. It gives us:

- **Ditches, gorges, cliffs and river channels** directly in the heightfield
  (which already carves river channels).
- **Overhangs, arches and bridges** as structures placed on the terrain.
- **Caves**, in two ways:
  - *Cave interiors* are 3D spaces below the surface: either hand-built from
    modular kit pieces, or generated. A generated cave is a density volume
    confined to the cave's bounding box, meshed with marching cubes / surface
    nets, so we get true 3D only where we need it.
  - *Cave entrances* are structures set into hillsides, with a hole cut in the
    terrain mesh where needed (a per-cell "hole" flag).
- **Upper floors and stairs** as parts of buildings, with no global floor numbers.

The old "layer" idea survives in the navigation grid: at any `(x, y)` there
may be several places to stand (the valley floor, the bridge above it, the
cave below), and the grid records each one. Characters move freely in 3D, and
the server checks movement against the grid.

### Where we are now

`venture_navigation` has the first, single-layer version: each chunk has a
grid of blocked cells (`ChunkObstacles`: deep water, lava, slopes steeper than
a step, tree trunks and boulders), and `find_path` runs A* over it and then
smooths the route with line-of-sight checks. The multi-layer grid below
replaces the blocked-cell grid when structures arrive.

### What this means for the data (planned)

- `Chunk` keeps the heightfield (done) and gains a per-cell hole flag.
- A new `Structure` type: a placed piece of 3D content (a model or a
  generated volume) with a position, rotation and footprint. Structures come
  from the generator (cave entrances, standing stones) and from authored
  overlays (towns, dungeons).
- A `NavGrid` per chunk: for each cell, a small list of walkable surfaces
  (height, flags such as water or no-go), built from the heightfield plus
  structures. Used for click-to-move pathfinding on the client and movement
  checks on the server.
- Transitions between surfaces (stairs, ladders, cave mouths) are simply
  places where surfaces connect in the nav grid. Ladders and similar can be
  interactive links between two surfaces.

## Props

`Prop`s are the things scattered over the land: trees (oak, birch, pine, dead
trees), boulders and bushes. Each has a kind, a position, the ground height at
its base, a scale and a rotation. They are part of the world's data, not just
decoration, because they will become resources for skills (felling trees,
mining boulders) and obstacles for movement.
