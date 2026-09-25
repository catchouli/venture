//! Hosting the game over the network, with one port for everything:
//! - `/ws`: game connections, over WebSockets (binary messages),
//! - `/world/...`: the baked world's files (the manifest and chunks),
//! - `/content/...`: the content pack's client files and their manifest
//!   (only presentation data: NPC definitions and scripts are never served),
//! - everything else: the web client's files.
//!
//! Runs on its own thread with a [tokio] runtime, passing connection events
//! to the game through the [`LinkSender`].

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
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
}

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
    let state = HostState { link, connections: Arc::new(AtomicUsize::new(0)), content: config.content.clone() };
    let client = ServeDir::new(&config.client_folder).append_index_html_on_directories(true);
    let app = Router::new()
        .route("/ws", get(accept_game_connection))
        .route("/content/{*path}", get(serve_content))
        .nest_service("/world", ServeDir::new(&config.world_folder))
        .fallback_service(client)
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

/// Serves the content manifest, or one of the client files it lists.
async fn serve_content(Path(path): Path<String>, State(state): State<HostState>) -> Response {
    let content = state.content.read().expect("the client content lock isn't poisoned");
    if path == CLIENT_MANIFEST_FILE {
        return ([(header::CACHE_CONTROL, "no-cache")], content.manifest.to_ron()).into_response();
    }
    match content.file(&path) {
        Some(bytes) => bytes.to_vec().into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
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
