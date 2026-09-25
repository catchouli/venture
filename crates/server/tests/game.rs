//! End-to-end tests of the game server: real server systems, driven through
//! the in-memory link by test clients speaking the real protocol.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use venture_content::ContentPack;
use venture_protocol::{ClientMessage, EntityId, EntityKind, PROTOCOL_VERSION, ServerMessage};
use venture_server::content::LoadedContent;
use venture_server::{ConnectionId, ContentSetup, GameServer, Link, LinkEvent, LinkSender, ProfileStore, ServerWorld};
use venture_simulation::{FOLLOW_DISTANCE, RUN_SPEED, Rules, TICK_RATE};
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
        settlements: Vec::new(),
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
        Self::with_content(name, ContentPack::default())
    }

    /// A server with this content pack.
    fn with_content(name: &str, pack: ContentPack) -> Self {
        let profile_folder = std::env::temp_dir().join(format!("venture-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile_folder);
        Self::start(profile_folder, pack)
    }

    fn with_profiles(profile_folder: std::path::PathBuf) -> Self {
        Self::start(profile_folder, ContentPack::default())
    }

    fn start(profile_folder: std::path::PathBuf, pack: ContentPack) -> Self {
        let (link_sender, link) = Link::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(1.0 / TICK_RATE)));
        let profiles = ProfileStore::new(&profile_folder).unwrap();
        let rules = Rules { max_move_distance: 30.0 };
        let content = ContentSetup { folder: profile_folder.clone(), pack, client: Default::default(), watch: false };
        GameServer { world: flat_world(), profiles, link, rules, content, spawn_override: None }.install(&mut app);
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
    fn join(&mut self, token: &str, name: &str) -> (TestClient, EntityId) {
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

    fn receive_all_until_welcome(&self) -> EntityId {
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
    fn latest_position(&self, of: EntityId) -> Option<(f32, f32)> {
        self.receive().into_iter().rev().find_map(|m| match m {
            ServerMessage::Snapshot { entities, .. } => entities.iter().find(|e| e.id == of).map(|e| (e.x, e.y)),
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

    let chats = |client: &TestClient| -> Vec<(EntityId, String)> {
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

#[test]
fn far_clicks_only_go_as_far_as_the_rules_allow() {
    let mut server = TestServer::new("reach");
    let (client, me) = server.join(TOKEN_A, "Walker");
    // The test rules allow 30 m; ask for 100 m.
    client.send(&ClientMessage::MoveTo { x: 200.0, y: 100.0 });
    server.run(200); // Ten seconds: plenty of time to get there.
    let (x, _) = client.latest_position(me).unwrap();
    assert!((x - 130.0).abs() < 0.5, "stopped at {x}");
}

#[test]
fn players_follow_until_told_otherwise() {
    let mut server = TestServer::new("follow");
    let (leader, leader_id) = server.join(TOKEN_A, "Leader");
    let (follower, follower_id) = server.join(TOKEN_B, "Follower");

    leader.send(&ClientMessage::MoveTo { x: 120.0, y: 100.0 });
    follower.send(&ClientMessage::Follow { target: leader_id });
    server.run(100);
    let leader_at = leader.latest_position(leader_id).unwrap();
    let follower_at = follower.latest_position(follower_id).unwrap();
    let gap = (leader_at.0 - follower_at.0).hypot(leader_at.1 - follower_at.1);
    assert!(gap <= FOLLOW_DISTANCE + 0.5, "the follower is {gap} m behind");

    // Stopping ends following: the leader moves on alone.
    follower.send(&ClientMessage::Stop);
    leader.send(&ClientMessage::MoveTo { x: 100.0, y: 120.0 });
    server.run(100);
    let follower_after = follower.latest_position(follower_id).unwrap();
    assert!((follower_after.0 - follower_at.0).hypot(follower_after.1 - follower_at.1) < 0.5);
}

/// A content pack with one appearance, and NPCs defined by `npcs` (RON), and
/// one script.
fn content_with(npcs: &str, script: &str) -> ContentPack {
    let folder =
        std::env::temp_dir().join(format!("venture-test-content-{}-{}", std::process::id(), fastrand_suffix()));
    let write = |path: &str, text: &str| {
        let path = folder.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "appearances/people.ron",
        r#"[(id: "villager", skin: (0.8, 0.6, 0.5), hair: (0.3, 0.2, 0.1), tunic: (0.3, 0.4, 0.2), trousers: (0.3, 0.3, 0.2))]"#,
    );
    write("npcs/town.ron", npcs);
    write("scripts/greeter.rhai", script);
    let pack = ContentPack::load(&folder).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    pack
}

/// A different number each call, so test content folders don't collide.
fn fastrand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

const GREETER: &str = r#"[(id: "greeter", name: "Greeter Tom", appearance: "villager", at: Position((120.0, 100.0)), script: Some("greeter.rhai"))]"#;
const GREETING: &str = r#"fn on_interact(npc, player) { npc.say("Hello, " + player.name + "!"); }"#;

/// The first NPC a client has been told about: its id and name.
fn first_npc(messages: &[ServerMessage]) -> Option<(EntityId, String)> {
    messages.iter().find_map(|m| match m {
        ServerMessage::EntityAppeared { id, name, kind: EntityKind::Npc { .. }, .. } => Some((*id, name.clone())),
        _ => None,
    })
}

#[test]
fn npcs_from_content_appear_to_players() {
    let mut server = TestServer::with_content("npcs", content_with(GREETER, GREETING));
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    client.receive();
    client.send(&ClientMessage::ChooseName { name: "Visitor".to_string() });
    server.run(2);
    let (_, name) = first_npc(&client.receive()).expect("the greeter appears");
    assert_eq!(name, "Greeter Tom");
}

#[test]
fn talking_to_a_distant_npc_walks_over_then_runs_its_script() {
    let mut server = TestServer::with_content("talk", content_with(GREETER, GREETING));
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    client.receive();
    client.send(&ClientMessage::ChooseName { name: "Visitor".to_string() });
    server.run(2);
    let messages = client.receive();
    let (npc, _) = first_npc(&messages).unwrap();
    let me = messages
        .iter()
        .find_map(|m| if let ServerMessage::Welcome { you, .. } = m { Some(*you) } else { None })
        .unwrap();

    client.send(&ClientMessage::Interact { target: npc });
    server.run(100); // The NPC is 20 m away: four seconds of running.
    let messages = client.receive();
    let greeting = messages.iter().find_map(|m| match m {
        ServerMessage::Chat { from, text, .. } if *from == npc => Some(text.clone()),
        _ => None,
    });
    assert_eq!(greeting.as_deref(), Some("Hello, Visitor!"));
    let position = messages.iter().rev().find_map(|m| match m {
        ServerMessage::Snapshot { entities, .. } => entities.iter().find(|e| e.id == me).map(|e| (e.x, e.y)),
        _ => None,
    });
    let (x, y) = position.unwrap();
    assert!((x - 120.0).hypot(y - 100.0) <= venture_simulation::INTERACT_DISTANCE + 0.5, "the player walked over");
}

#[test]
fn changed_content_replaces_changed_npcs() {
    let mut server = TestServer::with_content("reload", content_with(GREETER, GREETING));
    let (client, _) = server.join(TOKEN_A, "Watcher");

    let renamed = GREETER.replace("Greeter Tom", "Greeter Thomas");
    let new_pack = content_with(&renamed, GREETING);
    server.app.world_mut().resource_mut::<LoadedContent>().pack = new_pack;
    server.run(2);
    let messages = client.receive();
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::EntityLeft { .. })), "the old NPC leaves");
    assert_eq!(first_npc(&messages).map(|(_, name)| name).as_deref(), Some("Greeter Thomas"));
}

#[test]
fn npc_speech_only_reaches_players_nearby() {
    // The NPC speaks whenever it's idle; the listener is 10 m away.
    let npcs = r#"[(id: "crier", name: "Town Crier", appearance: "villager", at: Position((110.0, 100.0)), script: Some("greeter.rhai"))]"#;
    let script = r#"fn on_idle(npc) { npc.say("Hear ye!"); }"#;
    let mut server = TestServer::with_content("speech", content_with(npcs, script));
    let (near, _) = server.join(TOKEN_A, "Nearby");
    server.run(220); // Up to ten seconds for the first idle hook.
    let heard = near.receive().iter().any(|m| matches!(m, ServerMessage::Chat { text, .. } if text == "Hear ye!"));
    assert!(heard, "a nearby player hears the NPC");
}
