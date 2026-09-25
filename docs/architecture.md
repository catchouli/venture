# Architecture

## Crates

```
venture_client ─┬─►  venture_worldgen   ──►  venture_world
  (Bevy)        └─►  venture_navigation ──►  (no engine, no dependencies)
```

| Crate | Purpose |
| --- | --- |
| `venture_world` | Core world data: units, coordinates, chunks, terrain cells, biomes, liquids, props. Plain data that every other part of the game shares. |
| `venture_worldgen` | The world generation engine: deterministic procedural terrain, rivers, lakes, biomes, settlements, roads and props, plus the overlay system for authored content. |
| `venture_navigation` | Where characters can walk (blocked cells per chunk) and pathfinding (A* plus line-of-sight smoothing). Used by the client for click-to-move, and later by the server to check movement. |
| `venture_client` | The game client, built on [Bevy](https://bevy.org): chunk streaming, terrain meshing and shading, props, the player's character, click-to-move, camera, cursors, debug tools. |

**Rule:** only `venture_client` depends on Bevy. The world crates stay free of
the engine, so the future server can use them, and they are easy to test.

## Planned crates

| Crate | Purpose |
| --- | --- |
| `venture_simulation` | Game rules both sides run: movement, speed limits, validation. The client uses them to predict and the server to decide. |
| `venture_protocol` | Messages between client and server, and how they're encoded. |
| `venture_content` | Loading, checking and hot-reloading the content pack: data (RON), dialogue (Yarn Spinner) and scripts (Rhai). See [content-authoring.md](content-authoring.md). |
| `venture_server` | The authoritative game server. It also hosts the web client and the content pack for LAN play. |

## Client and server

The server is authoritative, and the client predicts the player's own
movement so it feels instant. The server streams the world settings and
content to clients. The full design is in [client-server.md](client-server.md),
and the build order in [plan-client-server.md](plan-client-server.md).

For now the client still generates and simulates everything itself. Chunk
meshing only uses chunk data (and neighbouring chunks' data), never the
generator directly, so switching to server-provided data won't change the
rendering code.

## Client structure

`venture_client` is organised as Bevy plugins, one per job:

| Module | Job |
| --- | --- |
| `world` | Owns the world generator resource; sets up the sun, ambient light and sky. |
| `chunk_streaming` | Loads chunk data around the player, builds meshes when a chunk's neighbours are ready, and unloads distant chunks. Work runs on Bevy's async task pool. |
| `terrain_mesh` | Turns chunk data into smooth terrain meshes and liquid surfaces. |
| `terrain_material` + `shaders/terrain.wgsl` | The terrain's shader: procedural surface detail, rock on steep slopes, subtle bumps. |
| `prop_mesh` | Builds tree, rock and bush meshes and merges each chunk's props into one mesh. |
| `palette` | Every colour in the world. |
| `character` | The placeholder human character (1.8 m tall), built from simple shapes with jointed limbs, and its procedural run and idle animation. |
| `player` | The player's character: spawning it and running it along its route. |
| `click_to_move` | Left-click: find the clicked ground point, plan a route, drop a move marker. |
| `move_marker` | The glowing crest and ground ring left at the destination. |
| `camera` | The camera: follows the character, or flies freely (F1). Also sets where chunks load. |
| `cursor` | The custom gold pointer and the sword cursor (for attacking, once targets exist). |
| `terrain_query` | Ground height, raycasts onto the ground, and blocked cells, from loaded chunk data. |
| `procedural_image` | Helpers for drawing placeholder images in code. |
| `debug_hud`, `screenshot`, `dev_settings` | Development tools. |
| `coords` | The single place world coordinates map to Bevy's 3D axes. |

## Platforms

- **Web:** WebAssembly with WebGL2, built with Trunk. The main constraint is
  that there are no background threads, so heavy work must be spread across frames.
- **Desktop:** native builds for Windows, macOS and Linux.
- **Mobile:** supported by Bevy. It will need touch input and an eye on performance.
