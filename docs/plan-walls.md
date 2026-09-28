# Walls and fences: the implementation plan

Dry stone walls and wooden fences, drawn in the editor and baked into the
world: the first **authored world data** (see
[world-authoring.md](world-authoring.md)), and the start of the tools for
building towns and villages. The first use is walling the starting village.

## What they are

- **Dry stone wall:** flat stones laid without mortar. There are two faces,
  each leaning slightly inwards (the *batter*: wide at the foot, narrower at
  the top), the stones laid in rough courses and overlapping the joints
  below, and a row of upright *coping* stones along the top. They're about
  1.2 to 1.4 m tall, 0.7 m wide at the foot and 0.4 m at the top. They're
  grey, weathered, and a little irregular.
- **Fence:** English post-and-rail. Split posts every 2.5 m or so, driven into
  the ground, with two or three rails between them following the slope. Plain
  weathered wood.

Other kinds (hedges, palisades, town walls) can follow as more choices in
the same dropdown.

## How a wall is described

A wall is a **path** (points on the ground, the curve passing through
them), a **kind**, a **height** and a **seed** (every generated thing has
one). In `world_authoring/walls.ron`:

```ron
[
    (
        id: "oakwengate_wall",
        kind: DryStone,           // or Fence
        points: [(2290.0, 2130.0), (2370.0, 2135.0), ...],
        closed: true,             // back round to the first point
        height: 1.3,
        seed: 7,
    ),
]
```

The curve is a **centripetal Catmull–Rom spline** (Yuksel, Schaefer and
Keyser, 2011). It passes through every point, curves smoothly, and doesn't
loop or overshoot at tight bends. It's sampled evenly along its length
(every half metre) for everything else.

## Baked, not live

Walls are static, so they're baked into the world like the terrain. The
bake does three things with each wall:
1. **Where it stands:** its samples, and the ground's height at each. These
   go in the manifest's `walls` (a few numbers a metre), so clients build
   its meshes without needing the terrain loaded.
2. **What it blocks:** the cells under it are blocked (the chunk's
   `structure_cells`, chunk format 4). So navigation, the server and the
   client's prediction all treat it as solid, like water or cliffs.
3. **What it clears:** trees, bushes and rocks on its line are removed.
   Where it crosses a road or path, it leaves a gap (a gateway), so roads
   stay open.

Clients build the meshes from the samples with the generator. That's the
same as trees: tiny downloads, and only the look depends on the client.

**Editing:** the editor builds walls straight from its own document, so
drawing is instant. To see them in the game, save and rebake (about 5
seconds today); the server loads the new world when it starts. Later, the
bake can redo only the chunks a change touches, and a running server could
reload them.

## The generator

A new crate, `venture_structures`, standalone like `venture_trees` (no
Bevy, no world). It does these jobs:
- **Paths:** the spline, even sampling, and the direction at each sample.
- **Dry stone walls:**
  - individual stones, low-poly rounded blocks of random length, height and
    depth, laid course by course up both faces along the path;
  - each course's joints offset from the one below, each stone tilted and
    nudged a little;
  - the faces' batter, then coping stones standing on edge on top;
  - stones stepping with the ground as it rises and falls.
- **Fences:** posts at even spacing (sunk into the ground), rails from
  post to post following the ground, split timber with bevelled ends.
- **Detail:** close up, every stone. Further away, each stretch becomes a
  plain extruded wall (the same profile, stone textured), with Bevy's
  `VisibilityRange` crossfading between them, as trees do. A long village
  wall in full detail would be too heavy to draw everywhere.

Meshes come in stretches of about 16 m, so each can switch its level of
detail on its own and be culled.

**Textures:** ambientCG's Rock051 for the stones, and a weathered wood for
fences (CC0). They're downloaded to `~/assets/structures` and imported into
`content/textures/drystone/` and `fence_wood/` (via LFS), and recorded in `licenses/` and
ATTRIBUTION.md.

## The editor

In the World tab:
- **Add wall:** click points on the ground to draw it, and double-click (or
  Enter) to finish; the wall generates as you go.
- **Edit:** each point is a handle to drag. Click the line between two
  points to add one there, and Delete removes the chosen one. Kind (a
  dropdown: dry stone wall or fence), height, whether it's closed, and the
  seed (with a reroll button) are in the properties panel.
- **Save** writes `world_authoring/walls.ron`, and **Bake** rebakes the world.

## Steps

1. The plan (this).
2. The generator crate: paths, dry stone walls, fences, far detail; textures.
3. Authored data and the bake: `world_authoring/walls.ron`, the manifest's
   walls with their samples, blocked cells, props cleared, gaps at roads.
4. In the game: meshes from the manifest, with levels of detail; the editor
   shows them too.
5. A dry stone wall round the starting village, and a fenced field.
6. The editor's wall tools (draw, edit, properties, save, bake).
7. Later: gates, stiles, hedges, walls meeting buildings, rebaking only the
   chunks a change touches.

## Status

- [x] 1. The plan
- [x] 2. The generator: `crates/structures` (`path.rs`, `walls.rs`,
  `mesh.rs`). A dry stone wall is roughly 500 triangles a metre close
  up and a twentieth of that far away; fences are a few boxes a post.
- [x] 3. Authored data and the bake: `world_authoring/walls.ron` is read by
  `bake`, which writes the manifest's `walls`, blocks each chunk's
  `structure_cells` (the wall's half-width: 0.4 m for dry stone, 0.15 m for
  fences), clears trees and rocks within 1.5 m more, and opens gaps where a
  road comes within its half-width plus 0.5 m (widened by 1 m either side).
- [x] 4. In the game: `crates/client/src/world/walls.rs` builds every wall in
  the background when the world arrives (the village's two take 4 ms: 17
  stretches, 112k triangles). Dry stone is every stone to 60 m, crossfading
  to its band, which is drawn to 450 m; fences look the same at any
  distance. Plain `StandardMaterial`s with the imported textures (mipmaps
  added on load) and tangents for the normal maps. The editor shows them
  too, through the world plugin. Not on the map yet.
- [x] 5. The village wall: a dry stone ring round Oakwengate's square
  (about 28 m out), open where the north–south road passes, and a fenced
  field to its south-west with a gateway facing the village.
- [x] 6. The editor's tools: draw, choose, drag points, add points on the
  line, Delete, New seed, Delete wall, fields, Save and bake (see
  [editor.md](editor.md)). The editor bakes walls live with
  `venture_worldgen::structures::bake_authored_wall`, the same function
  `bake` uses, and rebuilds only the walls that changed. Saving rewrites
  `walls.ron` tidily, so its comments go.
- [ ] 7. Later: gates, stiles, hedges, walls meeting buildings, walls on the
  map, rebaking only the chunks a change touches.
