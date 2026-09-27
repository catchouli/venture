# Content authoring

> Decided: **RON** for data, **Yarn Spinner** for dialogue, **Rhai** for
> scripts. How they fit the client and server is in [client-server.md](client-server.md).
>
> **Working now:** appearances, NPCs and items in RON, NPC behaviour in Rhai,
> conversations in Yarn Spinner, hot reload, a checking tool, and streaming
> the client's part to browsers, shops opened from dialogue, and quests. See
> "The content pack today" below.

The world's land (terrain, rivers, biomes, settlement sites, roads) is
generated. Everything that makes it a *game* is added by people on top:
NPCs and their dialogue, shops, monsters and where they roam, quests,
events. Much of it should be writable by **people who aren't programmers**
(writers, designers, community builders), and changes should show up
**without restarting** the game or server.

## Principles

1. **Use the simplest tool that works.** Most content is data, not code: a
   shop is a list of items and prices, and a monster spawn is a position, a
   kind and a count. Only reach for a script when something needs logic.
2. **Writers write dialogue, not code.** Conversation should read like a
   screenplay, with branching and conditions that don't look like programming.
3. **Hot reload everything.** Save a file, and the running game picks up the
   change within a second or two: data, dialogue and scripts alike.
4. **Safe by construction.** Content can't crash or hang the server, read
   files or reach the network. Scripts only get the game functions we give them.
5. **The server is in charge.** Quest state, rewards and shop transactions run
   on the server. The client shows dialogue and sends the player's choices.
   Client-side scripts (for presentation only) are streamed from the server
   and run in a strict sandbox. See "Scripts on both sides" in
   [client-server.md](client-server.md).
6. **Mistakes are caught early.** Content is checked on load (does this NPC's
   shop exist? does this quest give an item that exists?) and errors point to
   the file and line.

## The content pack today

```
content/
  classes/*.ron        the classes players choose from
  abilities/*.ron      what abilities do (sent to clients)
  appearances/*.ron    how characters look (sent to clients)
  characters/          character models: bodies, hair, outfit parts, held things and
                       animations (glTF), and characters.ron, which names them (sent to clients)
  items/*.ron          things players can carry (sent to clients)
  icons/*.png          item, ability and action pictures (sent to clients)
  textures/<name>/     ground textures: colour, normal and surface JPEGs (sent to clients;
                       see textures.md)
  skills/*.ron         gathering and making skills (sent to clients)
  sounds/*.wav         sounds (sent to clients)
  sounds.ron           the game's own sounds: hits, misses, levelling up
  ui/theme.ron, ui/*   the interface's look: frame pictures, tab icons, font (sent to clients)
  ui/layouts/*.ron     where each window's parts go (see ui-layouts.md)
  ui/styles.ron        the interface's colours, text styles and bar colours
  monsters/*.ron       monsters: stats, attack, behaviour and look
  spawners/*.ron       where monsters live
  respawn_points.ron   where knocked-out players wake up
  new_characters.ron   what new characters start with
  npcs/*.ron           who lives where
  shops/*.ron          what shops sell
  quests/*.ron         quests and their stages
  scripts/*.rhai       how NPCs behave
  dialogue/*.yarn      conversations with NPCs
```

**Notes, not comments.** The game's data files (classes, abilities,
appearances, items, skills, monsters, spawners, respawn points, new
characters, NPCs, shops and quests) are written by the editor (see
[plan-editor.md](plan-editor.md)), which rewrites a whole file when it saves.
So they hold no comments: what the fields mean is explained here, and
anything worth saying about a particular entry goes in its `notes` field,
which every entry has and the game ignores:

```ron
(id: "goblin_east", monster: "goblin", at: (2408.0, 2128.0),
 radius: 10.0, respawn_seconds: 45.0, notes: "A little way out, on open ground."),
```

Check it for mistakes (every problem is reported with its file, and line
where there is one):

```sh
cargo run -p venture_content --features scripts --bin check
```

**Hot reload:** while the server (or the desktop `--local` client) runs, just
save a file. The server reloads within about a second:
- NPCs whose definition changed are replaced;
- NPCs whose only change is their script keep going with the new script;
- connected clients download any changed appearances and redraw at once.

If the new content has a problem, the server logs it and keeps the old
content, so a typo never takes the game down.

### Classes

