//! Hosting the game over the network, with one port for everything:
//! - `/ws`: game connections, over WebSockets (binary messages),
//! - `/world/...`: the baked world's files (the manifest and chunks),
//! - `/content/...`: the content pack's client files and their manifest
//!   (only presentation data: NPC definitions and scripts are never served),
//! - everything else: the web client's files.
//!
//! Caching: what a browser may keep, it keeps for good, so returning players
//! download only what changed. That's anything whose address changes with
//! its bytes: content files asked for with their hash (`?h=`), world files
//! asked for with the world's fingerprint (`?v=`), and the web client's
//! files, whose names Trunk gives a hash. Everything else (the manifests,
//! `index.html`) is checked with the server every time. The web client's
//! files are also served Brotli-compressed, compressed once when the server
//! starts (see [`precompress_client`]); the rest is gzipped as it's sent.
//!
//! Runs on its own thread with a [tokio] runtime, passing connection events
//! to the game through the [`LinkSender`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use venture_content::CLIENT_MANIFEST_FILE;
use venture_protocol::MAX_CLIENT_MESSAGE_BYTES;

use crate::content::SharedClientContent;
use crate::link::{LinkEvent, LinkSender};

/// The most clients that may be connected at once.
const MAX_CONNECTIONS: usize = 64;

/// Where and what to serve.
pub struct HttpConfig {
    /// The address to listen on.
    pub address: SocketAddr,
    /// The baked world folder.
    pub world_folder: PathBuf,
    /// The built web client (the output of `trunk build`).
    pub client_folder: PathBuf,
    /// The content pack's client files.
    pub content: SharedClientContent,
}

#[derive(Clone)]
struct HostState {
    link: LinkSender,
    connections: Arc<AtomicUsize>,
    content: SharedClientContent,
    /// The fingerprint of the world's manifest, which world addresses carry.
    world_version: Arc<String>,
    /// The web client's folder.
    client_folder: Arc<PathBuf>,
}

/// Keep for good (a year, and don't check).
const CACHE_FOREVER: &str = "public, max-age=31536000, immutable";
/// Check with the server every time (a quick "not modified" if it hasn't).
const CACHE_CHECK: &str = "no-cache";

/// Starts serving on a background thread.
pub fn start(config: HttpConfig, link: LinkSender) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("network".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("the network runtime starts");
            runtime.block_on(serve(config, link));
        })
        .expect("the network thread starts")
}

async fn serve(config: HttpConfig, link: LinkSender) {
    let manifest = std::fs::read(config.world_folder.join(venture_world::MANIFEST_FILE)).unwrap_or_default();
    let state = HostState {
        link,
        connections: Arc::new(AtomicUsize::new(0)),
        content: config.content.clone(),
        world_version: Arc::new(venture_protocol::fingerprint(&manifest)),
        client_folder: Arc::new(config.client_folder.clone()),
    };
    let client_folder = config.client_folder.clone();
    tokio::task::spawn_blocking(move || precompress_client(&client_folder));
    let client = ServeDir::new(&config.client_folder).append_index_html_on_directories(true).precompressed_br();
    let app = Router::new()
        .route("/ws", get(accept_game_connection))
        .route("/content/{*path}", get(serve_content))
        .nest_service("/world", ServeDir::new(&config.world_folder))
        .fallback_service(client)
        .layer(middleware::from_fn_with_state(state.clone(), cache_headers))
        .layer(CompressionLayer::new())
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(config.address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Can't listen on {}: {error}", config.address);
            std::process::exit(1);
        }
    };
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("The network server stopped: {error}");
    }
}

/// Serves the content manifest, or one of the client files it lists (kept
/// for good by the browser when asked for with its current hash, `?h=`).
async fn serve_content(
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<HostState>,
) -> Response {
    let content = state.content.read().expect("the client content lock isn't poisoned");
    if path == CLIENT_MANIFEST_FILE {
        return ([(header::CACHE_CONTROL, CACHE_CHECK)], content.manifest.to_ron()).into_response();
    }
    let Some(bytes) = content.file(&path) else { return StatusCode::NOT_FOUND.into_response() };
    let hashed = query.get("h").is_some_and(|hash| content.has_hash(&path, hash));
    let cache = if hashed { CACHE_FOREVER } else { CACHE_CHECK };
    ([(header::CACHE_CONTROL, cache)], bytes.to_vec()).into_response()
}

