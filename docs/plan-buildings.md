# Plan: buildings

Buildings are what settlements are made of. The author decides what matters:
- where a building is, and its outline;
- its floor plan: rooms, and where the doors go;
- its style, how many floors it has, and a seed.

A generator makes the rest: walls, openings, floors, roof, chimney, and
all the small variations that stop one cottage looking like the next.

This first pass is for simple villages: small one- or two-storey buildings
in two styles. A full architecture system comes later. The framework
should grow into that without starting again:
- more styles (wooden huts up to grand stone halls);
- more roof shapes;
- joined buildings;
- interiors.

It works the same way as walls and fences (see [plan-walls.md](plan-walls.md)):
- **authored data** in `world_authoring/buildings.ron`;
- **baked** into the world: the manifest, blocked cells, cleared trees;
- **built into meshes** by a standalone generator on each client, and live in
  the editor.

## What the author draws

A building, in the editor:
1. **Add building**, then click the corners of its outline on the ground. It
   closes when you click the first corner again. A building is generated at
   once, with no rooms and a door on its longest side.
2. **Properties** (the panel on the right):
   - style (a dropdown);
   - floors (1 by default);
   - seed, with a **New seed** button;
   - roof shape;
   - chimney (the style's usual, yes or no).
3. **Edit plan** switches the building to its floor plan. Its generated
   meshes hide, and the camera looks down on it. Here you can:
   - drag corners, and add them on the outline;
   - draw inner walls (click to click, snapping to walls and to right angles);
   - place doors: click a wall, outer or inner, and drag the door along it;
   - delete what's chosen.

   **Done** (or Escape) leaves edit mode and generates the building again.
   (It also regenerates as you edit, but the mode keeps the view clear.)
4. **Move and turn** a whole building by dragging it, or with its turn handle.

As data:

```ron
(
    id: "oakwengate_cottage_1",
    style: "stone_cottage",
    floors: 1,
    seed: 4012,
    roof: Gable,                 // or Hip
    chimney: None,               // None: the style's usual; Some(true / false)
    outline: [(2310.0, 2150.0), (2318.0, 2150.0), (2318.0, 2156.0), (2310.0, 2156.0)],
    inner_walls: [((2314.0, 2150.0), (2314.0, 2156.0))],
    doors: [
        (wall: Outer(0), along: 0.5),        // the first outer edge, halfway
        (wall: Inner(0), along: 0.3),        // the first inner wall
    ],
)
```

- Doors are placed along a wall (0 to 1), not at a point, so they stay on
  their wall as corners move.
- Windows aren't drawn: the style places them along the outer walls. It
  keeps clear of corners, doors and inner walls, spaces them by its rules,
  and the seed varies them. Authored windows can come later.

### Footprints: right angles first

The first version only takes outlines whose corners are right angles
(rectangles, L, T and U shapes), at any rotation. The editor snaps to them.

**Why:** cottages, barns and longhouses are almost all like this. It also
makes the hard parts reliable:
- roofs are made over rectangles;
- windows are placed along straight runs;
- floor plans snap cleanly.

Free-angled outlines (round towers, odd-shaped plots) come later. They need
a general roof generator (a straight skeleton), which is fiddly to make
robust.

## Styles are data

A style is content (`content/buildings/styles.ron`), not code, so new ones
are mostly writing data. What the style sets:
- **Walls:** how they're built, one of a few *constructions* the generator
  knows:
  - `DryStone` (coursed stones, like the walls, but mortared-looking);
  - `Timber` (horizontal planks or logs with corner posts);
  - later `TimberFrame` (posts and braces with plaster between), `Brick`,
    `Plaster`.
  - Also how thick the walls are, and their texture.
- **Storeys:** the height of each floor, and the plinth (a stone base, so
  buildings sit level on sloping ground).
- **Roof:** its construction (`Thatch` for both styles now; later slate,
  shingles), pitch (with random variation), eaves overhang, thickness, and
  the ridge's finish.
- **Openings:** door and window sizes, how far apart windows go, their
  frames, lintels and sills (stone or wood), and shutters.
- **Chimney:** how likely one is, what it's made of, and where it goes (a
  gable end, or on the ridge).
- **Variation:** how much the pitch, colours, wall irregularity and window
  placement vary, all from the seed.

Two styles to begin with:
- **Stone cottage:**
  - walls of coursed rubble stone (the Rock051 texture again, or a warmer
    one), 0.5 m thick;
  - stone lintels and sills;
  - thatched roof, about 50°, with deep eaves;
  - a stone chimney on a gable end, most of the time.
- **Wooden cottage:**
  - rustic timber walls (horizontal boards over a frame, one of the
    weathered woods already in `~/assets/structures`);
  - corner posts, and wooden frames and shutters;
  - thatched roof;
  - a chimney only sometimes, and of stone when there is one.

## The generator

It goes in `venture_structures` (new modules beside the walls), standalone
like the tree generator, with no Bevy. Given a building and its style, it
works in stages:

1. **Plan:** checks the outline (right angles, no crossing edges, sensible
   sizes), then splits it into **wings** (rectangles: an L is two, a T or U
   three) for the roof. It snaps inner walls and doors onto their walls.
2. **Walls:** each outer edge and inner wall becomes a run of wall, storey
   by storey.
   - Openings are cut from it (doors where drawn; windows where the style
     puts them), and the wall is built around them by its construction:
     stones in courses, stepping round openings, with quoins at the corners;
     or boards with posts.
   - Gable ends go up under the roof.
3. **Openings:** frames, lintels and sills, doors (planked, a little ajar or
   shut), and windows (dark glass or shutters, which is cheaper and fits
   the period).