```ron
[
    (
        id: "fighter",                  // saved in profiles, so don't rename lightly
        name: "Fighter",
        description: "Tough and strong...",   // shown when choosing a class
        rules: (hit_die: 10, mana_per_level: 3, casting_stat: Wisdom),
        stats: (strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10),
        starting_equipment: ["training_sword", "wooden_chestguard", "wooden_legguards"],
        starting_abilities: ["whack"],
        abilities: [                       // learned with ability points: from a level, for a cost
            (ability: "cleave", level: 3, cost: 1),
            (ability: "second_wind", level: 5, cost: 2),
        ],
    ),
]
```

Players choose a class when they make a character, from those with
`starting` (true unless set to `false`: classes that aren't offered could be
reached later, by advancing); the first class listed is the default. `rules` is the class's part in the RPG rules (see
`crates/rpg`): `hit_die` sets health per level (SRD 5.1: d10 for a fighter,
d6 for a wizard), and `mana_per_level` plus the `casting_stat`'s modifier
sets mana. `stats` are a new character's stats. New characters wear
the `starting_equipment` (items that can be equipped; no two in the same
slot) and know the `starting_abilities`, plus everyone's abilities from
`new_characters.ron`. `abilities` are the ones the class can learn later.

### Abilities

```ron
[
    (
        id: "whack",                    // saved in profiles, so don't rename lightly
        name: "Whack",
        description: "A big, clumsy swing that lands hard.",
        icon: "whack.png",              // a PNG in icons/
        mana: 2,                        // optional: what it costs
        cooldown: 6.0,                  // optional: seconds before it can be used again
        effect: Strike(damage: "1d8"),
        sound: Some("whack.wav"),       // optional: a WAV in sounds/, played when it's used
        visual: (colour: (1.0, 0.75, 0.35), size: 1.6),  // optional: its flash, bolt or glow
    ),
    (
        id: "air_blast",
        // ...
        range: 15.0,                    // metres to the target, for bolts
        effect: Bolt(damage: "1d6", speed: 10.0),  // speed in m/s (12 if not given)
    ),
]
```

`effect` is one of:
- `Rest`: sit and recover health and mana three times as fast, until you
  move or are hit. It can't be used in a fight, and using it again stands up.
- `Strike(damage)`: a weapon blow with these dice instead of the weapon's,
  plus the weapon's stat. It reaches as far as a melee attack (whatever
  `range` says). The player runs up to the target first if needed, and goes
  on attacking afterwards.
- `Bolt(damage, speed)`: a spell that flies to the target and lands when it
  gets there, rolled with the spell attack bonus. The casting stat adds
  to its damage. It needs a `range`, and the player runs into range first if
  needed.
- `Heal(amount)`: heals the user, plus their casting stat's modifier.

Dice are written the usual way: `"1d8"`, `"2d6+1"`. The rules are in
`crates/rpg/src/abilities.rs`. Abilities are sent to clients for their names,
icons, sounds and looks.

### Skills

```ron
[
    (id: "fishing", name: "Fishing", description: "Catching fish in rivers, lakes and the sea.",
     icon: "skill_fishing.png"),
]
```

Skills are what characters get better at by doing them (gathering and
making things). Each has its own level and experience, on the same curve as
character levels, and the Skills tab lists them in file order. For now they
only have names and levels: what each one involves is still being designed
(see [vision.md](vision.md)). The `id` is saved in profiles.

### The interface's look (theme)

Every frame, button, slot, tab and bar in the interface is drawn with a
picture from `ui/`, named in `ui/theme.ron`:

```ron
(
    pieces: {
        "window": (image: "window.png", border: 12),
        "divider": (image: "divider.png", border: (20, 20, 0, 0)),   // (left, right, top, bottom)
        // ...
    },
    tab_icons: { "stats": "tab_stats.png", /* ... */ },
    font: Some("my_font.ttf"),   // optional: a TTF or OTF in ui/ for all text
)
```

Each picture is drawn as a **nine-slice**. `border` says how wide (in the
picture's pixels) the corners and edges are. The corners are drawn as they
are, the edges stretch along (or repeat, with `tile: true`), and the middle
stretches to fill the rest. So a window of any size keeps crisp corners.
Keep ornaments (rivets, corner flourishes) inside the corners, and make
edges that look right stretched.

Things to know when painting:
- **Nearly opaque:** the game blends in linear colour, so a dark frame that's
  even a little see-through shows a lot of the bright world behind it. Only
  the `chat` backing is meant to be see-through.
- **Bars are tinted:** `bar_fill` is tinted each bar's colour (health red,
  mana blue, experience gold) and `bar_back` a darker shade of it (dark red
  behind health), so paint both in white and greys. `bar_frame` is drawn
  over the whole bar as painted, for a rim or a shine; leave its middle
  clear.
  plain one is used.
