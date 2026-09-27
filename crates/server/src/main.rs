//! The Venture server: hosts the game (and the web client) on the network.
//!
//! ```text
//! cargo run --release -p venture_server
//! ```
//!
//! Then open the printed address in a browser. Settings are read from
//! `server.ron` (or `--config <file>`). See `docs/building-and-running.md`.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use clap::Parser;
use venture_content::ContentPack;
use venture_server::http::{self, HttpConfig};
use venture_server::{ContentSetup, GameServer, Link, ProfileStore, ServerConfig, ServerWorld, SharedClientContent};

/// How often the server's main loop runs. The game itself ticks at a fixed
/// rate within it.
const LOOP_RATE: f64 = 60.0;

/// The Venture game server.
#[derive(Parser)]
#[command(version, about)]
struct Options {
    /// The settings file (see `server.ron`). If it doesn't exist, every setting takes its default.
    #[arg(long, default_value = "server.ron")]
    config: PathBuf,
}

fn main() {
    let options = Options::parse();
    let config = ServerConfig::load(&options.config).unwrap_or_else(|error| fail(&format!("Bad settings: {error}")));
    let paths = &config.paths;

    let started = Instant::now();
    let world =
        ServerWorld::load(&paths.world).unwrap_or_else(|error| fail(&format!("Couldn't load the world: {error}")));
    println!("World loaded in {:.2?}.", started.elapsed());

    let profiles = ProfileStore::new(&paths.profiles)
        .unwrap_or_else(|error| fail(&format!("Can't use the profile folder {}: {error}", paths.profiles.display())));
    if !paths.client.join("index.html").exists() {
        eprintln!(
            "Warning: no web client in {}. Build it with: cd crates/client && trunk build --release --cargo-profile wasm-release",
            paths.client.display()
        );
    }

    let pack = ContentPack::load(&paths.content).unwrap_or_else(|errors| {
        for error in &errors {
            eprintln!("{error}");
        }
        fail(&format!("The content in {} has {} problem(s).", paths.content.display(), errors.len()))
    });
    println!("Content loaded: {} NPCs.", pack.npcs.len());
    let client_content = SharedClientContent::default();

    let (link_sender, link) = Link::new();
    let address = SocketAddr::new(config.network.bind, config.network.port);
    let http_config = HttpConfig {
        address,
        world_folder: paths.world.clone(),
        client_folder: paths.client.clone(),
        content: client_content.clone(),
    };
    http::start(http_config, link_sender);
    print_addresses(config.network.port);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / LOOP_RATE))),
        LogPlugin::default(),
        TerminalCtrlCHandlerPlugin,
    ));
    let content = ContentSetup { folder: paths.content.clone(), pack, client: client_content, watch: true };
    let admins = config.admins.clone();
    let server = GameServer {
        world,
        profiles,
        link,
        rules: config.rules(),
        item_cooldown: config.player.item_cooldown,
        party: config.party.clone(),
        content,
        spawn_override: None,
        admins,
        everyone_is_admin: false,
    };
    server.install(&mut app);
    app.run();
}

/// Prints a message and stops the server.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// Prints the addresses players can open, on this machine and on the local network.
fn print_addresses(port: u16) {
    println!("Venture is running. Open one of these in a browser:");
    println!("  On this computer:    http://localhost:{port}");
    if let Some(address) = local_network_address() {
        println!("  On your network:     http://{address}:{port}");
    }
}

/// This machine's address on the local network. "Connecting" a UDP socket
/// sends nothing; it just makes the system choose the outgoing address.
fn local_network_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip())
}
