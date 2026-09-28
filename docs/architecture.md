# Architecture

## Crates

```
venture_client ──► venture_protocol, venture_simulation, venture_navigation, venture_content, venture_world
   (Bevy)     └──► venture_server (desktop only, for --local mode)

venture_server ──► venture_protocol, venture_simulation, venture_navigation, venture_content (+scripts), venture_world
   (headless Bevy, tokio/axum, Rhai)

venture_editor ──► venture_client (its world rendering), venture_content (+reflect), venture_world
   (Bevy, egui)

venture_worldgen ──► venture_world     (the bake tool; nothing at runtime depends on it)
```

| Crate | Purpose |
| --- | --- |
| `venture_world` | Core world data: units, coordinates, chunks, terrain cells, biomes, liquids, props, and the baked world file format. Plain data that every other part of the game shares. |
| `venture_worldgen` | The world generation engine, and the `bake` tool that writes the world (and its map) to disk. |
| `venture_navigation` | Where characters can walk (blocked cells per chunk) and pathfinding (A* plus line-of-sight smoothing). |
| `venture_simulation` | Game rules both sides run: movement, speed and input validation, the day cycle, and the fight with a hooked fish. The client uses them to predict, and the server to decide. |
| `venture_rpg` | The RPG rules: stats and modifiers, dice, experience and levels, points per level, health, mana, armour class, attack speed, attack resolution, and fishing (what bites, hooks, strikes, shadows, reels, traps). Based on the SRD 5.1 (CC BY 4.0), adapted for real time. |
| `venture_protocol` | The messages between client and server, their encoding (postcard), size limits, and shared validation rules for names, chat and tokens. |
| `venture_content` | The content pack: class, appearance, item, shop, quest and NPC definitions, loading with every problem reported by file, script and dialogue checks (with the `scripts` feature), the client manifest, and the `check` tool. |
| `venture_server` | The authoritative game server: its config file, the gate for client messages, sessions and profiles (with unique names), characters and admin powers, gameplay, monsters, abilities and the hotbar, combat, fishing, inventories, shops, quests, NPCs, private NPC conversations (Yarn Spinner), the Rhai script sandbox, content hot reload, and the HTTP/WebSocket host. |
| `venture_client` | The game client: a view of the server's game, with rendering, input, prediction, actions and chat commands, chat, the dialogue window and debug tools. A library (its plugins, shared with the editor) and a thin binary. |
| `venture_editor` | The editor: the content's data files edited through the content crate's own types (by reflection), with undo, checking and saving, and the world shown with the client's rendering. See [editor.md](editor.md). |

**Rule:** only the client, the editor and the server depend on Bevy. (The
content, protocol and RPG crates can derive Bevy's `Reflect` for the editor,
behind their `reflect` feature, which only the editor turns on.) The shared crates
are engine-free, so both sides can use them and they're easy to test.

## Client and server

The server is authoritative, and the client predicts the player's own
movement so it feels instant. The world is baked ahead of time. The server
loads it (building collision and navigation from it) and serves chunks to
clients. The full design is in [client-server.md](client-server.md).

### Server structure: one way in

Every change to the live game goes through one path (see the `venture_server` crate docs):

1. `link`: transports (WebSocket, or in-memory for `--local`) deliver raw bytes.
2. `requests`: **the gate.** Decodes, size- and rate-limits, checks each
   message is allowed at that point in the connection's life, validates its
   contents, and produces typed requests. Nothing else reads client messages.
3. `sessions`: joining and leaving; the only code that creates and removes
   players, and that loads and saves profiles (through `profiles`).
4. `gameplay`: the only code that changes players while they play, and the
   per-tick simulation and snapshots.
5. `npcs`: the only code that creates, removes and moves NPCs. Scripts
   (`scripting`) can only queue commands, which `npcs` checks and applies.
   `content` loads the content pack and reloads it when files change.

`entities` holds what players and NPCs share (network id, name, position,
movement).

## Client structure

`venture_client` is grouped by concern: one folder per concern, each with a
`mod.rs` that explains it and adds a single plugin for it. Data from the
server flows in through `network` as `FromServer` messages, and commands
flow out as `ToServer` messages. No other module touches the connection.