- **Plain look:** a piece left out keeps the plain flat look, as does
  everything before the theme has downloaded.
- **Live changes:** save a picture (or `theme.ron`) while the game is running
  and the interface changes within a second or so.
- **Size:** pictures can be up to 1 MB each.

The pieces (the placeholders are made by `python3 tools/placeholder_ui.py`):

| Piece | What it is | Placeholder size | Border |
| --- | --- | --- | --- |
| `window` | the frame of a window (character window, shop, dialogue) | 64×64 | 12 |
| `header` | the strip across the top of a window, behind its title | 64×24 | 4 |
| `panel` | a smaller frame (player and target frames, chat, hotbar, toolbar) | 32×32 | 6 |
| `chat` | the chat log's backing (usually see-through) | 32×32 | 5 |
| `tooltip` | a tooltip's frame | 32×32 | 5 |
| `button` | a button with words on it | 48×24 | 6 |
| `button_hover` | a button under the mouse | 48×24 | 6 |
| `button_pressed` | a button being pressed | 48×24 | 6 |
| `button_chosen` | the button for the chosen option (a class or body type when making a character) | 48×24 | 6 |
| `slot` | a box holding an icon (inventory, gear, abilities, hotbar) | 40×40 | 8 |
| `slot_hover` | a slot under the mouse | 40×40 | 8 |
| `bar_back` | the empty part of a bar: drawn light, and tinted a darker shade of each bar's colour | 24×12 | 4 |
| `bar_fill` | the full part of a bar: drawn light, and tinted each bar's colour | 24×12 | 3 |
| `bar_frame` | drawn over a whole bar, not tinted: its rim, or a shine | 24×12 | 4 |
| `close` | a window's close button | 18×18 | 0 |
| `close_hover` | a close button under the mouse | 18×18 | 0 |
| `divider` | a line under headings | 64×6 | (20, 20, 0, 0) |
| `text_field` | a box to type in (chat, names) | 32×24 | 5 |
| `toolbar_button` | a toolbar button, behind its icon | 40×40 | 8 |
| `toolbar_button_hover` | a toolbar button under the mouse | 40×40 | 8 |
| `toolbar_button_open` | the toolbar button for the tab that's open | 40×40 | 8 |

`tab_icons` are the toolbar's icons, one for each
tab: `stats`, `skills`, `abilities`, `inventory`, `gear`, `quests` and `emotes`
(about 32×32, drawn at 26×26).

### Sounds

Sounds are WAV files in `sounds/` (at most 512 KB each), sent to clients.
Abilities name theirs, and `sounds.ron` names the game's own:

```ron
(hit: Some("hit.wav"), critical: Some("critical.wav"), miss: Some("miss.wav"),
 death: Some("death.wav"), level_up: Some("level_up.wav"))
```

The current sounds are placeholders made by `python3 tools/placeholder_sounds.py`.
Real recordings can replace them file for file.

### Appearances

```ron
[
    (id: "guard", skin: (0.84, 0.64, 0.51), hair: (0.22, 0.15, 0.09),
     tunic: (0.20, 0.25, 0.45), trousers: (0.18, 0.18, 0.20),
     body: Masculine, outfit: Armour, equipment: ["training_sword", "wooden_buckler"]),
]
```

Colours are sRGB red, green and blue, each from 0 to 1. `body` (`Masculine`,
`Feminine` or `Androgynous`) sets the build and hair, and `outfit` (`Tunic`,
`Armour` or `Robe`) the cut of the clothes; both are optional. With character
models, `outfit` picks a set of parts from `characters.ron`; `parts: [...]`
names parts to wear instead, `hairstyle: "long"` a hairstyle other than the
body's usual one (any hair that isn't a beard), and `beard: "beard"` a beard
(one of the `beards` in `characters.ron`). `equipment: [...]` names items
(from `items/`) they wear and carry, as players do: armour and clothes go on
over their parts (a part is left off where the item covers the same place),
and weapons and shields are held. It's only for show; NPCs don't fight. `height`, `build` and `head`
(from -10 to 10, as in the character creator) set their size; any left out
are picked from the appearance's id (between -6 and 6), so each person
looks a little different, but always the same. On models, the skin and
hair colours are used as they are; the tunic and trousers colours only
dress the placeholder shapes (shown until the models load).