4. **Floors:** a floor per storey, and the plinth under the whole footprint
   down to the lowest ground.
5. **Roof:** over each wing, a gable (ridge along the long side, gable walls
   at the ends) or a hip.
   - Where wings meet, their roofs intersect and make the valleys.
   - Thatch is thick, so the roof is a slab with rounded edges at the eaves
     and ridge, not a flat sheet.
6. **Chimney:** if the style (and seed) want one: a stack on a gable end or
   through the ridge, above the roof, with a pot or two.
7. **Variation:** the seed nudges everything within the style's ranges:
   - pitch, eave depth and ridge sag;
   - stone and board colours;
   - window count and spacing;
   - door colour;
   - how weathered the walls look.

Meshes come in the same shape as the walls': a full one and a far one per
building. The far one is the walls as flat boxes and the roof as plain
slabs, with no openings except dark window patches. The materials are:
- wall, by construction;
- roof, by construction;
- wood (frames, doors, shutters);
- dark (window glass and the dark inside of openings).

**A tool to see them without the game:** `cargo run -p venture_structures
--example building -- <file.ron> --out building.glb --preview building.png`,
using the tree tool's CPU preview renderer. This is for tuning styles
quickly, as the tree tool was.

## Baking and the world

As for walls:
- The bake reads `world_authoring/buildings.ron` and writes the manifest's
  `buildings`, with the ground's height under each corner (for the
  plinth's depth).
- **Blocked cells:** cells under walls are blocked, except doorways. Inside
  is walkable (on the ground floor), so you can go in.
- **Cleared:** trees and rocks inside and near the footprint are cleared.
- `bake --walls` becomes `bake --structures`, covering both. A running game
  loads it by itself, as walls do now.
- **The ground stays as it is:** the plinth takes up the slope. The floor is
  at the footprint's highest ground plus a step. (Flattening the ground
  under a building can come later, with a polygon flatten stamp.)

## In the game

- **Clients** build the meshes from the manifest in the background,
  building by building (as walls are). The far mesh is drawn past about
  80 m, and nothing past about 500 m.
- **Going inside:**
  - While the player is inside a building, its roof and upper floors fade
    out, as trees near the camera do, so you can see the room.
  - The server knows which building a position is in from the manifest, and
    clients work it out the same way.
  - Upper floors are for show at first. Stairs, and walking on upper floors,
    need navigation on more than one level, which comes later.
- **The map** draws each building's footprint in a roof colour.

## Steps

1. The plan (this), and textures:
   - ThatchedRoof001A (downloaded to `~/assets/structures`);
   - the stone and wood are already in the project.
2. **The generator, first pass:** data types, plan checks, wings, stone and
   wooden walls with doors and windows, floors, gable and hip roofs of
   thatch, chimneys, variation. Also the preview tool, and tests (closed
   meshes, openings where they should be, the same seed giving the same
   building).
3. **Baking:** `world_authoring/buildings.ron`, the manifest's buildings,
   blocked cells with doorways open, cleared props, and `bake --structures`.
4. **In the game:** meshes, levels of detail, materials, the roof fading
   when you're inside, and buildings on the map.
5. **The editor:**
   - add a building by drawing its outline;
   - properties (style dropdown, floors, seed, roof, chimney);
   - moving and turning it;
   - edit mode: corners, inner walls, doors; **Done** regenerates.
6. **A village:** a handful of cottages of both styles in Oakwengate.
7. **Later:**
   - more styles and constructions (timber frame, brick, slate, shingles);
   - free-angled outlines (straight-skeleton roofs);
   - authored windows;
   - stairs and upper floors you can walk on;
   - furniture;
   - joined buildings (terraces);
   - walls meeting buildings;
   - ground flattened under buildings;
   - building templates (drop a "cottage" and adjust it).

## Status

- [x] 1. The plan
- [x] 2. The generator: `crates/structures/src/buildings/`: `plan.rs` (the
  frame, checks, wings, openings), `faces.rs` (walls: stone courses and
  quoins, or weatherboards; the plinth; plaster inside), `details.rs`
  (reveals, lintels and sills or frames, windows with bars and shutters,
  doors standing open inward, floors, inner walls), `roof.rs` (gable or hip
  thatch over each wing, reaching into its neighbours so they make
  valleys; rolled ridges and hips; gable walls; chimney with pots),
  `bake.rs` (floor level, levelled ground, blocked cells, clearing). A
  cottage is 20 to 40 thousand triangles close up, a tenth of that far away.
  No preview tool yet: the game and editor have been enough.
- [x] 3. Baking: `world_authoring/buildings.ron`; the ground under a
  building is levelled to its floor (the average ground under it),
  blending back into the land over 2.5 m, and turned to trodden earth; its
  walls block cells except its doorways; trees and rocks within 2 m go.
  `bake --structures` (was `--walls`) bakes only the chunks walls and
  buildings touch.
- [x] 4. In the game: `crates/client/src/world/buildings.rs` (built in the
  background, full detail to 80 m then far to 600 m; a material a surface;
  the roof and floors above hidden while you're inside);
  `world/surfaces.rs` (textures shared with walls). Textures: Rock051
  (stone), Wood035 (weatherboards), Wood059 (woodwork, floors),
  ThatchedRoof001A (thatch). Buildings are on the map, in thatch colour.
- [x] 5. The editor (see [editor.md](editor.md)): drawing outlines with
  right angles, choosing, moving and turning, style, New seed, and the
  plan editor (corners, inner walls, doors, deleting). Styles are a tab of
  their own. Each style has a `scale` multiplying all its sizes.
- [ ] 6. A village: four test cottages so far; next, homes for the NPCs, a big stone smithy and a fishing guild, in a bigger ring of wall
- [ ] 7. Later
