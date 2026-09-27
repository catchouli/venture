# The editor

`venture_editor` is a desktop app for authoring the game: the world, and
the content in `content/` (see [plan-editor.md](plan-editor.md) for where
it's going).

```sh
cargo run -p venture_editor                                  # edits content/, shows world/
cargo run -p venture_editor -- --content content --world world
```

Bake the world first (`cargo run --release -p venture_worldgen --bin bake`),
as for `--local`.

## What it does so far

- **World tab:** the baked world, drawn as in the game, with everything
  placed in it: NPCs (in their real models), spawners (showing their
  creature, with the area creatures appear in drawn on the ground, and the
  distance aggressive ones notice players from), and respawn points.
  - On the left is every placed thing near where the camera is looking
    (**Only within** sets how near, or untick it to list everything): click
    one to choose it and look at it. **Add** puts a new NPC, spawner or
    respawn point where the camera is looking. **Copy** adds a copy two
    metres east, and **Delete** removes it.
  - Click a thing in the view to choose it, and drag it to move it along
    the ground.
  - The chosen thing's every field is on the right.
  - **Save and play here** saves, then starts the game (`--local`) with a
    new character at the camera's spot. It uses a profile of its own
    ("editor"), with admin powers, so your usual character is left alone.
  - Right drag turns the view, and middle drag (or Shift with right drag)
    slides it over the ground. The scroll wheel goes closer or further.
    W, A, S and D (or the arrow keys) move over the ground; hold Shift to
    go faster.
- **A tab for each kind of content:** classes, abilities, items, skills,
  creatures, spawners, NPCs, appearances, shops, quests, respawn points,
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
    attack, damage, points), and creatures how often they hit and how hard,
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
- `places.rs`: the World tab's placed things. The document stays the one
  truth; each placed thing has a marker entity kept in step with it (the
  game's own character and creature models, through `looks.rs`, which points
  `content://` at the content folder). Dragging moves only the marker until
  it's let go, then the entry's placement is rewritten, as one undo step.
- `fields.rs`: the property panel, drawn from the types' reflection. Fields
  naming other content are marked in the content crate with
  `#[reflect(@Refers::Item)]` (see `Refers`), which gives them their
  dropdowns and lets renames follow them.
- `previews.rs`: the class and creature previews.
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
  `VENTURE_EDITOR_DISTANCE=metres` set the camera.

Tests (`cargo test -p venture_editor`) check that saving everything gives the
game the same content, that awkward text survives, that undo works, that
problems are found before saving, and that placed things are added, copied,
and deleted properly.
