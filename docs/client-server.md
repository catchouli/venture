# Client and server

> Milestone 1 of this design is implemented: the baked world, the
> authoritative server with LAN hosting, automatic profiles, movement
> prediction and chat. Content streaming, client scripts and dialogue are
> still to come (see [plan-client-server.md](plan-client-server.md)).

## The rule: the server decides

The **server is authoritative**. Anything that matters (where characters
are, items, inventory, trading, damage, health, experience, skills, quest
progress) happens on the server and is then sent to clients. A client only
*asks* ("I want to move there", "I want to buy this", "attack that") and
*shows* what the server tells it. A modified client can therefore never
give itself items, damage or speed.

The client is responsible for presentation (rendering, sound, animation,
UI, the camera) and for *predicting* the results of the player's own
actions, so the game feels instant (see movement below).

## Layers of the game

```
┌──────────────────────── Content pack (hot-reloadable) ────────────────────────┐
│ RON data: NPCs, items, shops, spawns, loot   Yarn: dialogue   Rhai: behaviour │
└───────────────────────────────────────────────────────────────────────────────┘
                 ▲ calls a small, stable game API; never engine internals
┌──────────────────────────────── Core engine (Rust) ────────────────────────────┐
│ world generation · navigation · movement rules · combat maths · inventory     │
│ persistence · networking · validation · script and dialogue runtimes          │
└───────────────────────────────────────────────────────────────────────────────┘
```

- The **core engine** is Rust. It owns the rules that must be fast, safe or
  consistent: movement, collision, combat calculations, inventory
  transactions, saving.
- The **content pack** is data, dialogue and scripts. It says *what* exists
  and *how it behaves*: this NPC sells these items, this quest starts when you
  talk to her, this monster drops iron ore. Behaviour goes into the content
  pack wherever it reasonably can, so most fixes and new content are just a
  content reload, with no rebuild or restart.
- Scripts call a deliberately small **game API** (`give`, `take`,
  `start_quest`, `spawn`, `say`, and so on). This is the line between the
  engine and content.

## Crates

```
venture_world        plain world data
venture_worldgen     world generation
venture_navigation   walkability and pathfinding
venture_simulation   NEW: shared game rules (movement step, speed limits, validation)
venture_protocol     NEW: messages between client and server
venture_content      NEW: content pack types, loading, validation, hot reload
venture_server       NEW: the server binary
venture_client       the Bevy client
```

`venture_simulation` holds rules that **both** sides run: the client runs
them to predict, the server runs them to decide. Having one copy of the
code means prediction and authority agree.

## Movement: predicted on the client, checked by the server

Movement must feel instant, so waiting for the server before moving isn't
acceptable, even on a LAN. The standard answer is **client-side prediction
with server authority**, and it avoids trusting the client:

1. The player clicks (or holds WASD). The client **starts moving at once**,
   using the shared movement code, and sends the *intent* to the server:
   "path to (x, y)" or "steering in direction d at tick t".
2. The server runs the **same** movement code on the same terrain and
   obstacle data, from where *it* thinks the player is. It never accepts
   positions from the client, only intents, so speed hacks and
   teleporting are impossible by design.
3. The server sends the authoritative position back regularly. Normally it
   matches the prediction and nothing visible happens. If it differs (packet
   loss, or a cheat attempt), the client smoothly corrects towards the
   server's position, snapping only if the error is large.
4. **Other** players and creatures aren't predicted. They're shown slightly
   in the past, interpolating between the last two server updates, which
   looks smooth.

**As implemented:** the client and server share the movement code
(`venture_simulation`) and the baked obstacle data. When the prediction and the
server's position differ by more than a few metres while moving, the client
snaps to the server. Once both have stopped, it glides the last few
centimetres. On a LAN this is invisible. For play over the internet,
the standard next step is numbering each command, so the client can replay its
unacknowledged inputs on top of each server update.

So the answer to "client or server?" is both: the client *predicts* for
smoothness, and the server *simulates* for correctness. Because the server
simulates rather than checking reported positions, there's no separate
"is this too fast?" check to get right. Things like
`max_speed × elapsed time` are still asserted as a safety net.

