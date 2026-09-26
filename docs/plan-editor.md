# Plan: the editor (`venture_editor`)

A desktop app for authoring everything the game defines, visually. It starts
by editing what exists today, both what's placed in the world (NPCs,
creature spawners, respawn points) and the rules and things of the game
(classes, abilities, items, skills, creatures). It then grows into the world
edit app of [world-authoring.md](world-authoring.md): props, generator
plug-ins (trees, then buildings), areas, roads and towns.

This is the plan; none of it exists yet.

## Principles

1. **The files stay the truth.** The editor reads and writes the same RON
   files in `content/` that the game loads and authors can edit by hand.
   There's no editor database, and everything stays reviewable and
   mergeable in git.
2. **One schema.** The editor edits the content crate's own definition types
   (`ClassDef`, `CreatureDef`, `NpcDef`...), so it can't disagree with the
   game about what a field means. Adding a field to a definition makes it
   appear in the editor.
3. **Things in the world are entities; their definitions are components.**
   In the editor's 3D view, each placed NPC, spawner and respawn point is a
   Bevy entity carrying its definition as a reflected component. The
   property panel edits those component values; saving turns the
   components back into content files. (These are the *definitions*. The
   game server's own runtime components, such as health, are made from
   them when the content loads.)
4. **What you see is the game.** The editor reuses the client's terrain,
   prop, character and creature rendering, so an NPC looks in the editor
   exactly as in the game.
5. **Live.** Saving writes files, and a running game (the dev server, or
   `--local`) hot-reloads them within a second. It already keeps the last
   good content if a file has a problem. "Play from here" starts the game
   at the editor camera's spot.
