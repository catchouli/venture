# Venture: notes for working in this repository

- Read `docs/code-style.md` before writing code, and `docs/architecture.md` for where things go.
- `venture_world`, `venture_worldgen` and `venture_navigation` must never depend on Bevy. Keep engine code in `venture_client`.
- Every RPG rule (stats, levels, health, mana, attack rolls, damage, points per level) lives in `venture_rpg`, as small
  tested functions. Don't write RPG maths anywhere else; the server and client both call it.
- The server is authoritative for anything that matters (positions, items, damage, progress). Don't build game
  rules into the client; see `docs/client-server.md`.
- On the server, client messages are only read and validated in `requests.rs` (the gate); players are only
  created/removed in `sessions.rs` and changed in `gameplay.rs`; profiles only touched in `profiles.rs`.
  Inventories only change through `Inventory::apply` (`inventory.rs`), which is checked and all-or-nothing;
  quest logs only through `QuestLog::advance` (`quests.rs`), which only moves forward; characters (class, level,
  stats, points, abilities) only through `Character`'s methods (`character.rs`); equipment only in `equipment.rs`; items are only used (eaten, drunk) in `consumables.rs`. Admin powers live only in `admin.rs`, which
  checks the player is an admin every time. Creatures are only created, moved and removed in `creatures.rs`;
  health only changes through `Health`'s methods, and hits, deaths and experience only happen in `combat.rs`; abilities are only used in `abilities.rs`.
  Keep it that way.
- Content scripts never change the game directly: they queue `ScriptCommand`s (`scripting.rs`), which
  `npcs.rs` validates and applies. New script abilities go through that same path.
- Check content with `cargo run -p venture_content --features scripts --bin check`.
- On the client, only `network/` touches the connection: views read `FromServer`, input writes `ToServer`.
- Client code is grouped by concern (one folder and one plugin per concern); see "Client structure" in
  `docs/architecture.md`. Server settings and game rules go in `server.ron` (`ServerConfig`), not code.
- World generation must stay deterministic and seamless: see "Determinism rules" in
  `docs/world-generation-engine.md` (especially the rule about `lerp`).
- Before finishing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
- To see terrain changes quickly, use `cargo run --release -p venture_worldgen --example preview`.
  To check the 3D look without a person at the keyboard, use `--local` with `VENTURE_SCREENSHOT` and
  `VENTURE_CAMERA` (see `docs/building-and-running.md`). Bake the world first (`--bin bake`).
- Keep the docs in `docs/` up to date when behaviour or design changes.