```
src/
  lib.rs             the game's plugins (one per concern), and `run`, which makes the app
  main.rs            just calls `run`
  camera.rs          follow and free cameras, and their controls
  content/           the content pack's client files, downloaded and kept up to date
  network/           the connection to the server
    mod.rs             protocol flow and connection state (FromServer / ToServer)
    connection.rs      the transport: WebSocket, or in-memory for --local
    http.rs            downloading files from the server
    launch.rs          which server to use
    token.rs           the login token
    local_server.rs    --local mode: the game server inside the client
  map/               the minimap (top right) and the full map (M): map views drawing
                     baked tiles, place names and markers (docs/plan-map.md)
    mod.rs (MapMarker), tiles.rs (fetching tiles and names), view.rs (MapView),
    minimap.rs, window.rs
  world/             the world as the client shows it
    source.rs          loading the manifest and chunk files
    streaming.rs       chunks around the camera: data, meshes, unloading
    terrain_mesh.rs    terrain and liquid meshes
    terrain_material.rs + shaders/terrain.wgsl   the terrain's shader
    water_material.rs + shaders/water.wgsl       water's shader (waves, reflections)
    props.rs           tree, rock and bush meshes
    query.rs           ground height, raycasts, blocked cells
    lighting.rs        sun, moon, skies and their light, by the time of day
    sky.rs + shaders/sky.wgsl   drawing the sky: two skies blended, the sun and moon
    time_of_day.rs     the clock the server set, running on
    daylight.rs        the sun's colour and brightness, sky level and exposure by sun height
  characters/        characters in the world
    humans.rs          people as character models: loading them from the content,
                       dressing (body, hair, outfit parts, held things), and animation
    model.rs           the placeholder character (shown until the models load) and the
                       gait every character's animation follows
    player.rs          your character: prediction, steering, following, correction
    remote.rs          other players, NPCs and monsters, interpolated between snapshots
    monsters.rs       the placeholder monster models (rabbit, goblin) and their animation
    combat_effects.rs  swings, floating damage numbers, hit flashes, and the dead falling over
    nameplates.rs      names above characters, with orbs on the target and speech bubbles
    speech.rs          what characters said recently (shown in the bubbles)
    level_up.rs        the level-up effect
  fishing/           fishing: mod.rs (casting where water's clicked with a rod in hand, striking
                     and reeling, the float and line), panel.rs (what's happening, and the
                     fight's tension, line out and the fish's strength), shadows.rs (the fish
                     shoaling in the water round the player, solid or faint by level), traps.rs
                     (setting a trap from the bag, drawing traps, collecting your own), ripples.rs
                     (rings on the water where a line lands, at nibbles and bites), others.rs
                     (other players' lines and floats)
  interaction/       the player acting on the world
    actions.rs         the things a player can do, however they ask (click, chat command, later keys)
    admin_commands.rs  /setlevel, /reset, /setclass, /setgender, /time, /timescale, /give and /setskill, for admins
    orders.rs          ordering the character about: run, follow, walk over to talk, attack, use abilities
    abilities.rs          using abilities (checks, predicting the run-up) and their cooldowns
    targeting.rs       what's hovered and selected (anything with a Targetable), and /target's search
    clicks.rs          left clicks: select, follow, talk, attack, or run there
    move_marker.rs     the glowing move marker
  ui/                the interface
    character_window/  the character window: mod.rs (the tabs), party.rs, stats.rs, skills.rs,
                       abilities.rs, inventory.rs, equipment.rs, quests.rs, emotes.rs
    layout.rs          builds windows from the content's layout files (docs/ui-layouts.md)
    theme.rs           the interface's look: each frame, button, slot and bar drawn
                       with the content's theme pictures (nine-slices), or plain
    toolbar.rs (an icon button for each of the window's tabs, along the bottom), chat.rs, dialogue_window.rs,
    shop.rs, character_creation.rs,
    vitals.rs (your name, level, health and mana, top left),
    party.rs (your party and invitation, and the frames for the other members under yours),
    hotbar.rs (abilities and actions on keys 1 to 0, dragged there from the character window),
    target_frame.rs, loading_screen.rs, cursor.rs, text_input.rs,
    tooltip.rs, widgets.rs (shared pieces such as the close button)
  sound/             playing the content's sounds for what happens nearby
  dev/               development tools
    debug_hud.rs, screenshot.rs, settings.rs
  common/            helpers
    coords.rs, palette.rs, mesh_data.rs, procedural_image.rs
```

A concern gets split further (for example into `components.rs` and
`systems.rs`) when it grows large. The grouping stays by concern, so
everything about one feature lives together.

## Platforms

- **Web:** WebAssembly with WebGL2, built with Trunk. The main constraint is
  that there are no background threads, so heavy work must be spread across frames.
- **Desktop:** native builds for Windows, macOS and Linux.
- **Mobile:** supported by Bevy. It will need touch input and an eye on performance.
