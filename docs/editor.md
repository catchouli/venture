# The editor

`venture_editor` is a desktop app for authoring the game: the world, and
the content in `content/` (see [plan-editor.md](plan-editor.md) for where
it's going).

```sh
cargo run -p venture_editor                                  # edits content/, shows world/
cargo run -p venture_editor -- --content content --world world --authoring world_authoring
```

Bake the world first (`cargo run --release -p venture_worldgen --bin bake`),
as for `--local`.

## What it does so far

- **World tab:** the baked world, drawn as in the game, with everything
  placed in it: NPCs (in their real models), spawners (showing their
  monster, with the area monsters appear in drawn on the ground, and the
  distance aggressive ones notice players from), and respawn points.
  - On the left is every placed thing near where the camera is looking
    (**Only within** sets how near, or untick it to list everything): click
    one to choose it and look at it. **Add** puts a new NPC, spawner or
    respawn point where the camera is looking. **Copy** adds a copy two
    metres east, and **Delete** removes it.
  - Click a thing in the view to choose it, and drag it to move it along
    the ground.
  - The chosen thing's every field is on the right. The panel is always
    there (saying nothing's chosen when nothing is) and keeps its width, so
    the view doesn't change size; wide fields scroll sideways.
  - **Walls and fences** (`world_authoring/walls.ron`, baked into the
    world; see [plan-walls.md](plan-walls.md)) are listed too. **Draw**
    (with the kind beside it: dry-stone wall, fence or hedge) starts a new one:
    click points on the ground and it's built as you go; double-click,
    Enter or Escape finishes, and clicking its first point closes it into a
    ring. Click a wall to choose it: its points show as handles. Drag one
    to move it, click the wall between two points to add one there (and
    drag it), or the green + past either end (or **At the start** / **At
    the end** on the right) to add one a metre past it, and Delete removes
    the chosen point. A point added with the fields' **+ Add** starts a
    metre past the end too, rather than on the last point.
    **Split here…** (after choosing a point that isn't an end, and saying
    you're sure) splits the wall in two there, removing that point (a part
    left with one point is too short to be a wall, so it goes); on a ring,
    it opens the ring there. It's one undo. On the right are **New
    seed** (the same wall, its stones laid differently), **Delete wall**,
    and its fields (kind, points, whether it's a ring, height, seed).
    Walls are shown exactly as the bake will make them (the editor bakes
    them itself, against the same world generator), with gaps where roads
    cross.
  - **Buildings** (`world_authoring/buildings.ron`, baked into the world;
    see [plan-buildings.md](plan-buildings.md)) are listed too. **Building**
    in the Add row at the top puts an 8 × 5.5 m one where the camera is
    looking. **Add**
    (with the style beside it) draws a new one: click its corners on the
    ground, each squared up to the last wall, and click the first again
    (or press Enter) to close it; its last corner is found for you. Click
    a building to choose it: drag the handle in its middle to move it (or
    set its **Position**), and on the right are its style, **Edit plan**, **New seed**, turning (15°
    either way, or 90°), **Delete building**, and its fields (floors,
    roof, chimney, and the rest).
  - **Edit plan** hides the building and looks straight down on it. Its
    tools: **Corners** (drag one: the walls either side follow, so every
    corner stays square; drag the middle to move it all; with fewer than
    three corners, click to add one), **Inner walls** (click where one starts, then
    where it ends), **Doors** (click a wall, outer or inner) and
    **Choose** (click a door or inner wall; Delete removes it). **Done**
    (or Escape) leaves it, and the building is made again.
  - Building styles are a tab of their own (`content/buildings/styles.ron`):
    changes show on the buildings at once.
  - **Save and bake** saves, then bakes the world again in the background
    (`bake --walls`: only the chunks walls touch, about a second), so walls
    block the ground they stand on and clear trees from their way. The
    editor then shows the world as baked, and a game or server that's
    running loads it by itself.
  - **Save and play here** saves, then starts the game (`--local`) with a
    new character at the camera's spot. It uses a profile of its own
    ("editor"), with admin powers, so your usual character is left alone.
  - Right drag turns the view, and middle drag (or Shift with right drag)
    slides it over the ground. The scroll wheel goes closer or further.
    W, A, S and D (or the arrow keys) move over the ground; hold Shift to
    go faster.
- **A tab for each kind of content:** classes, abilities, items, skills,
  monsters, spawners, NPCs, appearances, shops, quests, respawn points,
  and what new characters start with.
  - On the left are the files and their entries. **Copy** adds a copy
    just after the chosen entry, with a new id; there's also **Delete**,
    and the arrows move an entry up or down (order matters for classes:
    the first is the default).
  - In the middle is every field of the chosen entry, straight from the
    game's own types, so a new field shows up here by itself. Fields have
    readable names, and hovering one shows what it means (its doc comment).
    Lists can be added to, removed from and reordered; choices have
    dropdowns; colours have a colour picker; dice are written like "2d6+1".
    Names of other things (an NPC's appearance, a class's starting items,
    an item's icon) are dropdowns of what exists, and show in red if what
    they name doesn't.
  - Above the fields: **Used by** lists everything that names this entry,
    and **Rename everywhere** renames it and every reference to it (as one
    undo). Ids saved in players' characters (classes, items, abilities,
    skills, quests) warn that renaming loses them for saved characters.
  - Items with held parts (a sword, a shield) show **How it's held**: a
    person holding it (on any body type, standing or swinging, turned any
    way), and each held part's grip from `characters/characters.ron`: the
    bone it's on, its position (metres along the bone's own directions),
    rotation (degrees) and size. Changes show at once; each item's grip is
    its own, so a staff can be held differently from a sword.
  - Appearances show **How it looks**: a person wearing it, as NPCs with it
    look in the game, updated as it changes.
  - The **Character models** tab edits the rest of `characters.ron`:
    animations, bodies, hair, parts and what players can choose.
  - Classes show what they're like at each level (health, mana, armour,
    attack, damage, points), and monsters how often they hit and how hard,
    worked out by the game's own rules.
  - **Table** shows every entry at once, a row each, with a column for each
    simple field (text, numbers, ticks, plain choices), for changing many
    things together. Click an entry's id to see all of it.
- **Undo and redo** (Ctrl+Z, and Ctrl+Shift+Z or Ctrl+Y; Cmd on a Mac):
  changes to the same file within a second undo together.
- **Problems:** the content is checked a moment after every change, with the
  game's own checks, on a copy of the content folder. Nothing is written to
  `content/` until you save. Click a problem to go to it.
- **Save** (Ctrl+S): writes the changed files. A running game (the server,
  or `--local`) picks them up within a second; if they have a problem, it
  keeps the old content.

Files are written tidily: each entry's fields on their own lines, and fields
at their defaults left out. Only changed files are rewritten. Comments
aren't kept, so notes about an entry go in its `notes` field (see
[content-authoring.md](content-authoring.md)).

