# Client and server

> Implemented: the baked world, the authoritative server with LAN hosting,
> automatic profiles, movement prediction, chat (milestone 1), and the content
> pack with NPCs, server-side scripts, hot reload and content streaming
> (milestone 2). Client scripts and dialogue are still to come (see
> [plan-client-server.md](plan-client-server.md)).

## The rule: the server decides

The **server is authoritative**. Anything that matters (where characters
are, items, inventory, trading, damage, health, experience, abilities, quest
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

## Entities

Players and NPCs are both **entities** on the wire, identified by an
`EntityId`. The server announces each one with `EntityAppeared { id, name,
kind, position }` (where `kind` is `Player` or `Npc { appearance }`), moves
them all in the same `Snapshot`, and removes them with `EntityLeft`. So chat
(`Chat { from, .. }`), speech bubbles, selecting and following work the same
for NPCs as for players.

Talking to an NPC is `Interact { target }`. If the player is within 3 m, the
conversation starts straight away. If not, the server walks them over (as
with following) and starts it on arrival. The client predicts the walk.
`Interact` on a player, or `Follow` on an NPC, is refused with a `Notice`
explaining why.

## Content: streamed from the server

The client needs content for display: NPC appearances now, and later item
icons and descriptions, dialogue text, UI strings and client-side scripts.

- The server publishes the client's part of the content pack at
  `/content/manifest.ron`: each file with its SHA-256 hash, plus a version
  that changes whenever any file does. The files are served at
  `/content/<path>`. Only files in the manifest can be fetched: NPC
  definitions and scripts are never served.
- On joining, the client downloads the manifest and the files it lists.
- When the server hot-reloads content, it sends `ContentChanged { version }`.
  Clients download the new manifest and only the files whose hash changed,
  then redraw what changed (NPCs are restyled in place).
- In `--local` mode the client reads the files straight from the server
  running inside it.

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

**How it works now** (`crates/server/src/dialogue.rs`): each player's
conversation is private, so several players can talk to one NPC at once. The
server runs the Yarn story until the player has to choose, and sends one page:
`Dialogue { npc, lines, options }`. The client shows it in the dialogue
window, and sends `ChooseDialogueOption { index }` for the reply clicked (an
index that isn't on offer is ignored). A page with no options is the end.
`EndDialogue` closes a conversation early, and the server sends
`DialogueEnded` when it ends one itself (the player walked more than 6 m away,
or the NPC went). Each page is limited in how many steps it may run, and the
content check refuses loops that would never pause, so dialogue can't stall
the server.

This is separate from what NPCs say **out loud** (`npc.say` in scripts),
which everyone within 40 m sees in chat and above the NPC's head.

## Player profiles and joining (vertical slice)

Deliberately simple, so people can get into the game quickly:

- On first visit, the browser creates a random **token** (a long random ID)
  and keeps it in `localStorage`. The desktop client keeps it in a file.
- On connecting, the client sends its token. If the server has a profile for
  it, that profile is loaded into memory. If not, the server asks for a new
  character (`CharacterNeeded`, offering a name and the classes), and the
  player chooses a name, a class, a body type and their looks
  (`CreateCharacter`, with `Features`: height, build and head size from -10
  to 10, a hairstyle, a beard, and a hair colour and skin tone from the
  content's palettes). The gate brings the sliders within range and turns
  away overlong names; the server then puts back anything the content
  doesn't offer (checked again at each login, as the content can change).
  **Names are unique**, ignoring case: a taken name is turned away
  (`CharacterRejected`) and the player picks another.
- A profile holds: name, position, class, body type, looks, level, experience,
  stats, unspent stat and ability points, Miasma, inventory and
  quest state. Fields added over time have defaults, so older profiles still
  load (and their coins become Miasma). The server saves it when the player leaves, and every
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
own. Actions on the target (following, talking and attacking) are commands
the server validates.

## Chat

The client sends `Chat { text }`. The server's gate rate-limits it (a
burst of 5, then one every 2 seconds), removes control characters, trims it
and caps its length, then broadcasts `Chat { from, name, text }` to everyone
in the game. The sender's own message appears when the server's broadcast
comes back, so every player sees exactly the same chat. Server messages
(joins, leaves, warnings) arrive as `Notice { text }`.

## Inventories

Each player carries an **inventory** of 50 slots and their **Miasma** (money),
which the server owns and saves in their profile. A slot is empty or holds
some of one item; stackable items share one slot, and others take a slot each.

On the server, an inventory can only change through `Inventory::apply`, which
takes a `Change` (items and Miasma to take and to give) and makes all of it or
none of it. So a purchase can't take the Miasma and then fail to hand over the
goods, nothing can be duplicated, and counts never go negative, whichever
module asks for the change. Whenever an inventory changes, the server sends
the player the whole thing (`Inventory { slots, miasma }`), which is small, and the
client's inventory tab shows it. Item names, descriptions and icons come from
the content pack's client files.

## Shops

A shop opens when an NPC's dialogue runs `<<open_shop ...>>`: the server
ends the conversation and sends `ShopOpened { npc, name, stock }`. The client
shows the shop beside the inventory. `Buy { index, count }` and `Sell { slot, count }`
(counts from 1 to 100, checked by the gate) each become one inventory
`Change`, Miasma for goods or goods for Miasma, so a trade happens completely
or not at all. The server answers every trade with a `Notice` saying how it
went ("You buy: Bread (2) for 10 Miasma.", "You don't have enough Miasma for
that.").
Shops pay half an item's value, and won't buy items worth nothing (such as
quest items). The shop closes with `CloseShop`, or when the player walks more
than 6 m away (`ShopClosed`). The server keeps the stock as it was when the
shop opened, so the player always buys what they were shown.

## Quests

Each player has a **quest log** (which quests they've started, and the stage
each has reached), which the server owns and saves in their profile. Dialogue
moves quests on with `<<set_quest ...>>`. On the server a quest log only
changes through `QuestLog::advance`, which only moves forward, so going back a
stage (and claiming a reward again) is impossible. Whenever it changes, the
server sends the player their journal: `Quests { quests }`, with each quest's
name, the journal text for its current stage, and whether it's complete. The
text comes from the server, so clients never see quest stages they haven't
reached. The NPC whose conversation moved a quest on can react through its
script's `on_quest_stage` hook.

## Characters, levels and admins

Each player's **character** (class, body type, level, experience, ability
scores and unspent points) lives on the server and changes only through its
own methods, which follow the rules in `venture_rpg`. Whenever it changes,
the server sends the player their character sheet (`Character`), including
the secondary stats it worked out (health, mana, armour class, attack bonus,
attack speed, damage), their abilities, the abilities their class can learn,
what they have equipped, and their level and experience in each skill
(gathering and making), so the client never repeats the rules. Players
spend points with `SpendStatPoint { stat }` and `LearnAbility { ability }`;
the server checks the level, the points and the class, and says in a
`Notice` if it can't.

**Equipment** lives in its own component, changed only in `equipment.rs`.
`Equip { slot }` (an inventory slot) and `Unequip { slot }` (an equipment
slot) swap items between the inventory and the slots as one inventory `Change`, so
nothing is lost or duplicated; an item filling several slots (a robe: body
and legs) replaces whatever is in any of them. What's worn feeds the
character's secondary stats (armour, the weapon's damage and speed).

**Using items** (eating bread) happens only in `consumables.rs`. `UseItem {
slot }` uses the item in an inventory slot: its `on_use` effect heals
(through `Health::heal`, by an amount from `venture_rpg::Healing`), and one
is taken with an inventory `Change`, only if it did something. The player
gets a `Notice`, everyone nearby a `Healed`, and they wait `item_cooldown`
seconds (from `server.ron`) before using another.

Other players see a player's level and look in `EntityAppeared` (`Player {
level, look }`), and `PlayerChanged` when either changes; `celebrate` marks
a level going up, and everyone nearby sees the level-up effect. A player's
look is their body type, their looks (`features`), how each item they wear
looks (`worn`, the placeholder shapes), and the outfit parts it puts on them
(`outfit`).

**Admins** are listed by character name in `server.ron`. Their commands
(`Admin(SetLevel | Reset | SetClass | SetBody)`: `/setlevel`, `/reset`,
`/setclass` and `/setgender`) go through the gate like any other,
then to `admin.rs`, which checks the player really is an admin before doing
anything; anyone else is told only admins can use them.

## Creatures and fighting

**Creatures** are a third kind of entity (`EntityKind::Creature { level,
look }`). Spawners in `creatures.rs` create and remove them. That file also
decides what they do: wander, pick a fight, chase, give up. Snapshots carry a
creature's health as a fraction (`EntityState.health`) for health bars.

**Attacking:** the client sends `Attack { target }`. The server refuses
players ("Players can't fight each other"), NPCs, and targets further away
than a move may go (`max_move_distance` in `server.ron`). Otherwise the player's character chases the target into melee range
(2.6 m) and swings at its attack speed. Any movement order stops it.

**Every blow is resolved in `combat.rs`** with `venture_rpg`: a d20 roll plus
the attack bonus against armour class, then damage, doubled dice on a natural
20. The result goes to everyone within 60 m as `Attacked { attacker, target,
result: Miss | Hit(n) | Critical(n) }`. The client shows it (swing, floating
number, flash) and decides nothing. Health changes only through `Health`'s
methods. A player's own health comes as `Vitals { health, max_health }`
whenever it changes.

**Deaths:** a creature that dies is announced with `Died { id }`, lies there
two seconds, then leaves. Its killer gets its experience and maybe a level
(full health, and the level-up effect for everyone). A player knocked out
wakes at the nearest respawn point with full health. Creatures fighting them
give up and heal. Out of combat for 6 seconds, health comes back by a
twentieth of the maximum every 3 seconds.

## Abilities, mana and the hotbar

**Using an ability:** the client sends `UseAbility { ability, target }`. The gate
passes it to `abilities.rs`, the only code that uses abilities. It checks four
things: the player knows the ability, it isn't cooling down (`Cooldowns`), they
have the mana (`Mana`, in `combat.rs`), and the target is a living creature.
Then it tells everyone within 60 m with `AbilityUsed { user, ability, target }`,
which clients play (a swing, a bolt flying off, a glow, a sound):
- A weapon ability becomes a `Strike` with the ability's dice, resolved by
  `combat.rs` like any attack. The player then goes on auto-attacking.
- A spell becomes a bolt in flight on the server, which strikes when it
  arrives (distance ÷ speed later).
- Out of range, the player first runs up (`PendingAbility`, moved by
  `gameplay.rs`).
- Heals come with `Healed { id, amount }`.

`Attacked` carries the ability used, if any, so clients can show it.
The client greys out abilities that are cooling down or too expensive, and
explains those refusals without asking. The server checks everything again
regardless.

**Resting** (the Rest ability) sits the player down (`Resting`). `sit_down` in
`gameplay.rs` stops them. Any movement order, or any blow landed or taken,
stands them up. Snapshots carry `resting` so everyone sees them sit.

**Mana** comes with health in `Vitals { health, max_health, mana, max_mana }`.

**The hotbar** is only a layout: ten slots of abilities or actions. It's saved
with the profile and sent as `Hotbar { slots }` on joining. The client sends
the whole bar after each change (`SetHotbar`), and `hotbar.rs` keeps only
abilities the player knows (sending back the corrected bar if it had to change
anything). New abilities go in the first empty slot, and forgotten ones come off.

## Jumping

For now jumping is for show: movement stays on the ground, so it doesn't
change where a character can go. Later it will be possible to jump onto
things, but the game won't require it. The client jumps at once and sends `Jump`; the server
allows one jump at a time (`JUMP_SECONDS`, in `venture_simulation`), stands a
resting player up, and tells everyone else within 40 m (`Jumped { id }`).

## Actions and chat commands

The things a player can do are **actions**, defined in one place on the
client (`crates/client/src/interaction/actions.rs`). Every way of doing
something asks for an action: clicking a selected character, typing a chat
command, and later an action bar or key bindings. So they all behave the
same, and a new action is added once.

A chat message starting with `/` names an action and is never sent as chat:
- `/follow`: follow the selected player;
- `/talk`: talk to the selected NPC;
- `/attack`: attack the selected creature (or click it again, or use it from the hotbar);
- `/wave`: wave hello (the server shows it to everyone within 40 m, as an
  `Emoted` message; emotes share chat's rate limit);
- `/town`: go back to the starting town's square;
- `/help`: list the commands (shown by the client itself).

Actions become ordinary protocol commands (`Follow`, `Interact`, `Attack`,
`ReturnToTown`), which the server validates like any other. Actions on the
selected target are sent even when they can't work (`/follow` with an NPC
selected), so the server decides and explains in a `Notice`; the client only
predicts the ones that will work. An unknown command gets a message pointing
to `/help`.

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