/// Says how long browsers may keep what's served (unless the handler
/// already said): see the module's notes.
async fn cache_headers(State(state): State<HostState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or_default().to_string();
    let mut response = next.run(request).await;
    if !response.status().is_success() || response.headers().contains_key(header::CACHE_CONTROL) {
        return response;
    }
    let forever = if let Some(world_path) = path.strip_prefix("/world/") {
        world_path != venture_world::MANIFEST_FILE
            && query.split('&').any(|pair| pair == format!("v={}", state.world_version))
    } else {
        is_hashed_client_file(&path)
    };
    let cache = if forever { CACHE_FOREVER } else { CACHE_CHECK };
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    // The WebAssembly's real size, for the page's loading bar (`content-length`
    // is its compressed size).
    if path.ends_with(".wasm")
        && let Some(name) = path.rsplit('/').next().filter(|name| !name.contains(".."))
        && let Ok(meta) = std::fs::metadata(state.client_folder.join(name))
    {
        response.headers_mut().insert("x-uncompressed-length", HeaderValue::from(meta.len()));
    }
    response
}

/// Whether a web client file's name has Trunk's hash in it
/// (`venture_client-0123456789abcdef_bg.wasm`).
fn is_hashed_client_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or_default();
    let stem = name.split('.').next().unwrap_or_default().trim_end_matches("_bg");
    stem.rsplit_once('-').is_some_and(|(_, hash)| hash.len() >= 16 && hash.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Brotli-compresses the web client's big files (`.wasm` and `.js`) beside
/// them (`.br`), once, so they're served compressed without compressing on
/// every request. (Until it's done, they're gzipped as they're sent.)
pub fn precompress_client(folder: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
        let wanted = path.extension().is_some_and(|extension| extension == "wasm" || extension == "js");
        let compressed = path.with_extension(format!("{}.br", path.extension().unwrap_or_default().to_string_lossy()));
        let modified = |path: &std::path::Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
        if !wanted || modified(&compressed).is_some_and(|done| modified(&path).is_some_and(|source| done >= source)) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let started = std::time::Instant::now();
        let mut out = Vec::new();
        let params = brotli::enc::BrotliEncoderParams { quality: 9, lgwin: 22, ..Default::default() };
        if brotli::BrotliCompress(&mut bytes.as_slice(), &mut out, &params).is_err() {
            continue;
        }
        // (Written beside, then moved into place, so it's never served half-written.)
        let partial = compressed.with_extension("br.partial");
        if std::fs::write(&partial, &out).is_ok() && std::fs::rename(&partial, &compressed).is_ok() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let (before, after) = (bytes.len() / 1024, out.len() / 1024);
            println!("Compressed {name}: {before} KB to {after} KB in {:.1?}.", started.elapsed());
        }
    }
}

/// Upgrades a request to a WebSocket game connection, if there's room.
async fn accept_game_connection(upgrade: WebSocketUpgrade, State(state): State<HostState>) -> Response {
    if state.connections.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
        return (StatusCode::SERVICE_UNAVAILABLE, "The server is full.").into_response();
    }
    upgrade.max_message_size(MAX_CLIENT_MESSAGE_BYTES).on_upgrade(move |socket| run_connection(socket, state))
}

/// Carries one connection's messages between its WebSocket and the game until either side closes it.
async fn run_connection(mut socket: WebSocket, state: HostState) {
    state.connections.fetch_add(1, Ordering::Relaxed);
    let (id, outgoing) = state.link.connect();
    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Binary(bytes))) => {
                    if !state.link.report(LinkEvent::Received { id, bytes: bytes.to_vec() }) {
                        break;
                    }
                }
                // Pings are answered automatically; text messages aren't part of the protocol.
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_))) => {}
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            },
            outgoing = outgoing.recv() => match outgoing {
                Ok(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                // The game dropped the outbox: it has closed this connection.
                Err(_) => break,
            },
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    state.link.report(LinkEvent::Disconnected { id });
    state.connections.fetch_sub(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_trunks_hashed_files_are_kept_for_good() {
        assert!(is_hashed_client_file("/venture_client-51708c4ba02fd85b_bg.wasm"));
        assert!(is_hashed_client_file("/venture_client-51708c4ba02fd85b.js"));
        assert!(!is_hashed_client_file("/index.html"));
        assert!(!is_hashed_client_file("/"));
        assert!(!is_hashed_client_file("/venture-client.js"));
    }
}
