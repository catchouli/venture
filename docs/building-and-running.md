# Building and running

## Requirements

- Rust (stable, 1.95 or newer), via [rustup](https://rustup.rs).
- For the web build: the WebAssembly target and [Trunk](https://trunkrs.dev):
  ```sh
  rustup target add wasm32-unknown-unknown
  cargo install --locked trunk
  ```

## The world

The server and clients use a **baked** world: generated once, ahead of
time, and written to the `world/` folder.

```sh
cargo run --release -p venture_worldgen --bin bake [--structures] [folder] [seed] [authoring folder]
```

Bake again whenever world generation changes. The bake reads the authored
world data in `world_authoring/` too (walls, fences and buildings: see
[plan-walls.md](plan-walls.md) and [plan-buildings.md](plan-buildings.md)),
and makes each chunk's navigation mesh tile (see
[plan-collision.md](plan-collision.md)): a whole bake takes about 30
seconds, most of it the tiles. With `--structures` (or `--walls`), only the
chunks walls and buildings touch (and their tiles, and their neighbours'
tiles) are baked again, in a few seconds; the editor's **Save and bake** does
this. It bakes everything if the world there was baked differently.

**A running server (or `--local` game) picks up a new bake by itself:** it
watches the world's manifest (which the bake writes last), loads the world
again in the background once it's settled, and tells players which chunks
changed, so their clients read those again (see "The world, baked again" in
[client-server.md](client-server.md)). The manifest records the
generator version. The bake also draws the world map (`world/map/`: tiles
and place names, see [plan-map.md](plan-map.md)); to draw just the map
again for a world already baked (such as one baked before there were maps):

```sh
cargo run --release -p venture_worldgen --bin bake_map [folder]
```

## The content pack

NPCs, their looks and their behaviour live in `content/` (see
[content-authoring.md](content-authoring.md)). Check it for mistakes with:

```sh
cargo run -p venture_content --features scripts --bin check
```

The server won't start with broken content, and while it runs, it reloads
content whenever a file changes (keeping the old content if the new has
problems).

## The server

```sh
cargo run --release -p venture_server
```

This serves the game, the web client and the world on one port (8080 by
default), and prints the addresses to open.

### The config file

The server reads its settings from `server.ron` in the current folder (or
`--config <file>`). Every setting is optional: leave one out, or the whole
file, and it takes its default. Misspelt settings are reported as errors.

```ron
(
    network: (
        port: 8080,         // for the game, the web client and the world files
        bind: "0.0.0.0",    // "0.0.0.0": your local network; "127.0.0.1": this computer only
    ),
    paths: (
        world: "world",
        profiles: "server_data/profiles",
        client: "crates/client/dist",
        content: "content",
    ),
    player: (
        max_move_distance: 150.0,   // metres one click can send a character
        item_cooldown: 1.0,         // seconds after using an item (eating bread) before using another
    ),
    day: (
        day_minutes: 120.0,         // how long a day and night lasts, in real minutes
        sunrise: 3.0,               // when the sun rises and sets, in game hours (0 to 24)
        sunset: 21.0,
        noon_height: 60.0,          // how high the sun gets at midday, in degrees
    ),
    party: (
        max_members: 8,             // the most players in a party
        invite_seconds: 15.0,       // how long an invited player has to /accept or /decline
        share_distance: 60.0,       // metres from a kill within which members share its experience
    ),
    admins: ["cat"],    // characters with admin powers (/setlevel, /reset, /setclass), by name
)
```

Character names are unique (ignoring case), and each belongs to one login
token, so an admin's name can't be taken by someone else. (Profiles made
before names had to be unique can share a name; the server warns about them
when it starts.)

The `player` block holds the game rules, which are sent to clients when
they join. The desktop client's `--local` mode reads the same file.

Profiles are saved when players leave, every minute, and when the server
stops (Ctrl+C).

If other devices can't connect, check that your computer's firewall allows
the port.

## Hosting with Docker

To host the game somewhere others can play it, build the Docker image
(`Dockerfile`). It has everything the server needs:
- the server;
- the web client it serves;
- the content;
- a world baked from `world_authoring/`.

Players' profiles go in a volume, so they outlive the container.

Build it from a checkout with its Git LFS files pulled (`git lfs pull`). The
content's textures and models are LFS files; without them, the image has
pointer files instead.

**On the host (an AMD64 Linux machine):**

```sh
git clone <repository> venture && cd venture
git lfs pull
docker compose up -d --build
```

Then open `http://<host>:8080` in a browser.

**On a Mac, for an AMD64 host.** The image builds for any platform from any
platform, at full speed:
- The build stage runs on the building machine's own platform.
- Only the server is compiled for the image's platform: a pure-Rust cross
  compile, with Debian's cross linker.
- The web client (WebAssembly) and the world (data) are the same everywhere.

So build it for `linux/amd64` and send it over:

```sh
docker buildx build --platform linux/amd64 -t venture-server --load .
docker save venture-server | gzip | ssh <host> 'gunzip | docker load'
```

Then, on the host, run it with Docker Compose (as below, without `--build`)
or `docker run`. (Or push it to a registry both can reach.) A plain
`docker build` on a Mac makes an `arm64` image, which is only for trying it
on the Mac.

**Building:**
- The first build compiles the server and the web client and bakes the world.
  That takes a while.
- Cargo's caches are kept between builds, so after a `git pull`, a rebuild
  takes a few minutes.

**Settings and data:**
- **Settings:** the image uses the repository's `server.ron`. To change settings
  (admins especially), copy it to `deploy/server.ron`, edit it, and uncomment
  the line for it in `compose.yaml`.
- **Profiles:** they live in the `venture-data` volume
  (`/srv/venture/server_data`).
- **Stopping:** `docker compose down` stops the server as Ctrl-C would, so it
  saves everyone's profile first.

**Without Compose:**

```sh
docker build -t venture-server .
docker run -d --name venture -p 8080:8080 -v venture-data:/srv/venture/server_data venture-server
docker logs -f venture
```

**Over the internet:** the server speaks plain HTTP and WebSockets. Put it
behind a reverse proxy with TLS (Caddy, nginx) that forwards WebSocket
upgrades; the web client connects back to whichever address served it, so
`https://` becomes `wss://` by itself.

## Web client

Build it once (and again after changing the client), then run the server,
which serves it:

```sh
cd crates/client
trunk build --release --cargo-profile wasm-release   # small, optimised build in dist/
trunk build --watch                                  # while developing: rebuilds on every change
```

The web client connects to the server that served the page. It uses WebGL2,
so it runs in every modern browser. On the web there are no background
threads, so chunk loading is spread across frames.

The server Brotli-compresses the built `.wasm` and `.js` when it starts (it
writes `.br` files beside them in `dist/`; after rebuilding the web client,
restart the server to compress the new build, or it's gzipped as it's sent
instead), and browsers keep everything
that can't change for good, so return visits load from their cache (see
"Caching in the browser" in [client-server.md](client-server.md)).