### Character models

People (players and NPCs) are a body, hair, and outfit parts, all glTF models
on one shared skeleton, animated by one file of animations. They live in
`characters/`, with `characters.ron` saying what's what:

```ron
(
    animations: "animations.glb",
    clips: { "idle": "Idle_Loop", "run": "Jog_Fwd_Loop", /* ... */ },
    bodies: (
        masculine: (model: "body_male.gltf", fits: Masculine, hair: "parted",
                    skin_colour: (0.66, 0.47, 0.33)),
        feminine: (/* ... */), androgynous: (/* ... */),
    ),
    hairstyles: ["parted", "long", "buns"],
    beards: ["beard"],
    hair_colours: [(0.32, 0.21, 0.12), (0.10, 0.07, 0.05)],
    skin_tones: [(0.93, 0.80, 0.72), (0.66, 0.47, 0.33), (0.32, 0.21, 0.15)],
    hair: { "parted": "hair_simpleparted.gltf", "long": "hair_long.gltf", "beard": "hair_beard.gltf" },
    parts: {
        "ranger_body": (masculine: "male_ranger_body.gltf", feminine: "female_ranger_body.gltf",
                        covers: [Torso]),
        "ranger_hood": (masculine: "male_ranger_head_hood.gltf", hides_hair: true),
        "bronze_sword": (masculine: "sword_bronze.gltf",
                         held: (bone: "hand_r", position: (0, 0.05, 0), rotation: (0, 0, 0), scale: 1)),
    },
    plain_clothes: ["peasant_body", "peasant_arms", "peasant_legs", "peasant_feet"],
    outfits: { "Tunic": [/* parts */], "Armour": [/* ... */], "Robe": [/* ... */] },
)
```

- **clips** maps the game's animation names to names in the animations file.
  The game needs `idle`, `walk`, `run`, `attack` (with a weapon), `punch`
  (without), `cast`, `sit_down`, `sitting` and `stand_up` (resting), `jump`,
  `wave`, `hit` and `death`. While a character moves, `attack`, `punch` and
  `cast` play on the upper body only, over the walk or run.
- **bodies**: each body type's model, which parts fit it (parts are made
  for masculine or feminine bodies; a part with only one model uses it on
  both), its usual hairstyle, and the colour of the skin in its texture
  (the texture is tinted from it to each skin tone).
- **hairstyles**, **beards**, **hair_colours** and **skin_tones** are what
  players choose from when making a character, in the order shown.
  Hairstyles and beards are names from **hair** (the hair models); colours
  are sRGB, 0 to 1. The server only accepts choices from these lists.
- **parts** are clothes and armour (worn on the skeleton) or, with `held`,
  things attached to a bone (`hand_r`, `hand_l`, `lowerarm_r`, `lowerarm_l`,
  `Head`, `spine_03`, `pelvis`, `root`), placed in the bone's own directions
  (metres, and degrees about X, Y, then Z). `covers` hides the body under the
  part (`Torso`, `Arms`, `Hands`, `Hips`, `Legs`, `Feet`) so skin doesn't
  poke through, and `hides_hair` is for hoods and helmets.
- **monsters** are whole models for monsters (`"puglin": (model:
  "puglin.gltf", scale: 0.9)`). They're on the characters' skeleton, so
  they play the same animations.
- **plain_clothes** fill in wherever nothing worn covers the body.
- **outfits** are the parts for each appearance `outfit`.

Items put parts on whoever wears them (`parts` in `equip`, below). A weapon
without a held part shows as a placeholder shape in the hand. The server
only sends the ids of what a player wears; the client works out the parts.

The models come from Quaternius's CC0 packs (see `licenses/README.md`).
`python3 tools/import_characters.py ~/assets` copies them in from the
downloads: it shrinks textures, drops what Bevy can't use, and keeps only the
animations in its list (so add a new animation there first). Model files are
kept in Git LFS.

### Items

