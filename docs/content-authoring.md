# Content authoring

> Decided: **RON** for data, **Yarn Spinner** for dialogue, **Rhai** for
> scripts. How they fit the client and server is in [client-server.md](client-server.md).
>
> **Working now:** appearances and NPCs in RON, NPC behaviour in Rhai,
> conversations in Yarn Spinner, hot reload, a checking tool, and streaming
> the client's part to browsers. See "The content pack today" below. Items,
> shops and quests come in milestone 3.

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
  appearances/*.ron    how characters look (the only part sent to clients)
  npcs/*.ron           who lives where
  scripts/*.rhai       how NPCs behave
  dialogue/*.yarn      conversations with NPCs
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

### Appearances

```ron
[
    (id: "guard", skin: (0.84, 0.64, 0.51), hair: (0.22, 0.15, 0.09),
     tunic: (0.20, 0.25, 0.45), trousers: (0.18, 0.18, 0.20)),
]
```

Colours are sRGB red, green and blue, each from 0 to 1.

### NPCs

```ron
[
    (
        id: "town_guard",               // unique; used to recognise the NPC across reloads
        name: "Guard Aldren",           // shown to players
        appearance: "guard",
        at: StartingSettlement(offset: (11.0, 1.0)),
        script: Some("guard.rhai"),     // optional
        dialogue: Some("Guard"),        // optional: the Yarn node its conversation starts at
        wander_radius: 0.0,             // optional: how far from home it may wander (metres)
    ),
]
```

Placement (`at`) is relative to the generated world's settlements, so content
stays put even when the terrain generator is tuned. Offsets are in metres,
`(east, south)`:
- `StartingSettlement(offset: (x, y))`: the town new characters start in.
- `Settlement(index: n, offset: (x, y))`: settlement number `n` in the baked
  world (0 is the best site).
- `Position((x, y))`: an exact spot in the world.

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
- `{player_name()}` is the name of the player in the conversation.
- Variables (`<<set $asked to true>>`) last for one conversation for now.
  Saved variables, and commands that affect the game (`<<start_quest ...>>`),
  come with quests.
- The conversation ends if the player walks more than 6 m away.
- Comments (`// ...`) go inside a node, not before its `title:`.

The check catches Yarn syntax errors, NPCs naming a node that doesn't exist,
jumps to nodes that don't exist, and loops that would jump round forever
without saying anything.

### Scripts

A script defines **hooks**, functions the game calls when things happen:

| Hook | When |
| --- | --- |
| `on_spawn(npc)` | The NPC appears (at startup, or after a reload replaces it). |
| `on_interact(npc, player)` | A player starts talking to the NPC (after walking over). The conversation itself is Yarn dialogue; this hook is for anything else the NPC should do. |
| `on_idle(npc)` | Every few seconds while the NPC is standing still. |

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
Scripts react to events. They never run every frame for every creature.

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