Development WebAssembly files are very large because they include debug
information. Use the `wasm-release` profile for anything you share.

## Desktop client

```sh
cargo run -p venture_client                                   # a server on this computer
cargo run -p venture_client -- --connect http://192.168.1.20:8080
cargo run -p venture_client -- --local                        # a server inside the client
```

`--local` runs the real server code inside the client, connected through an
in-memory link instead of the network (with `--world <folder>` to choose the
world). Profiles are kept in `local_data/`. Two flags help while working on
the game:

- `--new-character` deletes your local character first (freeing its name),
  so the game asks you to make a new one.
- `--admin` gives every character on the local server admin powers
  (`/setlevel`, `/reset`, `/setclass`, `/setgender`, `/time`, `/timescale`, `/give`, `/setskill`,
  `/tp`, `/setspeed`; `/help` lists them). Only you can join a
  local server, and the real server can't turn this on: its admins are
  only the names in `server.ron`.

```sh
cargo run -p venture_client -- --local --new-character --admin
```

Development builds optimise dependencies (Bevy) and the world generator, but
not the rest of our code, so they compile quickly and still run smoothly.

## Mobile (later)

Bevy supports iOS and Android. When we get there, the client will need a
platform-specific entry point and touch controls. The world crates need no
changes.

## Development tools

### World preview

```sh
cargo run --release -p venture_worldgen --example preview [seed]
```

This writes three images to `preview/`:
- `terrain.png`: the whole world at 2 m per pixel, showing materials, liquids,
  props and settlements (red squares).
