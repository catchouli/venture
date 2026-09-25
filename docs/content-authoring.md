# Content authoring

> Design only: nothing here is implemented yet. Decided: **RON** for data,
> **Yarn Spinner** for dialogue, **Rhai** for scripts. How they fit the client
> and server is in [client-server.md](client-server.md).

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
    dialogue: "millbrook/blacksmith.yarn",
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
