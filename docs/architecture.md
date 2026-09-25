# Architecture

## Crates

```
venture_client ──► venture_protocol, venture_simulation, venture_navigation, venture_world
   (Bevy)     └──► venture_server (desktop only, for --local mode)

venture_server ──► venture_protocol, venture_simulation, venture_navigation, venture_world
   (headless Bevy, tokio/axum)

venture_worldgen ──► venture_world     (the bake tool; nothing at runtime depends on it)
```

| Crate | Purpose |
| --- | --- |
| `venture_world` | Core world data: units, coordinates, chunks, terrain cells, biomes, liquids, props, and the baked world file format. Plain data that every other part of the game shares. |
| `venture_worldgen` | The world generation engine, and the `bake` tool that writes the world to disk. |
| `venture_navigation` | Where characters can walk (blocked cells per chunk) and pathfinding (A* plus line-of-sight smoothing). |
| `venture_simulation` | Game rules both sides run: movement, speed and input validation. The client uses them to predict, and the server to decide. |
| `venture_protocol` | The messages between client and server, their encoding (postcard), size limits, and shared validation rules for names, chat and tokens. |
| `venture_server` | The authoritative game server: the gate for client messages, sessions and profiles, gameplay, and the HTTP/WebSocket host. |
| `venture_client` | The game client: a view of the server's game, with rendering, input, prediction, chat and debug tools. |

**Rule:** only the client and the server depend on Bevy. The shared crates
are engine-free, so both sides can use them and they're easy to test.

## Planned crates

| Crate | Purpose |
| --- | --- |
| `venture_content` | Loading, checking and hot-reloading the content pack: data (RON), dialogue (Yarn Spinner) and scripts (Rhai). See [content-authoring.md](content-authoring.md). |

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

## Client structure

`venture_client` is organised as Bevy plugins, one per job. Data from the
server flows in through `network` as `FromServer` messages, and commands
flow out as `ToServer` messages. No other module touches the connection.

| Module | Job |
| --- | --- |
| `launch` | Which server to use (the page's own server on the web; `--connect` or `--local` on desktop). |
| `connection` | The transport: a WebSocket, or the in-memory link in `--local` mode. |
| `network` | The client's end of the protocol: connection state, publishing server events, and sending commands. |
| `token` | The login token, in `localStorage` (web) or `~/.venture` (desktop). |
| `local_server` | `--local` mode: runs the game server inside the client. |
| `world_source` | Loads the world manifest and chunk files, from the server over HTTP or from disk. |
| `chunk_streaming` | Loads chunk data around the camera, builds meshes when a chunk's neighbours are ready, and unloads distant chunks. |
| `terrain_mesh`, `terrain_material` + `shaders/terrain.wgsl` | Terrain and liquid meshes, and the terrain's shader. |
| `prop_mesh` | Tree, rock and bush meshes, merged per chunk. |
| `character` | The placeholder human character (1.8 m tall) and its procedural animation. |
| `player` | The player's own character: prediction, WASD steering and correction towards the server. |
| `remote_players` | Other players, interpolated between server snapshots. |
| `nameplates` | Names floating above characters. |
| `click_to_move`, `move_marker` | Clicking the ground to run there, and the glowing marker. |
| `chat` | The chat box. |
| `name_prompt` | Asking new players for a name. |
| `connection_overlay` | "Connecting…", and the red "Disconnected" overlay. |
| `text_input` | Simple text fields, and keyboard focus (which pauses keyboard game controls). |
| `camera`, `cursor` | The follow and free cameras; custom cursors. |
| `terrain_query` | Ground height, raycasts and blocked cells from loaded chunk data. |
| `palette`, `coords`, `mesh_data`, `procedural_image` | Colours, the coordinate mapping, and helpers. |
| `debug_hud`, `screenshot`, `dev_settings` | Development tools. |

## Platforms

- **Web:** WebAssembly with WebGL2, built with Trunk. The main constraint is
  that there are no background threads, so heavy work must be spread across frames.
- **Desktop:** native builds for Windows, macOS and Linux.
- **Mobile:** supported by Bevy. It will need touch input and an eye on performance.