- `biomes.png`: the biome regions.
- `map.png`: the coarse map used for rivers, lakes and roads.

Use it to tune `WorldGenConfig` in seconds, without launching the client.

### Automatic screenshots and test clients

The desktop client can run without anyone at the keyboard, which is useful
for checking changes (and for scripts or AI assistants):

```sh
VENTURE_CAMERA="480,2640,0.6,0.3,30" VENTURE_SCREENSHOT=shot.png cargo run -p venture_client -- --local
```

In the game, **F3** shows debug information (position, biome, chunks, frame
rate), and while it's showing **F1** switches to a free camera. Screenshot
runs start with it hidden.

| Variable | Meaning |
| --- | --- |
| `VENTURE_SCREENSHOT=path` | Wait until the world around the player has loaded, save a screenshot there, and exit. Renders offscreen, so it works even when the window isn't visible. |
| `VENTURE_SCREENSHOT_DELAY=seconds` | Wait this much longer after loading before taking the screenshot. |
| `VENTURE_CAMERA=x,y,yaw,pitch,distance` | In `--local` mode, new characters start at `(x, y)` metres. The camera starts at that rotation and tilt (radians) and distance (metres). |
| `VENTURE_MOVE_TO=x,y` | Once loaded, run to `(x, y)` as if it were clicked. |
| `VENTURE_SAY=text` | Say this in chat once in the game (separate several messages with `\|`). Chat commands work too, such as `/talk`. |
| `VENTURE_SAY_DELAY=seconds` | Wait this long once in the game before `VENTURE_SAY` (to answer something another client does first, such as `/accept` a party invitation). |
| `VENTURE_MAP_ZOOM=3.0` | How zoomed in the map opens (with `VENTURE_OPEN=map`), in metres a pixel. |
| `VENTURE_USE=air_blast` | Use these abilities (ids, separated with `\|`) once in the game, on the selected target (after `VENTURE_SELECT_NEAREST` has picked one). Automated runs are silent. |
| `VENTURE_SELECT_NEAREST=1` | Select the character (player or NPC) nearest yours once one is in sight (to check the target frame, or with `VENTURE_SAY=/talk`). |
| `VENTURE_FISH=x,y` | Once loaded, take the first fishing rod in the bag in hand (if none is) and cast at `(x, y)`. Give the gear first: `VENTURE_SAY="/give ledger_rod\|/give wooden_reel\|/give worm 20"` (with `--admin`). |
| `VENTURE_FISH_AUTO=1` | With `VENTURE_FISH`: strike at the bite, and reel while the fish rests, to see a fight. |
| `VENTURE_CHOP=1` | Once loaded, take the first axe in the bag in hand, select the nearest tree the character's Woodworking level can fell, and fell it. |
| `VENTURE_OPEN=stats` | Open the character window on a tab once in the game (`party`, `stats`, `skills`, `abilities`, `inventory`, `gear`, `quests` or `emotes`), or the map (`map`). |
| `VENTURE_NAME=name`, `VENTURE_CLASS=mage`, `VENTURE_BODY=feminine` | The name, class and body type automated runs make new characters with. `VENTURE_NAME=cat` makes an admin (with the default `server.ron`), to try admin commands with `VENTURE_SAY=/setlevel 5`. |
| `VENTURE_LOOK=height=5,build=-3,head=2,hair=long,beard=beard,hair_colour=3,skin=5` | The looks automated runs make new characters with, and the character creation screen starts with (any left out are the usual). |
| `VENTURE_SHOW_CREATION=1` | Show character creation even in an automated run, and take the screenshot there (with `VENTURE_CLASS` and `VENTURE_BODY` choosing what the preview shows). |
| `VENTURE_CHOOSE=1,2` | Pick these dialogue replies (numbered from 1), one per page, as conversations offer them. |
| `VENTURE_PROFILE=name` | Use a separate login token (so a separate character), for running several clients on one computer. Automated `--local` runs normally start with fresh profiles; with a name, the profile is kept between runs, so a check can carry on over several runs. |
| `VENTURE_AUTOMATED=1` | Take the suggested name without asking (implied by `VENTURE_SCREENSHOT`). |

For example, to test multiplayer on one computer, run the server, start one
client with `VENTURE_PROFILE=alice VENTURE_AUTOMATED=1`, and screenshot from
another with `VENTURE_PROFILE=bob`.