```ron
[
    (
        id: "bread",                    // unique; saved in players' inventories, so don't rename lightly
        name: "Bread",
        description: "A crusty loaf from Elsie's ovens, still a little warm.",
        icon: "bread.png",              // a PNG in icons/
        stackable: false,               // optional: true for things like arrows, which share one slot
        value: 4,                       // optional: worth in Miasma; shops buy for half. 0 = unsellable
        on_use: Heal(Flat(5)),          // optional: what using it does (clicking it in the bag)
    ),
    (
        id: "training_sword",
        name: "Training sword",
        description: "A blunt blade for learning the basics.",
        icon: "training_sword.png",
        value: 8,
        equip: Some((                   // optional: makes the item wearable
            slots: [DominantHand],      // Head, DominantHand, NonDominantHand, Body and/or Legs
                                        // (a robe fills [Body, Legs])
            weapon: Some((damage: "1d6", speed: 1.0, stat: Strength)),   // for DominantHand items
            armour: 0,                  // optional: armour class it adds
            look: (style: Sword, main: (0.72, 0.74, 0.78), trim: (0.36, 0.23, 0.13)),
            parts: ["bronze_sword"],    // optional: outfit parts it puts on (see Character models)
        )),
    ),
]
```

An item's `look` is how it shows on the character: `Plates` (chest and back),
`LegPlates`, `Robe` (shoulders to ankles), `Sword` or `Staff` (in the right
hand), in a main and a trim colour: the placeholder shapes, shown until the
character models load, and for weapons with no held part. A weapon's `speed` is attacks per second before Dexterity, and its
`stat` is the one it attacks and does damage with.

`on_use` is what happens when a player uses the item (clicks it in their
bag), which uses one up:
- `Heal(Flat(5))` heals 5 health; `Heal(Percent(20.0))` heals a fifth of
  the player's most health (at least 1). It never heals past full, and it
  isn't used up at full health.

Left out, the item can't be used. An item can't be both worn and used.
After using an item, a player waits a moment (`item_cooldown` in
`server.ron`) before using another.

Money is **Miasma**, kept on the character rather than in the inventory. Icons are small PNGs
(32 × 32 works well, at most 64 KB). The current ones are placeholders drawn
by `tools/placeholder_icons.py`; replace them with painted icons of the same
name whenever they're ready.

`new_characters.ron` says what every new character carries, and the abilities
everyone knows:

```ron
(miasma: 25, items: [(item: "bread", count: 1)], abilities: ["rest"])
```

### Monsters

```ron
[
    (
        id: "goblin",
        name: "Goblin",
        level: 2,
        health: 9,
        armour_class: 12,
        // Hits if d20 + bonus reaches the target's armour class. speed is attacks per second.
        attack: (damage: "1d4", bonus: 2, speed: 0.7),
        aggressive: true,      // attacks players within aggro_radius; otherwise only fights back
        aggro_radius: 9.0,
        wander_radius: 5.0,    // how far from home it strolls
        experience: 35,        // given to whoever kills it
        look: (shape: Goblin, main: (0.42, 0.58, 0.28), trim: (0.40, 0.30, 0.20)),
    ),
]
```

The shapes are placeholder models drawn by the client (`Rabbit` and `Goblin`
so far), coloured with `main` and `trim`. `model: Some("puglin")` in the
`look` dresses the monster in a monster model from `characters.ron`
instead, once the models have loaded (the shape stays until then). A monster led more than 30 m from
home gives up, goes back, and heals.

### Spawners

```ron
[
    (id: "goblin_north", monster: "goblin", at: (2328.0, 2083.0),
     radius: 10.0, respawn_seconds: 45.0),
]
```

Each spawner keeps one monster alive somewhere walkable within `radius`
metres of `at` (placed like NPCs), and brings a new one `respawn_seconds`
after it dies. A spawner with no walkable ground in its radius (say, in a
lake) logs a warning and spawns nothing. Changing spawners or monsters
restarts all spawners.

### Respawn points

```ron
[
    (name: "the town square", at: (2328.0, 2168.0)),
]
```

