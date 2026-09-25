# Client and server

> Design for the next stage. Nothing here is implemented yet; see the plan at
> the end.

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

So the answer to "client or server?" is both: the client *predicts* for
smoothness, and the server *simulates* for correctness. Because the server
simulates rather than checking reported positions, there's no separate
"is this too fast?" check to get right. Things like
`max_speed × elapsed time` are still asserted as a safety net.

Click-to-move suits this approach particularly well, because one small
message ("go to x, y") describes a whole run.

## Terrain and content: streamed from the server

The client should need nothing but itself. On joining, the server sends
everything else:

- **World:** the generator settings, the generator version, and any overlay
  stamps. The client then **generates the terrain itself**, which is
  deterministic, so no chunk data needs to be sent. Changes made while the
  server runs (such as a placed building) are sent as overlay updates. If
  the generator versions don't match, the client refuses to join rather
  than show the wrong world.
- **Content pack:** a manifest of files with content hashes. The client
  downloads what it's missing or what changed, and (later) caches it in the
  browser. When the server hot-reloads content, it sends the changed files to
  connected clients.

The client needs content for display: NPC names and appearances, item
icons and descriptions, dialogue text, UI strings, and client-side scripts
for presentation.

**Determinism across platforms:** the server runs natively and browsers run
WebAssembly. Floating-point `+ − × ÷` and square roots are identical
everywhere, but functions like `exp`, `powf` and `sin` come from different
maths libraries on different platforms and can differ in the last bit. To
keep terrain bit-identical, world generation should use the pure-Rust
[`libm`](https://crates.io/crates/libm) crate for those functions (the same
code on every target). The server still validates movement against its own
terrain, so a mismatch could never be exploited, only cause corrections.

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

## Hosting on a LAN

One command runs everything:

```sh
cargo run --release -p venture_server
```

The server:

- serves the **web client** (the built `dist/` files) over HTTP,
- accepts game connections over **WebSockets** on the same port,
- serves the content pack to clients,
- prints its LAN address, such as `http://192.168.1.20:8080`, for others on
  your network to open in their browser.

WebSockets run over TCP, which isn't ideal for fast-paced games on the
open internet, but they work in every browser and are perfectly smooth on a
LAN. The transport sits behind the protocol crate, so it can later be
swapped for WebTransport (UDP-like) without touching game code.

The server's world simulation runs at a fixed **tick rate** (20 per second
to start), independent of anyone's frame rate.
