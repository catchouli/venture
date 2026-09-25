//! End-to-end tests of the game server: real server systems, driven through
//! the in-memory link by test clients speaking the real protocol.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use venture_protocol::{ClientMessage, PROTOCOL_VERSION, PlayerId, ServerMessage};
use venture_server::{ConnectionId, GameServer, Link, LinkEvent, LinkSender, ProfileStore, ServerWorld};
use venture_simulation::{RUN_SPEED, TICK_RATE};
use venture_world::{CHUNK_FORMAT_VERSION, CHUNK_SIZE, Chunk, ChunkPos, WorldManifest};

/// A small flat world, 4 × 4 chunks.
fn flat_world() -> ServerWorld {
    let manifest = WorldManifest {
        world_size: 4 * CHUNK_SIZE,
        chunk_size: CHUNK_SIZE,
        chunk_format: CHUNK_FORMAT_VERSION,
        generator_version: 0,
        seed: 0,
        spawn_point: (100.0, 100.0),
    };
    let chunks: Vec<Chunk> = (0..16).map(|i| Chunk::new(ChunkPos::new(i % 4, i / 4))).collect();
    ServerWorld::from_chunks(manifest, &chunks)
}

/// A server running in a test, with its own profile folder.
struct TestServer {
    app: App,
    link: LinkSender,
    profile_folder: std::path::PathBuf,
    /// Whether to delete the profile folder when the server is dropped.
    delete_profiles: bool,
}

impl TestServer {
    fn new(name: &str) -> Self {
        let profile_folder = std::env::temp_dir().join(format!("venture-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile_folder);
        Self::with_profiles(profile_folder)
    }

    fn with_profiles(profile_folder: std::path::PathBuf) -> Self {
        let (link_sender, link) = Link::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(1.0 / TICK_RATE)));
        let profiles = ProfileStore::new(&profile_folder).unwrap();
        GameServer { world: flat_world(), profiles, link, spawn_override: None }.install(&mut app);
        Self { app, link: link_sender, profile_folder, delete_profiles: true }
    }

    /// Runs the server for a number of ticks.
    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.app.update();
        }
    }

    fn connect(&mut self) -> TestClient {
        let (id, outgoing) = self.link.connect();
        self.run(1);
        TestClient { id, outgoing, link: self.link.clone() }
    }

    /// Connects and logs in as a new character called `name`.
    fn join(&mut self, token: &str, name: &str) -> (TestClient, PlayerId) {
        let client = self.connect();
        client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: token.to_string() });
        self.run(2);
        if client.receive().iter().any(|m| matches!(m, ServerMessage::NameNeeded { .. })) {
            client.send(&ClientMessage::ChooseName { name: name.to_string() });
            self.run(2);
        }
        let id = client.receive_all_until_welcome();
        (client, id)
    }
}

impl TestServer {
    /// Stops the server, keeping its profiles for another to use.
    fn shut_down(mut self) -> std::path::PathBuf {
        self.delete_profiles = false;
        self.profile_folder.clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if self.delete_profiles {
            let _ = std::fs::remove_dir_all(&self.profile_folder);
        }
    }
}

struct TestClient {
    id: ConnectionId,
    outgoing: async_channel::Receiver<Vec<u8>>,
    link: LinkSender,
}

impl TestClient {
    fn send(&self, message: &ClientMessage) {
        self.send_bytes(message.encode());
    }

    fn send_bytes(&self, bytes: Vec<u8>) {
        self.link.report(LinkEvent::Received { id: self.id, bytes });
    }

    fn disconnect(&self) {
        self.link.report(LinkEvent::Disconnected { id: self.id });
    }

    /// Every message received since last time.
    fn receive(&self) -> Vec<ServerMessage> {
        std::iter::from_fn(|| self.outgoing.try_recv().ok())
            .map(|bytes| ServerMessage::decode(&bytes).unwrap())
            .collect()
    }

    fn receive_all_until_welcome(&self) -> PlayerId {
        self.receive()
            .into_iter()
            .find_map(|m| match m {
                ServerMessage::Welcome { you, .. } => Some(you),
                _ => None,
            })
            .expect("a welcome")
    }

    /// Whether the server has closed this connection.
    fn is_closed(&self) -> bool {
        self.outgoing.is_closed()
    }

    /// This player's position in the latest snapshot received.
    fn latest_position(&self, of: PlayerId) -> Option<(f32, f32)> {
        self.receive().into_iter().rev().find_map(|m| match m {
            ServerMessage::Snapshot { players, .. } => players.iter().find(|p| p.id == of).map(|p| (p.x, p.y)),
            _ => None,
        })
    }
}

