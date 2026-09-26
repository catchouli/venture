//! The Venture game client.
//!
//! The client is a view of the game on the server: it connects, streams the
//! baked world's chunks around the camera, shows everyone in the game, and
//! sends the player's commands. The game is in the library (see
//! `crates/client/src/lib.rs`), shared with the editor.
//!
//! Desktop options: `--connect http://host:port` to play on a server (the
//! default is `http://127.0.0.1:8080`), or `--local` to run a server inside
//! the client for development.

fn main() {
    venture_client::run();
}
