# World generation engine

`crates/worldgen` generates worlds. It is **deterministic**: the same settings
(`WorldGenConfig`) and the same authored overlays always produce exactly the
same world, down to the last bit, in any order and on any machine. It uses
standard, well-known techniques, each named in the code so you can look it up.

The default world is 4 × 4 km, surrounded by sea.

The generator runs **ahead of time**: a bake step writes the finished world
to disk, and the server and clients use the baked chunks (see
[client-server.md](client-server.md)). Bake with
`cargo run --release -p venture_worldgen --bin bake`.

## Overview

```
WorldGenConfig ─┬─► Regions (biomes)
                ├─► NoiseFields
                │
                ▼
     Stage 1: WorldMap (once, whole world, one point per 8 m)
       land height ─► Priority-Flood ─► sea, lakes, lava lakes
                                    └─► flow ─► rivers
       settlement sites ─► A* ─► roads
                │
                ▼
     Overlay (procedural stamps + authored stamps)
                │
                ▼
     Stage 2: generate_chunk (any chunk, any order, full detail)
       heights ─► liquids ─► shores ─► ground materials ─► props
```

## Stage 1: the world map

Built once when the `WorldGenerator` is created (well under a second in
release builds). Points are 8 m apart. Modules: `world_map.rs`,
`regions.rs`, `noise_fields.rs`, `hydrology.rs`, `settlements.rs`, `roads.rs`.

### Biome regions (`regions.rs`)

- Region centres sit on a **jittered grid** (about 850 m apart). Biomes are
  dealt to them like cards from a shuffled deck, so each biome gets close to
  its share, and volcanic regions are then moved so they never touch each other.
- A point belongs mostly to its nearest centre (a **Voronoi diagram**). Near
  borders the weights blend smoothly (a "soft" Voronoi using exponential
  falloff), so terrain changes gradually from one biome to the next.
- Positions are **domain-warped** with noise before finding the nearest
  centre, so borders wander naturally instead of being straight lines.
- Each biome has a `BiomeTerrain` in the config: base height, how mountainous,
  hilly and bumpy it is, where rock starts, how wooded it is, how many
  boulders and bushes it has, and whether it has volcanoes. Every one of these
  numbers is blended across borders by the biome weights.

### Land height (`noise_fields.rs`)

Built from layers of standard coherent noise (Perlin):

1. **Continents:** low-frequency fBm (fractal Brownian motion) plus the
   biome's base height.
2. **Mountains:** ridged multifractal noise, squared to keep valleys broad,
   scaled by how mountainous the biome is.
3. **Hills:** mid-frequency fBm, scaled by a "hilliness" noise (so some areas
   are flat plains) and by the biome.
4. **Volcanoes:** a concave cone with a crater at the centre of each volcanic
   region. The crater fills with lava (see below).
5. **Coast:** the land slopes down into the ocean near the world's edges. The
   continent noise wobbles the coastline.

### Water (`hydrology.rs`)

1. **Priority-Flood** (Barnes, Lehman & Mulla, 2014) fills every depression
   up to its spill height, working inwards from the sea. The neighbour each
   point is first reached from becomes its downstream neighbour, which gives
   every point a path to the sea that never goes uphill (D8 flow routing).
2. Filled basins that are deep and large enough become **lakes**. Smaller ones
   are filled in and become flat land. Small lakes in volcanic land hold **lava**.
3. **Flow accumulation** counts how many points drain through each point.
   Where it's high enough there is a **river**, whose width and depth grow with
   the flow. River points are nudged slightly off the grid so rivers meander.

### Settlements and roads (`settlements.rs`, `roads.rs`)

- **Settlement sites** are scored (flat, dry, not too high, close to a river
  but clear of its floodplain) and chosen best-first with a minimum spacing.
- **Roads** join them. The links are a minimum spanning tree (Prim's
  algorithm), so every settlement is reachable, plus a few extra links between
  close neighbours. Each link is routed with **A\*** search. The cost favours
  flat ground, avoids river crossings, forbids lakes and the sea, and makes
  reusing existing roads cheap, so roads merge into a network. A little random
  cost per cell makes roads wander. Routes are smoothed with **Chaikin's
  corner-cutting algorithm**.

## The overlay system: authored content on top

The generator makes natural terrain. **Overlays** reshape it wherever places
need to be just so. An overlay is an ordered list of **stamps**
(`overlay.rs`):

| Stamp | Effect |
| --- | --- |
| `Flatten { centre, radius, falloff, height }` | Levels the ground in a circle, blending into the land around it. With no height given, it uses the natural terrain height at the centre. |
| `Paint { centre, radius, ground }` | Covers the ground in a circle with a material. |

