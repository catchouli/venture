//! How the client was started: which server to play on.
//!
//! - **Web:** the server that served the page.
//! - **Desktop:** `--connect http://host:port` (default `http://127.0.0.1:8080`),
//!   or `--local` to run a server inside the client for development (see
//!   `crate::network::local_server`), optionally with `--world <folder>`,
//!   `--new-character` (delete your local character first, to make a new
//!   one) and `--admin` (give every character on the local server admin
//!   powers).

use bevy::prelude::*;

/// The server address desktop clients use unless told otherwise.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_SERVER: &str = "http://127.0.0.1:8080";

/// Which server to play on.
#[derive(Resource, Clone, Debug)]
pub enum ServerChoice {
    /// A server over the network, at this base URL (such as `http://192.168.1.20:8080`).
    Remote {
        /// The server's base URL, without a trailing slash.
        base_url: String,
    },
    /// A server running inside this client, using this baked world folder
    /// (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    Local {
        /// The baked world folder.
        world_folder: std::path::PathBuf,
        /// Whether to delete the local character first, to make a new one.
        new_character: bool,
        /// Whether every character on the local server has admin powers.
        admin: bool,
    },
}

impl ServerChoice {
    /// Works out the choice from the page address (web) or the command line (desktop).
    pub fn from_environment() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let origin = web_sys::window().and_then(|window| window.location().origin().ok());
            Self::Remote { base_url: origin.unwrap_or_default() }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let args: Vec<String> = std::env::args().collect();
            let value_after =
                |flag: &str| args.iter().position(|arg| arg == flag).and_then(|i| args.get(i + 1)).cloned();
            if args.iter().any(|arg| arg == "--local") {
                let world_folder = value_after("--world").unwrap_or_else(|| "world".to_string()).into();
                let flag = |name: &str| args.iter().any(|arg| arg == name);
                Self::Local { world_folder, new_character: flag("--new-character"), admin: flag("--admin") }
            } else {
                let base_url = value_after("--connect").unwrap_or_else(|| DEFAULT_SERVER.to_string());
                Self::Remote { base_url: base_url.trim_end_matches('/').to_string() }
            }
        }
    }

    /// Whether this is the in-process development server.
    pub fn is_local(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return matches!(self, Self::Local { .. });
        #[cfg(target_arch = "wasm32")]
        false
    }
}

/// The WebSocket address for a server's base URL (`http` becomes `ws`, `https` becomes `wss`).
pub fn websocket_url(base_url: &str) -> String {
    let address = base_url.strip_prefix("https://").map(|rest| format!("wss://{rest}"));
    let address = address.or_else(|| base_url.strip_prefix("http://").map(|rest| format!("ws://{rest}")));
    format!("{}/ws", address.unwrap_or_else(|| base_url.to_string()))
}
