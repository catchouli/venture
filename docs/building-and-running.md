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
cargo run --release -p venture_worldgen --bin bake [folder] [seed]
```

Bake again whenever world generation changes. The manifest records the
generator version.

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
world). Profiles are kept in `local_data/`.

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

| Variable | Meaning |
| --- | --- |
| `VENTURE_SCREENSHOT=path` | Wait until the world around the player has loaded, save a screenshot there, and exit. Renders offscreen, so it works even when the window isn't visible. |
| `VENTURE_SCREENSHOT_DELAY=seconds` | Wait this much longer after loading before taking the screenshot. |
| `VENTURE_CAMERA=x,y,yaw,pitch,distance` | In `--local` mode, new characters start at `(x, y)` metres. The camera starts at that rotation and tilt (radians) and distance (metres). |
| `VENTURE_MOVE_TO=x,y` | Once loaded, run to `(x, y)` as if it were clicked. |
| `VENTURE_SAY=text` | Say this in chat once in the game (separate several messages with `\|`). Chat commands work too, such as `/talk`. |
| `VENTURE_SELECT_NEAREST=1` | Select the character (player or NPC) nearest yours once one is in sight (to check the target frame, or with `VENTURE_SAY=/talk`). |
| `VENTURE_OPEN=bag,journal` | Open these windows once in the game. |
| `VENTURE_NAME=name`, `VENTURE_CLASS=mage`, `VENTURE_BODY=feminine` | The name, class and body type automated runs make new characters with. `VENTURE_NAME=cat` makes an admin (with the default `server.ron`), to try admin commands with `VENTURE_SAY=/setlevel 5`. |
| `VENTURE_SHOW_CREATION=1` | Show character creation even in an automated run, and take the screenshot there (with `VENTURE_CLASS` and `VENTURE_BODY` choosing what the preview shows). |
| `VENTURE_CHOOSE=1,2` | Pick these dialogue replies (numbered from 1), one per page, as conversations offer them. |
| `VENTURE_PROFILE=name` | Use a separate login token (so a separate character), for running several clients on one computer. Automated `--local` runs normally start with fresh profiles; with a name, the profile is kept between runs, so a check can carry on over several runs. |
| `VENTURE_AUTOMATED=1` | Take the suggested name without asking (implied by `VENTURE_SCREENSHOT`). |

For example, to test multiplayer on one computer, run the server, start one
client with `VENTURE_PROFILE=alice VENTURE_AUTOMATED=1`, and screenshot from
another with `VENTURE_PROFILE=bob`.