## How it's built

- `document.rs`: the content in memory, one list of entries per file, as
  the content crate's own types. It loads, writes, undoes (by keeping each
  changed file's text before and after), and checks.
- `buildings.rs`: the World tab's buildings: drawing (corners squared up,
  the last found), choosing, moving, turning, the plan editor, and the live
  bake (as walls are), handed to the game's building drawing (`ShownBuildings`).
- `walls.rs`: the World tab's walls and fences: drawing, handles, the
  live bake (a `WorldGenerator` built in the background from the world's
  seed, and `venture_worldgen::structures`, as `bake` uses), handing them to
  the game's wall drawing (`ShownWalls`), and running the bake. The walls
  file is a document collection like the content's, but saved to the
  authoring folder (and not part of the content check; walls have checks of
  their own).
- `places.rs`: the World tab's placed things. The document stays the one
  truth; each placed thing has a marker entity kept in step with it (the
  game's own character and monster models, through `looks.rs`, which points
  `content://` at the content folder). Dragging moves only the marker until
  it's let go, then the entry's placement is rewritten, as one undo step.
- `fields.rs`: the property panel, drawn from the types' reflection. Fields
  naming other content are marked in the content crate with
  `#[reflect(@Refers::Item)]` (see `Refers`), which gives them their
  dropdowns and lets renames follow them.
- `previews.rs`: the class and monster previews.
- `grips.rs`: the grip stage: a person on a little stage of their own,
  drawn by its own camera into a picture in the Items tab. Grip changes go
  into the document's character models, which `looks.rs` hands to the
  game's rendering (with a new version, so everyone is dressed again).
- `shell.rs`: the layout (egui): the tabs, the entry list or table, the properties
  (drawn by `bevy-inspector-egui` from the types' reflection), problems, and
  shortcuts. The interface has its own camera over the whole window; the 3D
  view's camera only draws between the panels.
- `camera.rs`: the camera orbiting a point on the ground. The world streams
  in around that point, using the client's world plugin.
- `screenshot.rs`: `VENTURE_SCREENSHOT=path` saves a picture of the window
  once the world has loaded, then exits. `VENTURE_EDITOR_TAB=Classes` and
  `VENTURE_EDITOR_ENTRY=mage` choose what's showing (with
  `VENTURE_EDITOR_TABLE=1` for tables), `VENTURE_EDITOR_PLACE=town_guard`
  chooses a placed thing, and `VENTURE_EDITOR_FOCUS=x,y` and
  `VENTURE_EDITOR_DISTANCE=metres` set the camera. `VENTURE_EDITOR_WALL=id`
  chooses a wall, and `VENTURE_EDITOR_BUILDING=id` a building (in its plan
  with `VENTURE_EDITOR_PLAN=1`).

Tests (`cargo test -p venture_editor`) check that saving everything gives the
game the same content, that awkward text survives, that undo works, that
problems are found before saving, that placed things are added, copied,
and deleted properly, and that walls are drawn, saved to the authoring
folder and checked.