A knocked-out player wakes up at the nearest one ("You wake up at the town
square.") with full health and nothing lost. With none, they wake at the
starting spawn point.

### Shops

```ron
[
    (
        id: "elsies_bakery",
        name: "Elsie's Bakery",            // shown at the top of the shop window
        stock: [
            (item: "bread", price: 5),     // price in Miasma
            (item: "meat_pie", price: 15),
        ],
    ),
]
```

A shop is opened from its NPC's dialogue with `<<open_shop elsies_bakery>>`
(see below). Shops buy anything with a `value`, for half of it.

### Quests

```ron
[
    (
        id: "tomas_pipe",                  // used by dialogue, and saved in profiles
        name: "Old Tomas's Pipe",
        stages: [
            (id: "asked", journal: "Old Tomas has lost his pipe..."),
            (id: "found", journal: "Hilda found a pipe by her anvil..."),
            (id: "done", journal: "I gave Old Tomas his pipe back..."),
        ],
    ),
]
```

A quest is a story told through dialogue: `<<set_quest tomas_pipe found>>`
moves the player on to a stage (starting the quest if need be), and
`quest_stage("tomas_pipe")` says where they are (`""` if they haven't
started). Reaching the **last** stage completes the quest. The player's quest
journal (**J**) shows each quest's name and the current stage's `journal`
text, and chat tells them when a quest starts, moves on or is complete.

Quests only move **forward**: setting a stage the player has already reached
or passed does nothing. So a reward given next to `<<set_quest ... done>>`
can't be claimed twice, as long as the dialogue checks `quest_stage` first.
Rewards are ordinary commands (`<<give_miasma 30>>`, `<<give_item bread>>`). The pipe quest in
`dialogue/starting_town.yarn` is a complete example: Tomas starts it, Hilda
hands over the pipe (checking `has_room_for` first), and Tomas takes it back
and pays up. His script (`scripts/wanderer.rhai`) celebrates out loud through
the `on_quest_stage` hook.

### NPCs

```ron
[
    (
        id: "town_guard",               // unique; used to recognise the NPC across reloads
        name: "Guard Aldren",           // shown to players
        appearance: "guard",
        at: (2339.0, 2169.0),
        script: Some("guard.rhai"),     // optional
        dialogue: Some("Guard"),        // optional: the Yarn node its conversation starts at
        wander_radius: 0.0,             // optional: how far from home it may wander (metres)
    ),
]
```

`at` is a spot in the world, in metres `(east, south)` from its north-west
corner (the same for spawners and respawn points). The easiest way to pick
one is to drag the NPC around in the editor. (If the terrain generator is
retuned so towns move, placed things have to be moved with them.)

### Two ways NPCs talk

- **Out loud**, for everyone nearby: a script calls `npc.say(...)`. It shows
  above the NPC's head and in the chat of players within 40 m. Use it for
  ambient life: greetings, mutterings, hints that repeat.
- **In conversation**, privately: when a player talks to an NPC (clicks it
  again once selected, or types `/talk`), a **dialogue window** opens with
  the NPC's Yarn conversation. Only that player sees it, and several players
  can talk to the same NPC at once, each in their own conversation.

### Dialogue

Conversations are [Yarn Spinner](https://docs.yarnspinner.dev) files in
`dialogue/`. All of them are compiled together, so an NPC's `dialogue` can
name a node in any file, and nodes can `<<jump>>` between files.

```
title: Guard
---
Guard Aldren: Halt! Oh, a new face. Welcome to town, {player_name()}.
-> Any advice for a traveller?
    Guard Aldren: Stay on the roads after dark. Wolves don't.
    <<jump GuardMore>>
-> Goodbye.
    Guard Aldren: Keep your wits about you.
===
```

- A line starting with a name and a colon is said by that character; it's
  shown in the window with the name highlighted.
- `-> ...` lines are the player's replies. The window shows everything said
  up to the next set of replies, then waits for the player to pick one. An
  option with a condition that's false (`-> Secret <<if $friendly>>`) isn't
  offered. When there are no more replies, the conversation is over and the
  window offers "Goodbye".
- **Functions** tell dialogue about the player:
  - `{player_name()}`: their name.
  - `miasma()`: how much Miasma they have.
  - `item_count("bread")`: how many of an item they carry
    (`-> Here's your pie. <<if item_count("meat_pie") > 0>>`).
  - `has_room_for("old_pipe")`: whether their inventory has room for one more.
  - `quest_stage("tomas_pipe")`: the stage they've reached in a quest, or `""`.
- **Commands** make things happen:
  - `<<give_item bread 2>>` puts items in the player's inventory (the count is
    optional). If the inventory is full, the player is told and gets nothing.
  - `<<take_item old_pipe>>` takes items. Check they have them first with
    `item_count`.
  - `<<give_miasma 30>>` pays the player, and `<<take_miasma 10>>` takes their
    money (check `miasma()` first).
  - `<<set_quest tomas_pipe found>>` moves a quest on (see Quests above).
  - `<<open_shop elsies_bakery>>` ends the conversation and opens a shop. It
    ends at once, so lines before it on the same page aren't shown.

  The player sees what they received or handed over in chat. Conditions
  later on the same page already see the change.
- Variables (`<<set $asked to true>>`) last for one conversation for now.
  Saved variables, and commands that affect the game (`<<start_quest ...>>`),
  come with quests.
- The conversation ends if the player walks more than 6 m away.
- Comments (`// ...`) go inside a node, not before its `title:`.

The check catches Yarn syntax errors, NPCs naming a node that doesn't exist,
jumps to nodes that don't exist, loops that would jump round forever without
saying anything, misspelt functions, and commands that don't exist or name
items, shops, quests or stages that don't.

### Scripts

A script defines **hooks**, functions the game calls when things happen:

| Hook | When |
| --- | --- |
| `on_spawn(npc)` | The NPC appears (at startup, or after a reload replaces it). |
| `on_interact(npc, player)` | A player starts talking to the NPC (after walking over). The conversation itself is Yarn dialogue; this hook is for anything else the NPC should do. |
| `on_idle(npc)` | Every few seconds while the NPC is standing still. |
| `on_quest_stage(npc, player, quest, stage)` | A player's conversation with the NPC moved a quest on (the quest and stage ids, as text). |

What scripts can use:

| | |
| --- | --- |
| `npc.name`, `npc.x`, `npc.y`, `npc.home_x`, `npc.home_y` | Read-only facts about the NPC. |
| `npc.say(text)` | Say something: shown above the NPC and in the chat of players within 40 m. |
| `npc.wander()` | Walk to a random spot within the NPC's `wander_radius` of home. |
| `npc.move_to(x, y)` | Walk towards a point, kept within the wander radius. |
| `player.name`, `player.x`, `player.y` | Read-only facts about the player. |
| `random(n)` | A random whole number from 0 to n - 1. |
| `chance(p)` | True with probability `p` (0.0 to 1.0). |
| `print(text)` | Write to the server log (handy while writing a script). |

```rhai
fn on_idle(npc) {
    npc.wander();
    let mutterings = ["Where did I leave my pipe?", "Lovely weather for it."];
    if chance(0.1) {
        npc.say(mutterings[random(mutterings.len())]);
    }
}
```

**How scripts are kept safe:** a script never changes the game itself.
Calls like `npc.say` only *queue a request*, which the engine checks (for
example, speech is cleaned and length-limited, and walking stays within the
wander radius) and then carries out. Scripts can't read files, reach the
network, `eval` code or import other files, and each hook call has limits on
how much work it may do and how big its data may grow. A runaway loop is
stopped and logged, and the game carries on. A misspelt hook name, or a hook
with the wrong number of arguments, is caught when the content is checked.

## Three layers

| Layer | For | Technology | Who writes it |
| --- | --- | --- | --- |
| **Data** | NPCs, shops, items, monsters, spawn areas, loot tables, overlay stamps | [RON](https://github.com/ron-rs/ron) files, loaded with `serde` | Designers (and later, in-game editors) |
| **Dialogue** | Conversations, choices, barks | [Yarn Spinner](https://www.yarnspinner.dev) (`.yarn` files) | Writers |
| **Logic** | Quest steps, event reactions, special NPC behaviour | [Rhai](https://rhai.rs) scripts (`.rhai`) | Designers with a little scripting |

Heavy systems (combat, movement, AI pathfinding, economy maths) stay in Rust.
Scripts react to events. They never run every frame for every monster.

### Data: RON

RON reads like Rust structs, is easy to hand-edit, and maps straight onto our
types with `serde`, so a typo in a field name is a load error with a line
number, not a silent bug.

```ron
// content/npcs/millbrook_blacksmith.ron
Npc(
    id: "millbrook_blacksmith",
    name: "Hilda Brand",
    appearance: "human_female_smith",
    settlement: "millbrook",        // placed relative to a settlement overlay
    position: (12.0, -4.5),
    dialogue: Some("Hilda"),         // a node in dialogue/*.yarn
    shop: Some("millbrook_smithy"),
)
```

### Dialogue: Yarn Spinner

Yarn is a screenplay-like format designed for writers: lines, choices,
variables and conditions, plus *commands* that call into the game. It is
widely used in shipped games, and has an official Rust implementation
(`yarnspinner`, plus `bevy_yarnspinner` for Bevy). Its compiler and runtime
don't need an engine, so dialogue can run on the server.

```yarn
title: Start
---
Hilda: Back again? The forge doesn't feed itself.
-> I'd like to trade.
    <<open_shop "millbrook_smithy">>
-> Any work going? <<if not quest_started("iron_for_hilda")>>
    Hilda: Bring me ten iron ore from the hills and I'll make it worth your while.
    <<start_quest "iron_for_hilda">>
-> Just passing through.
    Hilda: Mind the sparks.
===
```

*Risk:* the Rust port is marked as a work in progress and has few users. The
format and its tools (a VS Code extension with previews) are mature, and the
runtime is small, so we can fix problems ourselves if we need to. The
fallback is [Ink](https://www.inklestudios.com/ink/), via a Rust runtime such as `bladeink`.

### Logic: Rhai

Rhai is a scripting language made for embedding in Rust. Its syntax looks
like a friendlier mix of JavaScript and Rust (no lifetimes, no types to
declare). Why it fits:

- **Pure Rust**: no C toolchain, and it builds for every target we use,
  including WebAssembly (`wasm32-unknown-unknown`). The same scripts could
  run in the browser client for previews or offline play.
- **Sandboxed**: scripts can only call functions we register. There are
  built-in limits on operations, call depth, string and array sizes, so a
  runaway script is stopped instead of freezing the server.
- **Hot reload is trivial**: a script compiles to a syntax tree in
  milliseconds. When the file changes, compile it again and swap it in.
- **Actively maintained**, with a good book and a playground.

```rhai
// content/quests/iron_for_hilda.rhai
fn on_item_collected(player, item) {
    if item == "iron_ore" && player.count("iron_ore") >= 10 {
        player.set_step("iron_for_hilda", "return");
        player.notify("You have enough ore. Return to Hilda.");
    }
}

fn on_talk(player, npc) {
    if npc == "millbrook_blacksmith" && player.step("iron_for_hilda") == "return" {
        player.take("iron_ore", 10);
        player.give("iron_sword", 1);
        player.complete("iron_for_hilda");
    }
}
```

## Alternatives considered

| Option | For | Against |
| --- | --- | --- |
| **Luau** via `mlua` | Much faster than Rhai. Gradual typing. Proven with non-programmer creators (Roblox), and used in Alan Wake 2, Warframe and Farming Simulator 25. | C++ underneath: `mlua` doesn't build for `wasm32-unknown-unknown` (only Emscripten), so it couldn't run in our web client. Less "native" to Rust. A pure-Rust port, `luaur`, exists but is young. |
| **Rune** | Rust-like syntax, async support. | Smaller community. Its closeness to Rust is a drawback for non-programmers. |
| **Koto** | Small, clean, pure Rust. | Small community; a functional style is less familiar to designers. |
| **WebAssembly modules** (Wasmtime, Wasmi) | Very fast, any source language, strong sandbox. | Far too technical for non-programmers: they'd need a compiler toolchain. |
| **Visual scripting** (node graphs) | The most approachable of all. | A big project to build well. Better as a later layer that *generates* data or scripts. |

**The main trade-off:** Rhai is slower than Luau (it interprets a syntax tree
rather than compiling to bytecode). That's fine for event-driven content
logic, which runs occasionally per player. If profiling ever shows scripting
as a bottleneck on the server, we can move hot paths into Rust or switch the
server to Luau. To keep that option open, scripts should only see a small
**game API** we define (such as `give`, `take`, `start_quest`, `spawn`),
never engine internals, so the language behind it can change.

## How it fits together

```
content/
  npcs/*.ron       quests/*.ron      shops/*.ron     spawns/*.ron
  dialogue/**/*.yarn                 scripts/**/*.rhai
      │
      ▼  loaded and validated at startup; watched for changes
venture_content   (new crate: types, loading, validation, hot reload)
      │
      ├──► server: runs dialogue and scripts, owns quest state, spawns NPCs
      └──► client: shows NPCs, dialogue text and choices
```

- **Where content lives:** a `content/` folder in the repository, in plain text,
  so it can be versioned and reviewed like code.
- **Hot reload:** the server watches `content/`. On a change it reloads and
  re-validates only the files affected, then swaps them in. If validation
  fails, it keeps the old version and reports the error. The client uses
  Bevy's asset hot reloading for anything it displays.
- **Validation tool:** `cargo run -p venture_content -- check` loads
  everything and reports broken references, unreachable dialogue nodes, and
  scripts that call unknown functions. It runs in CI.
- **Placing content in the world:** NPCs and spawns can be placed relative
  to generated settlements and landmarks (for example "12 m east of
  millbrook's square"), using the same overlay system the terrain uses (see
  [world-generation-engine.md](world-generation-engine.md)). Content then
  stays put even if the terrain generator is tuned.
- **Later: editors.** A web-based editor (or an in-game "builder mode") to
  place NPCs, draw spawn areas and edit dialogue visually, saving to the same
  files.
