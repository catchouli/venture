# World authoring: generated terrain, authored places

How the world gets made. **The generator makes the land. Authors decide what
goes on it.** The generator lays out terrain, rivers, biomes and settlement
sites from a seed. A **world edit app** then lets a person put things on top
of that land:
- NPCs, creature spawners and other content;
- individual trees and forests, and clearings;
- paths and roads;
- buildings, blocked out as floorplans.

Trees and buildings are made by **object generators**: standalone,
parameterised generators that know nothing about the world, so any of them
could also be its own little app. Everything that shapes the world
physically is baked into the world files at bake time.

The app is being built first as a general editor for all content, starting
with what exists today: see [plan-editor.md](plan-editor.md).

This is the plan; little of it exists yet. Today's pieces are the overlay
stamps (see [world-generation-engine.md](world-generation-engine.md)), the
generator's own prop scattering and roads, and content placed relative to
settlements (see [content-authoring.md](content-authoring.md)).

## The layers

```
seed + WorldGenConfig ──► generated world: terrain, biomes, rivers, lakes, settlement sites
                              │
authored world files  ───────►│  (written by the world edit app)
  roads and paths             │    paint, flatten, clear props along a spline
  forests, clearings, trees   │    where trees go, and which ones
  buildings and structures    │    floorplan + style, made by the building generator
  terrain stamps              │    flatten, raise, carve, paint (the existing overlays)
                              ▼
                      bake (--bin bake) ──► world/ (chunks, props, structures, navigation)

content pack  ─── NPCs, spawners, respawn points, shops, quests...  (live: hot-reloads)
```

There are two kinds of authored data, split by whether they change the
physical world:
- **Baked world data** (roads, trees, buildings, terrain stamps) changes the
  ground, what blocks movement, and what the client draws. It lives in world
  authoring files next to the generator config. The bake turns it into chunk
  files, and changing it means re-baking the affected chunks.
- **Live content** (NPCs, spawners, respawn points) is what the content pack
  already holds. It stays there, keeps hot reloading on a running server, and
  never needs a bake. The edit app just places it, writing the same content
  files an author could write by hand.

Everything is data. The edit app is only a convenient way to write it:
every authored file stays readable RON that can be edited, reviewed and
merged in git.

## The world edit app

A separate desktop app (`venture_editor`, a Bevy binary), not part of the
game client. It reuses the client's terrain, prop and structure rendering,
so what the author sees is what players see.

- **Views:** a top-down map for layout, and the 3D game view for checking
  the look.
- **Tools:**
  - **Place:** NPCs, spawners (showing their radius), respawn points, single
    trees and rocks, and structures.
  - **Paint areas:** forests (density and species mix), clearings where
    nothing is scattered, ground materials, and biome overrides.
  - **Draw paths and roads:** splines with a width, a surface (dirt path,
    cobbled road) and an edge falloff. A road flattens and paints the ground
    along it and clears trees out of its way. Roads through forests are
    simply drawn through them.
  - **Block out buildings:** draw a floorplan, set properties, and see the
    generated building at once (see below).
  - **Terrain stamps:** flatten, raise, carve and paint, as the overlays
    already do.
- **Live preview:** generation is deterministic and works chunk by chunk, so
  after each edit the app regenerates only the affected chunks.
- **Save:** writes the authored world files and content files. The bake
  reads the same files, so the app and the game can never disagree.
- **Generated content stays overridable.** The generator's own roads between
  settlements and its scattered trees are a starting point. An author can
  delete a generated road, replace it with a drawn one, or clear and replant
  a forest. Authored data always wins over generated data.

The app doesn't talk to a running server at first. Later it could push live
content (like moving an NPC) to a dev server through hot reload, since
content files are already watched.

## Object generators

Each kind of generated object (trees first, then rocks, buildings, bridges,
fences and so on) has its own **generator**:

- **Isolated:** its own crate (for example `venture_trees`,
  `venture_buildings`), depending on nothing but plain maths and
  mesh-data types. It knows nothing of the world, the generator or Bevy, like
  `venture_world` and `venture_worldgen`.
- **Parameterised and deterministic:** the same parameters and seed always
  give the same object.
- **Output:** plain mesh data (positions, normals, colours or material
  slots), plus the facts the world needs: its footprint (what it blocks) and,
  for structures, walkable floors, stairs and doors for the navigation grid.
- **Usable on its own:** each generator comes with a small example or app
  that makes objects from parameters and writes them out (for example to
  glTF), with a gallery view. So "a tree generator app" is just a front end
  on the generator crate.
- **Shared by everything:** the worldgen's own scattering (a pine forest in
  the highlands) and the edit app's hand-placed trees call the same
  generator. They just choose parameters differently.

### Trees

Parameters, for example:
- **species:** oak, birch, pine, willow, elven silverbark... Each species is
  data: branching pattern, leaf shape and colour, bark colour, proportions;
- **age:** sapling to ancient, which drives height, trunk girth and crown size;
- **size:** height within what the species and age allow;
- **health or season:** full, sparse, dead, autumn;
- **seed:** for variation within all of that.

A forest area just picks these per tree (species mix, age spread) from the
area's settings. That's also where views get better: an author can keep a
path's surroundings open, put tall old trees deep in a forest, and keep
towns clear, instead of relying on random scatter.

### Buildings

The author blocks out a **floorplan** on a grid: rooms, walls, doors,
windows, stairs. Then they set **properties**:
- number of floors;
- roof type (gable, hip, flat, tower cone);
- **architectural style:** human (timber frame and plaster, thatch or
  slate), elven (ornate, curved, pale stone and living wood), dwarven (heavy
  stone, carved), and so on. A style is data: wall, roof, trim and window
  parts, materials and proportions;
- condition (new, weathered, ruined) and a seed for variation.

The building generator turns that into walls, floors, roofs, doors, windows,
stairs and trim. It outputs the mesh and the navigation facts (floors to
walk on, stairs between them, walls that block). Interiors work because the
world model already plans for a multi-layer navigation grid (see
[world-model.md](world-model.md)). Bridges, walls and towers can be more
generators in the same mould.

## Baking

The bake reads the seed and config, the authored world files and the
generators, and writes:
- the terrain chunks as today, with roads, stamps and paint applied;
- **props and structures by parameters** (species, age, seed, floorplan
  reference and style), not as meshes;
- the navigation data, with every footprint, floor and stair already worked
  out.

The client runs the same generators to build meshes from those parameters,
which keeps the world download small. Only the look depends on the client's
meshes, never what blocks movement, since that's baked into the navigation
data. So tiny floating-point differences between web and desktop builds
can't matter for gameplay.

Instead of storing parameters, the bake could store finished meshes (which
costs download size), or the client could cache meshes it has built. That
can be decided once the generators exist and their cost can be measured.

## Order of work (when we get to it)

1. Authored world files for **roads and paths** and **forests and clearings**,
   read by the bake. They're data only, no app yet.
2. The **tree generator** as its own crate, with a gallery example that
   writes models. The worldgen's scatter then switches over to it.
3. The **world edit app:** a map view and the place/paint/road tools, with
   live chunk regeneration.
4. Live content placement in the app (NPCs, spawners, respawn points).
5. The **building generator:** floorplans, styles, floors and roofs, with the
   multi-layer navigation grid.
