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

- **World tab:** the baked world, drawn as in the game.
  - Right drag turns the view, and middle drag (or Shift with right drag)
    slides it over the ground.
  - The scroll wheel goes closer or further.
  - W, A, S and D (or the arrow keys) move over the ground; hold Shift to
    go faster.
- **A tab for each kind of content:** classes, abilities, items, skills,
  creatures, spawners, NPCs, appearances, shops, quests, respawn points,
  and what new characters start with.
  - On the left are the files and their entries. **Copy** adds a copy
    just after the chosen entry, with a new id; there's also **Delete**,
    and the arrows move an entry up or down (order matters for classes:
    the first is the default).
  - In the middle is every field of the chosen entry, straight from the
    game's own types, so a new field shows up here by itself.
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
- `shell.rs`: the layout (egui): the tabs, the entry list, the properties
  (drawn by `bevy-inspector-egui` from the types' reflection), problems, and
  shortcuts. The interface has its own camera over the whole window; the 3D
  view's camera only draws between the panels.
- `camera.rs`: the camera orbiting a point on the ground. The world streams
  in around that point, using the client's world plugin.
- `screenshot.rs`: `VENTURE_SCREENSHOT=path` saves a picture of the window
  once the world has loaded, then exits. `VENTURE_EDITOR_TAB=Classes` and
  `VENTURE_EDITOR_ENTRY=mage` choose what's showing, and
  `VENTURE_EDITOR_FOCUS=x,y` where the camera looks.

Tests (`cargo test -p venture_editor`) check that saving everything gives the
game the same content, that awkward text survives, that undo works, and that
problems are found before saving.