Stamps come from two sources, applied in this order:

1. **Procedural:** stamps the generator creates, such as each settlement's
   level ground and its town square.
2. **Authored:** stamps written by designers, passed to
   `WorldGenerator::with_authored_stamps`. Later they will be loaded from data
   files (for example RON), one file per place.

Later stamps win where they overlap, so authored content always has the last word.
Props aren't scattered inside stamped areas, so designed places start clean.

### Planned growth

The overlay system is the groundwork for towns and hand-made places. Planned stamps:

- **Structures:** place a building, bridge, cave entrance or dungeon (see
  [world-model.md](world-model.md)).
- **Props and NPCs:** place specific trees, rocks, market stalls and people.
- **Polygon shapes and splines:** paint and flatten along a path (walls,
  plazas, custom roads), not just circles.
- **Carve and raise:** dig moats and quarries, raise mounds.
- **Region overrides:** force a biome, or change generation settings, inside an area.

Designers would work on a generated world: place stamps in an editor (or data
files), regenerate the affected chunks, and see the result straight away.
The full plan (the world edit app, drawn roads, forests, and standalone tree
and building generators) is in [world-authoring.md](world-authoring.md).
Because generation is deterministic, only the stamps need to be stored. The
rest of the world is always regenerated identically.

## Stage 2: chunk generation (`chunk_gen.rs`)

Any chunk can be generated at any time, in any order. For each chunk:

1. **Corner heights**, built in stages (`corner_height`):
   1. the world map height (bilinear interpolation) plus **fine relief**
      (5-octave fBm, scaled per biome) for bumps and mounds,
   2. graded gently down towards lakes and the sea, leaving room for beaches,
   3. levelled under roads,
   4. shaped by overlay stamps,
   5. carved by rivers: a rounded channel, and flat floodplains beside it.
2. **Liquids:** a cell is covered by a river, lake, sea or lava lake when one
   is there and the ground dips below its surface.
3. **Shores:** a **chamfer distance transform** (`distance_field.rs`) finds each
   cell's distance to water. Lakes and the sea get wide beaches, rivers narrow banks.
4. **Ground materials** (`surface.rs`): a short list of readable rules. In
   order: liquid beds, overlay paint, roads, snow on peaks, rock on cliffs,
   rock increasing with height, beaches, then the biome's own ground with
   patches of a second material. Patchy noise makes every border ragged and natural.
5. **Props** (`props.rs`): candidate spots on a jittered 4 m grid. Woodland
   noise groups trees into woods and clearings, and the biome decides the
   species (oaks in grassland, birches in meadows, pines in the highlands,
   dead trees in volcanic land). Boulders favour rocky ground.

Heights and liquids are worked out for a margin around the chunk as well
(needed for beaches), so neighbouring chunks agree.

## Determinism rules

Chunks generated separately must match exactly along their edges. These rules
keep that true:

1. **Every value is a pure function of the config, overlay and position.** No
   state carries over between chunks, and no randomness except seeded hashes
   and noise.
2. **Feature queries must cover every feature that can reach the area.** When
   a chunk collects nearby rivers, roads or stamps, the search radius must be
   at least the feature's full reach, plus the length of a segment. Otherwise
   one chunk sees a feature its neighbour doesn't.
3. **Ties are broken consistently.** Features are always returned in a fixed
   global order, and "nearest" picks the first of equals.
4. **Out-of-reach features must change nothing, exactly.** `lerp(a, b, t)` is
   written as `a * (1 - t) + b * t`, so `t = 0` gives exactly `a` and `t = 1`
   exactly `b`. The more common `a + (b - a) * t` can be off by one bit at
   `t = 1`, which once made chunk edges disagree by a millionth of a metre.

The test `neighbouring_chunks_share_edge_heights` checks this. Keep it passing.

## Tuning

Almost every number is in `WorldGenConfig` (`config.rs`), each with a doc
comment. Use the preview tool to see the effect of a change on the whole world:

```sh
cargo run --release -p venture_worldgen --example preview [seed]
```

## Adding a biome

1. Add it to `Biome` in `crates/world/src/biome.rs` (and to `Biome::ALL`).
2. Give it a `BiomeTerrain` in `WorldGenConfig` and handle it in `WorldGenConfig::biome`.
3. Add its share to `BIOME_ODDS` in `regions.rs`.
4. Give it ground rules in `surface.rs` and tree species in `props.rs`.
5. Add colours for any new ground materials in the client's `palette.rs` and
   the preview example.