6. **Checked as you go.** The content check (`venture_content`'s checks)
   runs after every edit, and problems show in a panel with a link to the
   thing at fault.
7. **Undo from the start.** Every change is a command that can be undone
   and redone. That's cheap to build in at the start and painful to add
   later.

## Choices

- **Its interface is egui** (`bevy_egui` and `bevy-inspector-egui`, both on
  Bevy 0.19), not the game's `bevy_ui`. Tools need tables, trees, text
  fields, dropdowns and docking panels, which egui has ready-made.
  `bevy-inspector-egui` builds property panels from reflected types,
  which is exactly principle 3. The game's own interface is unaffected.
- **Reflection on the content types.** The definition types (and the
  protocol and RPG types inside them, such as `Stats`, `Dice` and
  `EquipSlot`) derive `Reflect` behind a `reflect` feature. Only the
  editor and client turn it on, so the server and the Bevy-free crates
  stay as they are.
- **The client becomes a library plus a thin binary.** Its plugins (world
  rendering, characters, creatures, content assets) become usable from the
  editor. The game itself doesn't change.
- **One app with tabs**, not a separate app per job. A tab (or tool) is a
  plug-in: a Bevy plugin that registers its panels. The generator tools
  later arrive the same way, each wrapping a Bevy-free generator crate.

## The layout

```
┌ World │ Classes │ Abilities │ Items │ Skills │ Creatures │ Looks ────────── [Save] [Play here] ┐
│ Outliner / list    │  3D view (World tab)                              │ Properties (inspector) │
│  ▸ NPCs (4)        │  or a table of every entry (other tabs)           │  name   [Guard Aldren] │
│  ▸ Spawners (8)    │                                                   │  appearance [guard  ▾] │
│  ▸ Respawn points  │                                                   │  at  [x 11.0][y 1.0]   │
├────────────────────┴───────────────────────────────────────────────────┴────────────────────────┤
│ Problems: item "wooden_staff": there's no outfit part called "oak_staff"          (click to go) │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Milestones

### 0. Foundations

The editor opens, shows the world, and can save content without losing
anything.

- Split `venture_client` into a library and a thin binary.
- The `venture_editor` crate: a window, egui, the tab shell (with the
  plug-in registry), and a flying/orbiting camera over the baked world
  (terrain chunks stream in around the camera as in the game).
- **Content in and out.** Load the content pack, remembering which file
  each entry came from. Save entries back to their own files as tidy RON
  (fields at their default left out). A round-trip test checks that
  loading then saving then loading the real content gives the same pack.
- Commands with undo and redo, unsaved-changes markers, Ctrl+S, and the
  problems panel.
- `Reflect` on the content types (the `reflect` feature).

### 1. The World tab: placing things

- **Outliner** of NPCs, spawners and respawn points, with search. Clicking
  one flies the camera to it.
- **In the 3D view:** NPCs as their real models; spawners as their
  creature, with the spawn radius drawn on the ground (and the aggro radius
  for aggressive ones); respawn points and settlement centres as markers.
- **Tools:** select (click), move (drag along the ground; it follows the
  terrain), duplicate, delete, and add from a palette. Placements relative to
  a settlement stay relative when moved; "make absolute" converts one to a
  world position.
- **Properties:** every field, with references (appearance, creature,
  script, dialogue, shop) as dropdowns of what exists, and "open" buttons to
  jump to them.
- **Table view:** all NPCs or all spawners in a spreadsheet, for editing
  many at once.
- **Live:** saving updates a running game; "Play from here" starts
  `--local` at the camera's spot.

### 2. Rules tabs: classes, abilities, items, skills

Each tab is a table of entries plus the property panel, with add,
duplicate, delete and "where is this used?".

- **Classes:** name, description, stats, hit die, mana, casting stat,
  starting equipment and abilities, and the abilities learned later (level
  and cost) as an editable table. A new field, `starting`, says whether the
  class is offered at character creation (so later classes can be reached by
  advancement instead). A **preview** shows health, mana, attack and armour
  at each level, worked out by `venture_rpg`, the same maths the server
  uses.
- **Abilities**, **items** (slots, weapon, armour, outfit parts, icon) and
  **skills**, and what new characters start with.
- **Renaming an id** is careful: ids are saved in players' profiles (the
  items they carry, the abilities they know). The editor warns, and renames
  every reference in the content. Profiles can be migrated separately if
  it's ever needed.

### 3. Creatures, with real models

- A **creature model registry**, like `characters.ron`: models, animation
  clips, scale and tint. The client draws creatures from it (the
  placeholder shapes stay as the fallback).
- Import what fits from `~/assets` (the Bestiary kit has the Imp and the
  Puglin; whether they share the humanoid skeleton, and so its animations,
  needs checking) and give goblins and rabbits models.
- **The Creatures tab:** stats (level, health, armour class, attack, speed,
  experience), behaviour (aggressive, radii), and a **model picker** with a
  turning preview. A preview shows what an attack does against a level N
  character, from `venture_rpg`.

### 4. Looks

- **Appearances** (NPC looks) with the character creator's preview: body,
  hair, beard, sliders, colours, outfit parts.
- **Items' outfit parts** tried on a mannequin, and **held things** posed
  live: adjust a sword's bone, position and rotation while the model plays
  its idle and attack.

### 5. Props

A new kind of content: **placed objects** (crates, barrels, fences, stalls,
from the Medieval Village and Fantasy Props kits), put down in the World tab
like NPCs. At first they're only for looks. Blocking movement means adding
them to the navigation, either baked (so re-baking chunks) or as the
server's own blockers; that's decided when we get there.

### After that

The rest of [world-authoring.md](world-authoring.md), each as a plug-in:
- generators (trees first) as tools with their own panels;
- areas (forests, clearings) painted on the map;
- roads and paths;
- buildings and towns.

Shops, quests and dialogue can get tabs too (Yarn has its own editors, so
dialogue may just open the file).

## Open questions

- **Comments in content files.** Saving from the editor rewrites a file,
  so hand-written comments in it would be lost. The suggestion: explanations
  of the format move into the docs (most are there already), and notes
  about particular entries become a `notes` field that the game ignores.
  Keeping comments in place is possible, by editing only the changed
  entries' text, but much more work.
- **Props:** only for looks at first, or blocking from the start?
- **Where the editor runs:** desktop only (like `--local`). Editing a
  shared server's content remotely can come much later, if ever; content
  goes through git.