const TOKEN_A: &str = "test-token-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TOKEN_B: &str = "test-token-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn new_players_choose_a_valid_name_and_enter_the_game() {
    let mut server = TestServer::new("names");
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    assert!(matches!(client.receive()[..], [ServerMessage::NameNeeded { .. }]));

    client.send(&ClientMessage::ChooseName { name: "!!".to_string() });
    server.run(2);
    assert!(matches!(client.receive()[..], [ServerMessage::NameRejected { .. }]));

    client.send(&ClientMessage::ChooseName { name: "  Aldric  ".to_string() });
    server.run(2);
    let welcome = client.receive().into_iter().find(|m| matches!(m, ServerMessage::Welcome { .. }));
    assert!(
        matches!(welcome, Some(ServerMessage::Welcome { ref name, position: (100.0, 100.0), .. }) if name == "Aldric")
    );
}

#[test]
fn players_move_no_faster_than_running_speed() {
    let mut server = TestServer::new("movement");
    let (client, me) = server.join(TOKEN_A, "Runner");

    client.send(&ClientMessage::MoveTo { x: 150.0, y: 100.0 });
    server.run(20); // One second.
    let (x, _) = client.latest_position(me).unwrap();
    assert!(x > 100.0 && x <= 100.0 + RUN_SPEED * 1.01, "moved to {x}");

    // A forged, huge steering vector is no faster.
    client.send(&ClientMessage::Steer { x: 1.0e9, y: 0.0 });
    server.run(20);
    let (x2, _) = client.latest_position(me).unwrap();
    assert!(x2 - x <= RUN_SPEED * 1.01, "moved {} in a second", x2 - x);
}

#[test]
fn chat_reaches_everyone_and_empty_messages_are_ignored() {
    let mut server = TestServer::new("chat");
    let (alice, alice_id) = server.join(TOKEN_A, "Alice");
    let (bob, _) = server.join(TOKEN_B, "Bob");
    alice.receive();

    alice.send(&ClientMessage::Chat { text: "   ".to_string() });
    alice.send(&ClientMessage::Chat { text: " Hello\u{0} there ".to_string() });
    server.run(1);

    let chats = |client: &TestClient| -> Vec<(PlayerId, String)> {
        client
            .receive()
            .into_iter()
            .filter_map(|m| match m {
                ServerMessage::Chat { from, text, .. } => Some((from, text)),
                _ => None,
            })
            .collect()
    };
    let expected = vec![(alice_id, "Hello there".to_string())];
    assert_eq!(chats(&alice), expected);
    assert_eq!(chats(&bob), expected);
}

#[test]
fn commands_before_logging_in_are_refused() {
    let mut server = TestServer::new("order");
    let client = server.connect();
    for _ in 0..10 {
        client.send(&ClientMessage::MoveTo { x: 10.0, y: 10.0 });
    }
    server.run(1);
    assert!(client.is_closed(), "too many out-of-order messages close the connection");
}

#[test]
fn garbage_and_wrong_versions_are_refused() {
    let mut server = TestServer::new("garbage");
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION + 1, token: TOKEN_A.to_string() });
    server.run(1);
    assert!(matches!(client.receive()[..], [ServerMessage::Rejected { .. }]));
    assert!(client.is_closed());

    let client = server.connect();
    for _ in 0..10 {
        client.send_bytes(vec![255, 255, 255, 255]);
    }
    server.run(1);
    assert!(client.is_closed());
}

#[test]
fn a_character_cant_be_logged_in_twice() {
    let mut server = TestServer::new("twice");
    let (_first, _) = server.join(TOKEN_A, "Twin");
    let second = server.connect();
    second.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    assert!(matches!(second.receive()[..], [ServerMessage::Rejected { .. }]));
}

#[test]
fn characters_come_back_where_they_left_off() {
    let mut server = TestServer::new("persistence");
    let (client, me) = server.join(TOKEN_A, "Wanderer");
    client.send(&ClientMessage::MoveTo { x: 110.0, y: 100.0 });
    server.run(60);
    let left_at = client.latest_position(me).unwrap();
    client.disconnect();
    server.run(2);

    // A new server with the same profile folder.
    let mut server = TestServer::with_profiles(server.shut_down());
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    let welcome = client.receive().into_iter().find_map(|m| match m {
        ServerMessage::Welcome { name, position, .. } => Some((name, position)),
        _ => None,
    });
    assert_eq!(welcome, Some(("Wanderer".to_string(), left_at)));
}