Click-to-move suits this approach particularly well, because one small
message ("go to x, y") describes a whole run.

## The world is baked ahead of time

World generation is a **build step**, not something done while playing:

1. **Bake:** a command-line tool runs the generator, applies authored overlay
   stamps, and writes the finished world to disk: a manifest (world
   settings, generator version, bake date) plus one file per chunk with its
   heights, cells and props. Because this happens offline, the generator can
   afford to be slow and thorough (for example, erosion simulation later).
2. **Server:** loads the baked world. Collision and navigation data
   (`ChunkObstacles` now, the multi-layer nav grid later) are built from it at
   load time, so pathfinding and movement checks use exactly the baked terrain.
3. **Clients:** download chunk files from the server over HTTP as the player
   moves (small and compressed, and cached by the browser), then mesh them as
   they do now. The desktop build can ship with the whole world file instead.

Why bake:

- Server and clients see **exactly the same world**, with no dependence on
  floating-point maths being identical on every platform.
- Browsers don't spend time generating terrain, so streaming is smoother.
- Authored edits (overlay stamps, and later hand-sculpted terrain) are baked
  in, so there's one source of truth.
- The full world is too large to put in the initial web download (tens of
  megabytes compressed), so it's streamed chunk by chunk rather than built in.

**Size estimate:** heights and liquid levels are stored as whole centimetres
(`i16`), and cells as a few bytes each, so a chunk is about 30 KB before
compression. The whole 4 × 4 km world is about 120 MB on the server's disk,
and much smaller compressed. A client only ever downloads the chunks near
its player.

**Changes while running:** when authored content changes, the affected
chunks are re-baked (a fraction of a second each) and clients are told to
fetch the new versions.

## Content: streamed from the server

The client needs content for display: NPC names and appearances, item
icons and descriptions, dialogue text, UI strings, and client-side scripts
for presentation. On joining, the client gets a manifest of content files
with hashes, downloads what it's missing or what changed, and (later)
caches it. When the server hot-reloads content, it sends the changed files
to connected clients.

## Scripts on both sides, and whether client scripts are safe

Rhai (and RON and Yarn) are pure Rust and run on both the server and the
browser client.

- **Server scripts** implement game behaviour: quests, NPC reactions, events.
  They're trusted (we wrote them) and still run with limits, so a buggy script
  can't hang the server.
- **Client scripts** are for presentation only: UI behaviour, visual effects,
  how an NPC's idle animation plays. They never decide anything that
  matters. The server would ignore it anyway.

**Is it safe to stream scripts from the server to clients?** Yes, if the
client's scripting is set up as a strict sandbox:

- Rhai has **no built-in access** to files, the network, the clipboard,
  processes, or the browser page. A script can only call functions the
  client explicitly registers, and there's no "unsafe" escape hatch.
- The client registers **only presentation functions**: show text, play an
  animation or effect, update a UI element. Nothing that opens URLs, runs
  other code, reads storage (including the login token), or sends
  arbitrary network messages.
- The engine is configured with **hard limits** on operations, call depth,
  and string, array and map sizes, so a hostile script can at worst waste its
  own time budget. It can't freeze the browser or eat memory.
- Scripts are compiled with the script engine, never `eval`'d as
  JavaScript, and loaded modules come only from the content pack.

With those rules, a hijacked server sending malicious client scripts could
do no more than it could by sending malicious *data* (for example, silly
text in dialogue). It couldn't touch the player's machine. Later we could
also **sign** content packs, so clients only accept content signed by our key.

## Dialogue: Yarn Spinner for an RPG

Yarn is best known from dialogue-heavy games (Night in the Woods, A Short
Hike, DREDGE), but it fits an RPG well, because it only does dialogue and
leaves the rest to us:

