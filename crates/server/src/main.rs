//! The Venture server: hosts the game (and the web client) on the network.
//!
//! ```text
//! cargo run --release -p venture_server
//! ```
//!
//! Then open the printed address in a browser. See `docs/building-and-running.md`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use clap::Parser;
use venture_server::http::{self, HttpConfig};
use venture_server::{GameServer, Link, ProfileStore, ServerWorld};

/// How often the server's main loop runs. The game itself ticks at a fixed
/// rate within it.
const LOOP_RATE: f64 = 60.0;

/// The Venture game server.
#[derive(Parser)]
#[command(version, about)]
struct Options {
    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Address to listen on. The default accepts connections from the local network.
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::UNSPECIFIED))]
    bind: IpAddr,
    /// The baked world folder.
    #[arg(long, default_value = "world")]
    world: PathBuf,
    /// Where player profiles are saved.
    #[arg(long, default_value = "server_data/profiles")]
    profiles: PathBuf,
    /// The built web client.
    #[arg(long, default_value = "crates/client/dist")]
    client: PathBuf,
}

fn main() {
    let options = Options::parse();

    let started = Instant::now();
    let world = match ServerWorld::load(&options.world) {
        Ok(world) => world,
        Err(error) => {
            eprintln!("Couldn't load the world: {error}");
            std::process::exit(1);
        }
    };
    println!("World loaded in {:.2?}.", started.elapsed());

    let profiles = ProfileStore::new(&options.profiles).unwrap_or_else(|error| {
        eprintln!("Can't use the profile folder {}: {error}", options.profiles.display());
        std::process::exit(1);
    });
    if !options.client.join("index.html").exists() {
        eprintln!(
            "Warning: no web client in {}. Build it with: cd crates/client && trunk build --release --cargo-profile wasm-release",
            options.client.display()
        );
    }

    let (link_sender, link) = Link::new();
    let address = SocketAddr::new(options.bind, options.port);
    http::start(HttpConfig { address, world_folder: options.world, client_folder: options.client }, link_sender);
    print_addresses(options.port);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / LOOP_RATE))),
        LogPlugin::default(),
        TerminalCtrlCHandlerPlugin,
    ));
    GameServer { world, profiles, link, spawn_override: None }.install(&mut app);
    app.run();
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
