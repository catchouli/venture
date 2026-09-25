# Venture: notes for working in this repository

- Read `docs/code-style.md` before writing code, and `docs/architecture.md` for where things go.
- `venture_world`, `venture_worldgen` and `venture_navigation` must never depend on Bevy. Keep engine code in `venture_client`.
- The server is authoritative for anything that matters (positions, items, damage, progress). Don't build game
  rules into the client; see `docs/client-server.md`.
- World generation must stay deterministic and seamless: see "Determinism rules" in
  `docs/world-generation-engine.md` (especially the rule about `lerp`).
- Before finishing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
- To see terrain changes quickly, use `cargo run --release -p venture_worldgen --example preview`.
  To check the 3D look without a person at the keyboard, use `VENTURE_SCREENSHOT` and `VENTURE_CAMERA`
  (see `docs/building-and-running.md`).
- Keep the docs in `docs/` up to date when behaviour or design changes.
