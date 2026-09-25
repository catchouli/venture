# Plan: from single-player prototype to playing together on a LAN

The design is in [client-server.md](client-server.md). This is the order
we'll build it in. Each milestone ends with something playable.

## Milestone 1: play together on a LAN

**Goal:** run `cargo run --release -p venture_server`, open the printed
address in several browsers on the network, and see each other running
around the same world. Characters come back where they left off.

### 1. Groundwork

- **Identical terrain everywhere:** use the pure-Rust `libm` crate for `exp`,
  `powf`, `hypot` and friends in `venture_worldgen`. Add a test that
  fingerprints a few chunks (a hash of their heights and cells), and check it
  gives the same result natively and in WebAssembly.
- **`venture_simulation` crate:** move the movement rules out of the client
  (route following, WASD steering with sliding, run speed) into shared,
  engine-free code with unit tests. The client keeps working exactly as it
  does now, calling the shared code.
- Add a **generator version** constant to `venture_worldgen`, increased
  whenever the generated world changes.

### 2. Protocol (`venture_protocol`)

- Message types, serialised with `serde` and
  [`postcard`](https://crates.io/crates/postcard) (compact, and works in WebAssembly):
  - Client to server: `Hello { token, protocol_version }`,
    `MoveTo { x, y }`, `Steer { direction }`, `Stop`.
  - Server to client: `Welcome { your_id, name, world_settings, generator_version, position }`,
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
- **World:** the server generates the chunks around each player (for
  navigation and movement checks), using the same streaming radius idea as
  the client.
- **Movement:** applies `MoveTo` / `Steer` intents with `venture_simulation`,
  so positions only ever come from the server's own simulation.
- **Profiles:** load on `Hello` (or create a temporary profile with a
  generated name), save on disconnect and every 60 seconds, as RON files in
  `server_data/profiles/`.
- Prints the LAN address(es) at startup. Command-line options for port and
  data folder.

### 4. Client networking

- **Connection layer** with one interface and two implementations: the
  browser's WebSocket (through `web-sys`) and a native WebSocket client on
  desktop.
- **Token:** created on first run, stored in `localStorage` (browser) or a
  file (desktop).
- On `Welcome`, set up the world from the server's settings and spawn the
  player where the server says.
- **Prediction and correction:** keep moving instantly as now, send intents,
  and correct smoothly towards the server's position when they differ.
- **Other players:** spawn a character for each, move them by interpolating
  between snapshots, and show name labels.
- A simple "Connecting…" / "Disconnected, retrying" overlay.

### 5. Check it works

- Two browser tabs plus the desktop client, all seeing each other move smoothly.
- Restart the server: everyone reconnects to their saved position.
- Join from another device on the LAN.
- Send forged messages (a far-away `MoveTo`, a huge steer vector) from a
  test client: the server doesn't let the character move any faster than
  walking there.
- Automated tests: protocol round trips, simulation rules, profile save and load.

## Milestone 2: content pack v0

- `venture_content` crate: RON definitions for items and NPCs, a
  `content/` folder, and validation (`cargo run -p venture_content -- check`).
- The server spawns NPCs from content and sends them to clients.
- The Rhai runtime on the server, with the game API boundary and limits.
  Hot reload by watching `content/` with the
  [`notify`](https://crates.io/crates/notify) crate.
- **Content streaming:** a manifest with hashes; clients download what they
  need and receive updates when the server reloads.

## Milestone 3: talk, trade, carry

- **Yarn dialogue** on the server; a dialogue window on the client.
- **Inventory** (server-authoritative) and a first **shop** NPC in the
  first town.
- First steps of a quest, written with RON + Yarn + Rhai, end to end.

## Later

Combat and targeting (with the sword cursor), gathering skills (felling trees
from the props the generator already places), creatures, real accounts, and
WebTransport for play over the internet.
