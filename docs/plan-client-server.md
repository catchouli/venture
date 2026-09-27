# Plan: from single-player prototype to playing together on a LAN

The design is in [client-server.md](client-server.md). This is the order
we'll build it in. Each milestone ends with something playable.

## Milestone 1: play together on a LAN (done)

Everything below is implemented, plus a chat box. The server has end-to-end
tests (`crates/server/tests/game.rs`) covering joining, name rules, movement
speed limits against forged commands, chat, refusal of out-of-order and
malformed messages, double logins, and characters returning where they left
off. It has been played on a LAN, in the browser and on desktop.

**Goal:** run `cargo run --release -p venture_server`, open the printed
address in several browsers on the network, and see each other running
around the same world. Characters come back where they left off.

### 1. Groundwork

- **Baked world format and tool:** a `bake` command (in `venture_worldgen`)
  that generates the world, applies overlay stamps, and writes
  `world/manifest.ron` plus `world/chunks/{x}_{y}.bin`. Chunk files are
  serialised with `postcard`, with heights and liquid levels stored as whole
  centimetres. Loading code lives with the data types, so the server and the
  desktop client can read baked worlds. Tests check that a baked and reloaded
  chunk matches the generated one to within a centimetre.
- A **generator version** in the manifest, so stale bakes are noticed.
- **`venture_simulation` crate:** move the movement rules out of the client
  (route following, WASD steering with sliding, run speed) into shared,
  engine-free code with unit tests. The client keeps working exactly as it
  does now, calling the shared code.

### 2. Protocol (`venture_protocol`)

- Message types, serialised with `serde` and
  [`postcard`](https://crates.io/crates/postcard) (compact, and works in WebAssembly):
  - Client to server: `Hello { token, protocol_version }`,
    `MoveTo { x, y }`, `Steer { direction }`, `Stop`.
  - Client to server also: `ChooseName { name }` (first join only).
  - Server to client: `Welcome { your_id, name, world_id, position }` (or
    `NameNeeded` for a new player),
    `PlayerJoined`, `PlayerLeft`, `Snapshot { tick, positions }`
    (everyone nearby, 20 times a second), and your own authoritative state.
- A protocol version, checked on `Hello`.

### 3. Server (`venture_server`)

- **Headless Bevy app** (Bevy's ECS and scheduler, with no window or
  rendering), ticking at 20 Hz. This is the same style as the client, and
  NPCs, monsters and scripts will need an ECS soon anyway.
- **Networking** on a [`tokio`](https://tokio.rs) +
  [`axum`](https://crates.io/crates/axum) thread, passing messages to the game
  loop through channels:
  - `GET /` and friends serve the built web client (`crates/client/dist`);
  - `GET /ws` upgrades to a WebSocket game connection.
- **World:** loads the baked world and builds the obstacle grids from it.
  Serves chunk files at `GET /world/chunks/{x}/{y}` (compressed, cacheable).
- **Local mode:** the server's game plugin can also run inside the desktop
  client (`--local`), connected through an in-memory transport.
- **Movement:** applies `MoveTo` / `Steer` intents with `venture_simulation`,
  so positions only ever come from the server's own simulation.
- **Profiles:** load on `Hello` (or create a temporary profile with a
  generated name), save on disconnect and every 60 seconds, as RON files in
  `server_data/profiles/`.
- Prints the LAN address(es) at startup. Command-line options for port and
  data folder.

### 4. Client networking

- **Connection layer** with one interface and three implementations: the
  browser's WebSocket (through `web-sys`), a native WebSocket client on
  desktop, and an in-memory connection for `--local` mode.
- **Token:** created on first run, stored in `localStorage` (browser) or a
  file (desktop).
- **First join:** a small "What's your name?" box, pre-filled with a random
  fantasy name.
- **World data from the server:** chunk streaming fetches baked chunks over
  HTTP (or reads the local world file on desktop) instead of generating them.
  The rest of the rendering is unchanged.
- **Prediction and correction:** keep moving instantly as now, send intents,
  and correct smoothly towards the server's position when they differ.
- **Other players:** spawn a character for each, move them by interpolating
  between snapshots, and show name labels.
- **Disconnected overlay:** the game fades out with a red "Disconnected from
  the server, reconnecting…" message, and keeps retrying.

### 5. Check it works

- Two browser tabs plus the desktop client, all seeing each other move smoothly.
- Restart the server: everyone sees the disconnected overlay, then reconnects
  to their saved position.
- `--local` desktop mode plays without a separate server.
- Join from another device on the LAN.
- Send forged messages (a far-away `MoveTo`, a huge steer vector) from a
  test client: the server doesn't let the character move any faster than
  walking there.
- Automated tests: protocol round trips, simulation rules, profile save and load.

## Milestone 2: content pack v0 (done)

Implemented as planned, with two adjustments:
- **Item definitions move to milestone 3**, where inventory gives them a use.
- **Players and NPCs became one kind of network entity** (protocol version
  3), so chat, speech bubbles, selecting and following work for NPCs too.

Also added: NPC placement relative to generated settlements (the baked world
now records its settlements); talking to NPCs (walking over first) with a talk
cursor; four townsfolk with scripts in the starting town. The server tests
cover NPCs appearing, talking from a distance, content changes replacing NPCs,
and NPC speech range. Script sandbox tests cover queued commands, runaway
loops and disabled `eval`.

Original plan:

- `venture_content` crate: RON definitions for items and NPCs, a
  `content/` folder, and validation (`cargo run -p venture_content -- check`).
- The server spawns NPCs from content and sends them to clients.
- The Rhai runtime on the server, with the game API boundary and limits.
  Hot reload by watching `content/` with the
  [`notify`](https://crates.io/crates/notify) crate.
- **Content streaming:** a manifest with hashes; clients download what they
  need and receive updates when the server reloads.

## Milestone 3: talk, trade, carry

- ~~**Yarn dialogue** on the server; a dialogue window on the client.~~ Done:
  private conversations run on the server, one page at a time.
  Still to come: saved Yarn variables and game commands from dialogue.
- ~~**Item definitions** in the content pack, streamed to clients (names,
  descriptions, icons).~~ Done, with placeholder icons.
- ~~**Inventory** (server-authoritative) and a first **shop** NPC in the
  first town.~~ Done: Elsie's bakery, opened from her dialogue.
- ~~First steps of a quest, written with RON + Yarn + Rhai, end to end.~~
  Done: "Old Tomas's Pipe", with a quest journal.

## Later

Combat and targeting (with the sword cursor), gathering skills (felling trees
from the props the generator already places), monsters, real accounts, and
WebTransport for play over the internet.
