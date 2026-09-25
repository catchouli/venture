# Architecture

## Crates

```
venture_client ──► venture_protocol, venture_simulation, venture_navigation, venture_content, venture_world
   (Bevy)     └──► venture_server (desktop only, for --local mode)

venture_server ──► venture_protocol, venture_simulation, venture_navigation, venture_content (+scripts), venture_world
   (headless Bevy, tokio/axum, Rhai)

venture_worldgen ──► venture_world     (the bake tool; nothing at runtime depends on it)
```

| Crate | Purpose |
| --- | --- |
| `venture_world` | Core world data: units, coordinates, chunks, terrain cells, biomes, liquids, props, and the baked world file format. Plain data that every other part of the game shares. |
| `venture_worldgen` | The world generation engine, and the `bake` tool that writes the world to disk. |
| `venture_navigation` | Where characters can walk (blocked cells per chunk) and pathfinding (A* plus line-of-sight smoothing). |
| `venture_simulation` | Game rules both sides run: movement, speed and input validation. The client uses them to predict, and the server to decide. |
| `venture_protocol` | The messages between client and server, their encoding (postcard), size limits, and shared validation rules for names, chat and tokens. |
| `venture_content` | The content pack: appearance and NPC definitions, loading with every problem reported by file, script and dialogue checks (with the `scripts` feature), the client manifest, and the `check` tool. |
| `venture_server` | The authoritative game server: its config file, the gate for client messages, sessions and profiles, gameplay, NPCs, private NPC conversations (Yarn Spinner), the Rhai script sandbox, content hot reload, and the HTTP/WebSocket host. |
| `venture_client` | The game client: a view of the server's game, with rendering, input, prediction, actions and chat commands, chat, the dialogue window and debug tools. |

**Rule:** only the client and the server depend on Bevy. The shared crates
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
  main.rs            the app: adds one plugin per concern
  camera.rs          follow and free cameras, and their controls
  content/           the content pack's client files, downloaded and kept up to date
  network/           the connection to the server
    mod.rs             protocol flow and connection state (FromServer / ToServer)
    connection.rs      the transport: WebSocket, or in-memory for --local
    http.rs            downloading files from the server
    launch.rs          which server to use
    token.rs           the login token
    local_server.rs    --local mode: the game server inside the client
  world/             the world as the client shows it
    source.rs          loading the manifest and chunk files
    streaming.rs       chunks around the camera: data, meshes, unloading
    terrain_mesh.rs    terrain and liquid meshes
    terrain_material.rs + shaders/terrain.wgsl   the terrain's shader
    props.rs           tree, rock and bush meshes
    query.rs           ground height, raycasts, blocked cells
    lighting.rs        sun, sky and ambient light
  characters/        characters in the world
    model.rs           the placeholder character and its animation
    player.rs          your character: prediction, steering, following, correction
    remote.rs          other players and NPCs, interpolated between snapshots
    nameplates.rs      names above characters, with orbs on the target and speech bubbles
    speech.rs          what characters said recently (shown in the bubbles)
  interaction/       the player acting on the world
    actions.rs         the things a player can do, however they ask (click, chat command, later keys)
    orders.rs          ordering the character about: run, follow, walk over to talk
    targeting.rs       what's hovered and selected
    clicks.rs          left clicks: select, follow, talk, or run there
    move_marker.rs     the glowing move marker
  ui/                the interface
    toolbar.rs (the window buttons), chat.rs, dialogue_window.rs, inventory.rs
    (the bag), journal.rs (quests), shop.rs, name_prompt.rs,
    target_frame.rs, connection_overlay.rs, cursor.rs, text_input.rs,
    tooltip.rs, widgets.rs (shared pieces such as the close button)
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