- Writers get branching conversations, variables and conditions ("only offer
  this line if the quest is active") in a readable, screenplay-like format.
- **Commands** (`<<start_quest "iron_for_hilda">>`, `<<open_shop ...>>`) call
  our game API, so quests, items and shops stay in the engine and Rhai.
- Every line gets an ID, which is the groundwork for translating the game.

In an online game, conversations **run on the server**: it steps through
the Yarn story, sends the client the lines and the choices, receives the
chosen option, and runs any commands itself. Nothing a client sends can skip
a condition or run a reward command.

## Player profiles and joining (vertical slice)

Deliberately simple, so people can get into the game quickly:

- On first visit, the browser creates a random **token** (a long random ID)
  and keeps it in `localStorage`. The desktop client keeps it in a file.
- On connecting, the client sends its token. If the server has a profile for
  it, that profile is loaded into memory. If not, the server **creates a new
  temporary profile** (with a generated name) at the spawn point.
- A profile holds: name, position, stats, experience, skills, inventory and
  quest state. The server saves it when the player leaves, and every
  minute or so while they play.
- Storage: one RON file per profile in the server's data folder, named by a
  hash of the token (so the files don't reveal tokens). This is easy to
  inspect and edit while testing. We can move to SQLite when we need to.
- **Known limits, accepted for now:** clearing browser data loses the
  character. Anyone who copies a token can play that character. There are
  no passwords. This is fine for LAN testing, and must be replaced by real
  accounts before any public server.

## Rules and following

The server's tunable **game rules** live in its config file (`server.ron`,
the `player` block), and are sent to each client in `Welcome`, so the client
predicts by the same rules. The first rule is `max_move_distance`: the
furthest one click can send a character (the server clamps any further
`MoveTo` to that distance in the same direction), and the furthest a
character keeps following someone.

**Following:** the client sends `Follow { target }` for a selected player.
The server checks the target exists and is within reach, then keeps
re-planning the follower's route as the target moves (shared code in
`venture_simulation`, which the client also uses to predict). Any other
movement command (`MoveTo`, `Steer`, `Stop`) ends following. Deselecting on
the client sends `Stop`.

**Selecting** a target is purely client-side, since it changes nothing on its
own. Actions on the target (following now, attacking later) are commands the
server validates.

## Chat

The client sends `Chat { text }`. The server's gate rate-limits it (a
burst of 5, then one every 2 seconds), removes control characters, trims it
and caps its length, then broadcasts `Chat { from, name, text }` to everyone
in the game. The sender's own message appears when the server's broadcast
comes back, so every player sees exactly the same chat. Server messages
(joins, leaves, warnings) arrive as `Notice { text }`.

## Connection problems

The live game always needs the server. If the client can't connect, or loses
the connection, the game fades out under a dark overlay with a red message
("Disconnected from the server, reconnecting…"), and keeps retrying. When it
reconnects, the overlay lifts and the player carries on from the server's
saved state.

## Offline development: an in-process server

For working on terrain, art and features without starting a separate server,
the **desktop client** has a development mode (`--local`) that runs the real
server code *inside the same process*. Client and server talk through an
in-memory connection instead of a WebSocket. It's the same code path as live
play, just without the network. The automatic screenshot tools use this mode.

## Joining

- The first time a browser joins, it asks "What's your name?", with a box
  pre-filled with a random fantasy name. After that, the token logs in
  automatically.
- Milestone 1 supports both the browser and the desktop client.
- Networking is our own thin layer: WebSocket plus our messages and
  prediction. We understand and control all of it, and it's enough for
  click-and-path movement. We can adopt a bigger framework later if we need one.

## Hosting on a LAN

One command runs everything:

```sh
cargo run --release -p venture_server
```

The server:

- serves the **web client** (the built `dist/` files) over HTTP,
- serves **baked world chunks** and the **content pack** over HTTP,
- accepts game connections over **WebSockets** on the same port,
- prints its LAN address, such as `http://192.168.1.20:8080`, for others on
  your network to open in their browser.

WebSockets run over TCP, which isn't ideal for fast-paced games on the
open internet, but they work in every browser and are perfectly smooth on a
LAN. The transport sits behind the protocol crate, so it can later be
swapped for WebTransport (UDP-like) without touching game code.

The server's world simulation runs at a fixed **tick rate** (20 per second
to start), independent of anyone's frame rate.
