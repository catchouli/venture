# Building and running

## Requirements

- Rust (stable, 1.95 or newer), via [rustup](https://rustup.rs).
- For the web build: the WebAssembly target and [Trunk](https://trunkrs.dev):
  ```sh
  rustup target add wasm32-unknown-unknown
  cargo install --locked trunk
  ```

## Desktop

```sh
cargo run -p venture_client            # development build
cargo run -p venture_client --release  # optimised build
```

Development builds optimise dependencies (Bevy) and the world generator, but
not the rest of our code, so they compile quickly and still run smoothly. This
is set up in the workspace `Cargo.toml`.

## Web

```sh
cd crates/client
trunk serve                                            # development, with live reload
trunk build --release --cargo-profile wasm-release     # small, optimised build in dist/
```

The web build uses WebGL2, so it runs in every modern browser. The client fills
the page (`crates/client/index.html`). On the web there are no background
threads, so chunk generation runs on the main thread between frames. The
streaming code starts only one chunk per frame there, to keep things smooth.

Development WebAssembly files are very large because they include debug
information. Use the `wasm-release` profile for anything you deploy.

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

### Automatic screenshots

The client can take a screenshot by itself, which is useful for checking how
changes look (and for scripts or AI assistants):

```sh
VENTURE_CAMERA="480,2640,0.6,0.3,30" VENTURE_SCREENSHOT=shot.png cargo run -p venture_client
```

- `VENTURE_CAMERA` is `x,y,yaw,pitch,distance`: where the player starts (in
  metres), and the camera's rotation and tilt (in radians) and distance (in metres).
- `VENTURE_MOVE_TO` is `x,y`: once loaded, the player runs there as if that
  point were clicked (useful for seeing the run animation and the move marker).
- `VENTURE_SCREENSHOT` is where to save the image. The client waits until every
  visible chunk has loaded, saves the screenshot and exits. It renders
  offscreen, so it works even when the window isn't visible.
