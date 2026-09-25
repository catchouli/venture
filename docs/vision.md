# Vision

> This is a living document. The pitch is still developing, and for now the
> priority is a **vertical slice**: a small part of the world that looks and
> feels right.

## The game in a sentence

A medieval fantasy MMORPG with the lush, hazy, walkable world of **Lineage 2**,
the variety and depth of **Old School RuneScape**'s skills, and a world built
by a deterministic generator, then shaped and filled by hand.

## Look and feel

- **Fidelity: close to Lineage 2**, not blocky. Smooth rolling terrain with
  detail at every scale: hills and valleys you can see for kilometres, and
  bumps, dips and mounds you notice as you walk. The ground is richly
  textured and blends between materials (worn dirt paths through grass, sand
  fading into turf).
- **Atmosphere matters most.** A warm, hazy light; long afternoon shadows;
  woods you can get lost in; standing stones and boulders; mountains fading
  into the mist.
- **Camera:** third person, close behind and above the character, freely
  rotated and zoomed.
- **Style:** stylised and slightly painterly rather than photoreal. Simple
  shapes, rich colour and lighting. This keeps it achievable for a small team
  and fast in a browser.

## Moving around

- **Click to move.** Clicking the ground drops a glowing marker (as in
  Lineage 2) and the character runs there, finding a path around obstacles.
  The marker disappears when the character arrives. (Working in the prototype.)
- **Limited reach:** one click sends the character at most a set distance
  (150 m to start, set in the server's `server.ron`). Crossing the world takes
  active play, not a single click.
- **Cursor:** a custom ornate pointer. It changes to show what a click will
  do: a pointer with a trailing arrow to follow a selected player, and a
  sword to attack a selected creature.

## Chat

- A chat box in the bottom-left corner shows recent messages; press Enter
  (or click it) to type, and Enter again to send.
- Messages starting with `/` are commands: `/town` takes you back to the
  starting town, and `/help` lists the others.
- What nearby players say also appears in **speech bubbles** above their
  heads. Up to three recent messages stack up, newest nearest the name, and
  each disappears after a few seconds. Long messages are shortened in the
  bubble; the chat box has the full text.

## Targeting

- **Selecting:** clicking a character selects it: players now, NPCs and
  creatures later. The selected character's name gets a glowing orb on each
  side.
- **Target frame:** a box at the top middle of the screen shows the selected
  target's name. For creatures it also shows a health bar. NPCs can't be
  killed, and other players' health stays private even once PvP exists, so
  neither shows one. An ✕ in the frame's corner deselects the target and
  hides the frame. Escape does the same.
- **Talking:** clicking the selected NPC again walks over and talks to them
  (the cursor shows a speech bubble while hovering over them).
- **Following:** clicking the selected player again follows them (the
  cursor shows the follow arrow while hovering over them). The character
  keeps close behind until you move, steer, stop, deselect, or they get out
  of reach.
- **Attacking** (later): clicking a selected creature again attacks it, with
  the sword cursor.
- **Free movement.** Characters are *not* locked to tiles as in RuneScape.
  Positions are continuous, in metres. The terrain grid is only how the
  ground is stored.
- Later: optional keyboard (WASD) movement, which also suits controllers and mobile.

## A world of distinct lands

The world is divided into regions, and each biome is home to one of its peoples:

| Biome | Land | People |
| --- | --- | --- |
| Grassland | Open plains and wide river valleys, farms and market towns | Humans |
| Meadow | Rolling, enchanted meadows of flowers, groves and ponds | Elves |
| Highlands | Rocky mountains, gorges, mines and deep caves | Dwarves |
| Volcanic | Ash plains, black rock and lava around smoking volcanoes | To be decided (orcs? a fire-touched people?) |

More biomes can be added later (such as frozen north, swamp, desert, dark
forest) without changing the engine. See [world-generation-engine.md](world-generation-engine.md).

Settlements sit on flat land near rivers and are joined by roads. The
generator places them, and designers turn each one into a real town or
village using the overlay system.

## Skills

A full set of skills for gathering, making and doing things is central to the
game. The exact list needs careful design. We want our own system that suits
this world, not a copy of RuneScape's.

### Principles

1. **Everything connects.** Every skill feeds at least one other and the
   player economy. Gathering feeds making, making feeds adventuring, and
   adventuring brings back rare materials. No skill is an island.
2. **Place matters.** Resources come from the land, so they differ by region:
   ironwood in the elven meadows, deep ores in the dwarven highlands,
   obsidian and fire-salts in the volcanic lands. This gives each land a
   purpose, drives travel and trade between peoples, and connects skills to
   the world generator (a tree is a lumber source; a boulder is a mining node).
3. **Peoples have specialities.** Each people has a traditional craft with
   unique recipes and techniques, which others can learn only by travelling
   there and earning trust.
4. **Choices, not just grinding.** Crafting has decisions: materials change
   the properties of what you make (a sword of volcanic steel versus dwarven
   steel), and quality depends on skill, tools and care.
5. **Made things matter.** The best equipment comes mostly from players.
   Tools, armour, potions and food wear out or get used up, so crafters stay needed.
6. **Social by design.** Some projects need several skills and several people
   (a ship, a guild hall, rebuilding a village).

### A starting point for discussion

These are proposals to react to, not decisions:

- **Gathering:** Mining, Forestry (felling and harvesting trees), Foraging
  (herbs, flowers, mushrooms), Fishing, Hunting and trapping (hides, meat,
  bone).
- **Making:** Smithing (metal gear and tools), Leatherworking and tailoring,
  Woodworking (bows, staves, furniture), Alchemy (potions and reagents),
  Cooking, Enchanting (imbuing items with magic).
- **World and community:** Building (houses, and helping rebuild settlements),
  Cartography and exploration (maps, discovering places, fast travel),
  Husbandry (mounts and animals).

## Authored content

The land is generated; the life in it is authored. NPCs, dialogue trees,
shops, monsters and questlines are written on top of the generated world,
largely by non-programmers, and reload while the game is running. See
[content-authoring.md](content-authoring.md).

## Input on the pitch

Some thoughts that could help bring this together:

- **Lean into the generator.** A deterministic world lets us have a *big*
  world cheaply. The hard part is making it feel authored. The overlay system
  exists for exactly this: generate the canvas, then paint by hand the places
  players remember.
- **Give each people a home town first.** A vertical slice could be one
  region, for example the human grassland: one town, the roads and woods
  around it, a river, a cave, two or three gathering skills and one crafting
  skill. That's enough to test whether the look and feel work.
- **Borders are interesting places.** Where two lands meet (dwarven highlands
  against the volcanic wastes) is a natural place for trade posts, conflict and
  rare mixed resources.
- **Settlements could grow.** Since settlement sites are generated and
  deterministic, towns could have states (ruined, rebuilt, thriving) that
  players change over time with the Building skill.
