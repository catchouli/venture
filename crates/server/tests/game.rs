//! End-to-end tests of the game server: real server systems, driven through
//! the in-memory link by test clients speaking the real protocol.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use venture_content::ContentPack;
use venture_protocol::{
    AdminCommand, BodyType, ClientMessage, EntityId, EntityKind, EquipSlot, Features, FishingCommand, FishingState,
    HotbarAction, HotbarEntry, PROTOCOL_VERSION, ServerMessage,
};
use venture_server::content::LoadedContent;
use venture_server::entities::NetworkId;
use venture_server::npcs::Npc;
use venture_server::{ConnectionId, ContentSetup, GameServer, Link, LinkEvent, LinkSender, ProfileStore, ServerWorld};
use venture_simulation::{FOLLOW_DISTANCE, RUN_SPEED, Rules, TICK_RATE};
use venture_world::{CHUNK_FORMAT_VERSION, CHUNK_SIZE, Chunk, ChunkPos, Fluid, FluidBody, FluidKind, WorldManifest};

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
        map: None,
    };
    let chunks: Vec<Chunk> = (0..16).map(|i| Chunk::new(ChunkPos::new(i % 4, i / 4))).collect();
    ServerWorld::from_chunks(manifest, &chunks)
}

/// The flat world with a river across it: cells 104 to 113 metres east,
/// all the way north to south, a metre deep.
fn river_world() -> ServerWorld {
    let manifest = WorldManifest {
        world_size: 4 * CHUNK_SIZE,
        chunk_size: CHUNK_SIZE,
        chunk_format: CHUNK_FORMAT_VERSION,
        generator_version: 0,
        seed: 0,
        spawn_point: (100.0, 100.0),
        settlements: Vec::new(),
        map: None,
    };
    let chunks: Vec<Chunk> = (0..16)
        .map(|i| {
            let mut chunk = Chunk::new(ChunkPos::new(i % 4, i / 4));
            if i % 4 == 1 {
                for y in 0..CHUNK_SIZE as usize {
                    for x in 40..50 {
                        chunk.cell_mut(x, y).fluid =
                            Some(Fluid { kind: FluidKind::Water, body: FluidBody::River, level: 1.0 });
                    }
                }
            }
            chunk
        })
        .collect();
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

    /// A server with the game's own content, in the river world.
    fn fishing(name: &str) -> Self {
        let profile_folder = std::env::temp_dir().join(format!("venture-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile_folder);
        let pack = ContentPack::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")).unwrap();
        let mut server = Self::start(profile_folder, pack);
        server.app.insert_resource(river_world());
        server
    }

    fn start(profile_folder: std::path::PathBuf, pack: ContentPack) -> Self {
        let (link_sender, link) = Link::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(1.0 / TICK_RATE)));
        let profiles = ProfileStore::new(&profile_folder).unwrap();
        let rules = Rules { max_move_distance: 30.0, ..Rules::default() };
        let content = ContentSetup { folder: profile_folder.clone(), pack, client: Default::default(), watch: false };
        let admins = vec!["Boss".to_string()];
        GameServer {
            world: flat_world(),
            profiles,
            link,
            rules,
            item_cooldown: 1.0,
            party: Default::default(),
            fishing: Default::default(),
            content,
            spawn_override: None,
            admins,
            everyone_is_admin: false,
        }
        .install(&mut app);
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
        if client.receive().iter().any(|m| matches!(m, ServerMessage::CharacterNeeded { .. })) {
            client.send(&ClientMessage::CreateCharacter {
                name: name.to_string(),
                class: "fighter".to_string(),
                body: BodyType::Androgynous,
                features: Features::default(),
            });
            self.run(2);
        }
        let id = client.receive_all_until_welcome();
        (client, id)
    }

    /// The id of the (first) NPC in the game.
    fn npc(&mut self) -> EntityId {
        let world = self.app.world_mut();
        let mut npcs = world.query_filtered::<&NetworkId, With<Npc>>();
        npcs.iter(world).next().expect("an NPC").0
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
    assert!(matches!(client.receive()[..], [ServerMessage::CharacterNeeded { .. }]));

    client.send(&ClientMessage::CreateCharacter {
        name: "!!".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
    server.run(2);
    assert!(matches!(client.receive()[..], [ServerMessage::CharacterRejected { .. }]));

    client.send(&ClientMessage::CreateCharacter {
        name: "  Aldric  ".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
    server.run(2);
    let welcome = client.receive().into_iter().find(|m| matches!(m, ServerMessage::Welcome { .. }));
    assert!(
        matches!(welcome, Some(ServerMessage::Welcome { ref name, position: (100.0, 100.0), .. }) if name == "Aldric")
    );
}

#[test]
fn chosen_features_are_kept_within_what_the_content_offers() {
    let mut server = TestServer::new("features");
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    let features = Features {
        height: 100,
        build: -4,
        hair: Some("no such hair".to_string()),
        hair_colour: 200,
        ..Features::default()
    };
    client.send(&ClientMessage::CreateCharacter {
        name: "Tallis".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features,
    });
    server.run(2);
    let look = client.receive().into_iter().find_map(|m| match m {
        ServerMessage::Welcome { look, .. } => Some(look),
        _ => None,
    });
    let features = look.expect("a welcome").features;
    assert_eq!((features.height, features.build), (10, -4));
    assert_eq!(features.hair, None, "the test content has no hairstyles");
    assert_eq!(features.hair_colour, 0);
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
    content_with_dialogue(npcs, script, None)
}

/// Like [`content_with`], plus one dialogue file. Every test pack also has
/// bread, a bakery selling bread for 5 Miasma, a two-stage quest ("errand"),
/// and new characters start with 25 Miasma and a loaf. There are two
/// classes, Fighter and Mage.
fn content_with_dialogue(npcs: &str, script: &str, dialogue: Option<&str>) -> ContentPack {
    content_with_files(npcs, script, dialogue, &[])
}

/// Like [`content_with_dialogue`], plus more files: (path, contents).
fn content_with_files(npcs: &str, script: &str, dialogue: Option<&str>, extra: &[(&str, &str)]) -> ContentPack {
    let folder =
        std::env::temp_dir().join(format!("venture-test-content-{}-{}", std::process::id(), fastrand_suffix()));
    let write = |path: &str, contents: &[u8]| {
        let path = folder.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    };
    write(
        "appearances/people.ron",
        br#"[(id: "villager", skin: (0.8, 0.6, 0.5), hair: (0.3, 0.2, 0.1), tunic: (0.3, 0.4, 0.2), trousers: (0.3, 0.3, 0.2))]"#,
    );
    write("npcs/town.ron", npcs.as_bytes());
    write("scripts/greeter.rhai", script.as_bytes());
    write("items/things.ron", ITEMS.as_bytes());
    write("classes/classes.ron", CLASSES.as_bytes());
    write("abilities/abilities.ron", ABILITIES.as_bytes());
    write(
        "skills/skills.ron",
        br#"[(id: "fishing", name: "Fishing", description: "Fish.", icon: "thing.png"),
            (id: "mining", name: "Mining", description: "Rocks.", icon: "thing.png")]"#,
    );
    write("shops/bakery.ron", br#"[(id: "bakery", name: "The Bakery", stock: [(item: "bread", price: 5)])]"#);
    write(
        "quests/errand.ron",
        br#"[(id: "errand", name: "An Errand", stages: [(id: "asked", journal: "Tom wants a loaf."), (id: "done", journal: "I gave Tom a loaf.")])]"#,
    );
    write("new_characters.ron", br#"(miasma: 25, items: [(item: "bread", count: 1)], abilities: ["rest"])"#);
    // Only the start of a PNG, but that's all the content check looks at.
    write("icons/thing.png", b"\x89PNG\r\n\x1a\n");
    if let Some(dialogue) = dialogue {
        write("dialogue/town.yarn", dialogue.as_bytes());
    }
    for (path, contents) in extra {
        write(path, contents.as_bytes());
    }
    let pack = ContentPack::load(&folder).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    pack
}

// (The sword uses `Weapon`, the dominant hand's old name, which still works.)
const ITEMS: &str = r#"[
    (id: "bread", name: "Bread", description: "A loaf.", icon: "thing.png", value: 4, on_use: Heal(Flat(5))),
    (id: "sword", name: "Sword", description: "Sharp.", icon: "thing.png", value: 10,
     equip: Some((slots: [Weapon], weapon: Some((damage: "1d6", speed: 1.0, stat: Strength)),
                  look: (style: Sword, main: (0.7, 0.7, 0.7), trim: (0.3, 0.2, 0.1))))),
    (id: "chestguard", name: "Chestguard", description: "Wooden.", icon: "thing.png", value: 6,
     equip: Some((slots: [Body], armour: 2, look: (style: Plates, main: (0.6, 0.4, 0.3), trim: (0.3, 0.2, 0.1))))),
    (id: "robe", name: "Robe", description: "Long.", icon: "thing.png", value: 6,
     equip: Some((slots: [Body, Legs], look: (style: Robe, main: (0.3, 0.3, 0.5), trim: (0.8, 0.7, 0.4))))),
]"#;

const CLASSES: &str = r#"[
    (id: "fighter", name: "Fighter", description: "Hits things.",
     rules: (hit_die: 10, mana_per_level: 3, casting_stat: Wisdom),
     stats: (strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10),
     starting_equipment: ["sword", "chestguard"], starting_abilities: ["whack"],
     abilities: [(ability: "bash", level: 2, cost: 1)]),
    (id: "mage", name: "Mage", description: "Casts things.",
     rules: (hit_die: 6, mana_per_level: 8, casting_stat: Intelligence),
     stats: (strength: 8, dexterity: 12, constitution: 12, intelligence: 15, wisdom: 13),
     starting_equipment: ["robe"], starting_abilities: ["blast"]),
]"#;

const ABILITIES: &str = r#"[
    (id: "rest", name: "Rest", description: "Sit down.", icon: "thing.png", effect: Rest),
    (id: "whack", name: "Whack", description: "A heavy blow.", icon: "thing.png", mana: 2, cooldown: 6.0, effect: Strike(damage: "1d8")),
    (id: "bash", name: "Bash", description: "Another blow.", icon: "thing.png", effect: Strike(damage: "1d6")),
    (id: "blast", name: "Blast", description: "Whoosh.", icon: "thing.png", mana: 3, range: 15.0, effect: Bolt(damage: "1d8", speed: 10.0)),
]"#;

/// A different number each call, so test content folders don't collide.
fn fastrand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

const GREETER: &str = r#"[(id: "greeter", name: "Greeter Tom", appearance: "villager", at: (120.0, 100.0), script: Some("greeter.rhai"))]"#;
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
    client.send(&ClientMessage::CreateCharacter {
        name: "Visitor".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
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
    client.send(&ClientMessage::CreateCharacter {
        name: "Visitor".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
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
    let npcs = r#"[(id: "crier", name: "Town Crier", appearance: "villager", at: (110.0, 100.0), script: Some("greeter.rhai"))]"#;
    let script = r#"fn on_idle(npc) { npc.say("Hear ye!"); }"#;
    let mut server = TestServer::with_content("speech", content_with(npcs, script));
    let (near, _) = server.join(TOKEN_A, "Nearby");
    server.run(220); // Up to ten seconds for the first idle hook.
    let heard = near.receive().iter().any(|m| matches!(m, ServerMessage::Chat { text, .. } if text == "Hear ye!"));
    assert!(heard, "a nearby player hears the NPC");
}

#[test]
fn returning_to_town_sends_players_home() {
    let mut server = TestServer::new("town");
    let (walker, me) = server.join(TOKEN_A, "Wanderer");
    let (listener, _) = server.join(TOKEN_B, "Listener");
    walker.send(&ClientMessage::MoveTo { x: 125.0, y: 100.0 });
    server.run(120);
    assert!(walker.latest_position(me).unwrap().0 > 120.0);
    listener.receive();

    walker.send(&ClientMessage::ReturnToTown);
    server.run(2);
    let messages = walker.receive();
    let home = messages.iter().rev().find_map(|m| match m {
        ServerMessage::Snapshot { entities, .. } => entities.iter().find(|e| e.id == me).map(|e| (e.x, e.y)),
        _ => None,
    });
    assert_eq!(home, Some((100.0, 100.0)), "back at the spawn point");
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::Notice { text } if text == "You return to town.")));
    assert!(!listener.receive().iter().any(|m| matches!(m, ServerMessage::Notice { .. })), "only they are told");
}

/// The notices in a batch of messages.
fn notices(messages: &[ServerMessage]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|m| if let ServerMessage::Notice { text } = m { Some(text.clone()) } else { None })
        .collect()
}

#[test]
fn players_are_told_why_they_cant_follow_npcs_or_talk_to_players() {
    let mut server = TestServer::with_content("refusals", content_with(GREETER, GREETING));
    let (client, me) = server.join(TOKEN_A, "Visitor");
    let (_, other) = server.join(TOKEN_B, "Other");
    let npc = server.npc();
    client.receive();

    client.send(&ClientMessage::Follow { target: npc });
    client.send(&ClientMessage::Interact { target: other });
    client.send(&ClientMessage::Follow { target: me });
    server.run(2);
    assert_eq!(
        notices(&client.receive()),
        [
            "You can't follow Greeter Tom. Only other players can be followed.",
            "Other is another player. To talk to them, use the chat box.",
            "You can't follow yourself.",
        ]
    );
    server.run(40);
    assert_eq!(client.latest_position(me), Some((100.0, 100.0)), "refused orders don't move anyone");
}

const TALKER: &str =
    r#"[(id: "talker", name: "Tom", appearance: "villager", at: (102.0, 100.0), dialogue: Some("Tom"))]"#;
const TALK: &str = "title: Tom\n---\nTom: Hello, {player_name()}.\n-> Who are you?\n    Tom: I'm Tom.\n-> Bye.\n===\n";

/// The conversation pages in a batch of messages: (lines, options).
fn pages(messages: &[ServerMessage]) -> Vec<(Vec<String>, Vec<String>)> {
    messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Dialogue { lines, options, .. } => {
                Some((lines.iter().map(|line| line.text.clone()).collect(), options.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn conversations_are_private_and_branch_on_the_players_choices() {
    let mut server = TestServer::with_content("dialogue", content_with_dialogue(TALKER, "", Some(TALK)));
    let (talker, _) = server.join(TOKEN_A, "Visitor");
    let (bystander, _) = server.join(TOKEN_B, "Bystander");
    let npc = server.npc();

    talker.send(&ClientMessage::Interact { target: npc });
    server.run(2);
    let expected_options = vec!["Who are you?".to_string(), "Bye.".to_string()];
    assert_eq!(pages(&talker.receive()), [(vec!["Hello, Visitor.".to_string()], expected_options)]);
    assert!(pages(&bystander.receive()).is_empty(), "nobody else sees the conversation");

    // A choice that isn't on offer is ignored; a real one moves the conversation on.
    talker.send(&ClientMessage::ChooseDialogueOption { index: 7 });
    talker.send(&ClientMessage::ChooseDialogueOption { index: 0 });
    server.run(2);
    assert_eq!(pages(&talker.receive()), [(vec!["I'm Tom.".to_string()], vec![])], "and then it's over");

    // Walking away ends a conversation.
    talker.send(&ClientMessage::Interact { target: npc });
    server.run(2);
    talker.receive();
    talker.send(&ClientMessage::MoveTo { x: 90.0, y: 100.0 });
    server.run(10);
    assert!(!talker.receive().contains(&ServerMessage::DialogueEnded), "not while still close by");
    server.run(30);
    assert!(talker.receive().contains(&ServerMessage::DialogueEnded));
}

/// The latest inventory in a batch of messages, as (item, count) pairs.
fn latest_inventory(messages: &[ServerMessage]) -> Option<Vec<(String, u32)>> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Inventory { slots, .. } => {
            Some(slots.iter().flatten().map(|stack| (stack.item.clone(), stack.count)).collect())
        }
        _ => None,
    })
}

#[test]
fn new_characters_get_starting_items_and_keep_them() {
    let mut server = TestServer::with_content("inventory", content_with("[]", ""));
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    client.send(&ClientMessage::CreateCharacter {
        name: "Newcomer".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
    server.run(2);
    let expected = vec![("bread".to_string(), 1)];
    let messages = client.receive();
    assert_eq!(latest_inventory(&messages), Some(expected.clone()));
    assert_eq!(latest_miasma(&messages), Some(25));
    client.disconnect();
    server.run(2);

    let mut server = TestServer::with_profiles(server.shut_down());
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    let messages = client.receive();
    assert_eq!(latest_inventory(&messages), Some(expected), "the bag comes back after a restart");
    assert_eq!(latest_miasma(&messages), Some(25));
}

const SHOPKEEPER: &str = "title: Tom\n---\nTom: Buying?\n-> Yes, please.\n    <<open_shop bakery>>\n===\n";

/// The latest amount of Miasma in a batch of messages.
fn latest_miasma(messages: &[ServerMessage]) -> Option<u64> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Inventory { miasma, .. } => Some(*miasma),
        _ => None,
    })
}

/// The slot holding an item, in the latest inventory in a batch of messages.
fn slot_of(messages: &[ServerMessage], item: &str) -> Option<u32> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Inventory { slots, .. } => {
            slots.iter().position(|slot| slot.as_ref().is_some_and(|stack| stack.item == item)).map(|i| i as u32)
        }
        _ => None,
    })
}

#[test]
fn shops_open_from_dialogue_and_trade_miasma_for_goods() {
    let mut server = TestServer::with_content("shop", content_with_dialogue(TALKER, "", Some(SHOPKEEPER)));
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    client.send(&ClientMessage::CreateCharacter {
        name: "Shopper".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
    server.run(2);
    client.receive();
    let npc = server.npc();

    client.send(&ClientMessage::Interact { target: npc });
    server.run(2);
    client.send(&ClientMessage::ChooseDialogueOption { index: 0 });
    server.run(2);
    let messages = client.receive();
    assert!(messages.contains(&ServerMessage::DialogueEnded), "opening the shop ends the conversation");
    let opened = messages.iter().any(|m| {
        matches!(m, ServerMessage::ShopOpened { name, stock, .. }
        if name == "The Bakery" && stock.len() == 1 && stock[0].price == 5)
    });
    assert!(opened, "the shop opens: {messages:?}");

    // 25 Miasma: two loaves cost 10.
    client.send(&ClientMessage::Buy { index: 0, count: 2 });
    server.run(2);
    let messages = client.receive();
    assert_eq!(notices(&messages), ["You buy: Bread (2) for 10 Miasma."]);
    let loaf = ("bread".to_string(), 1);
    assert_eq!(latest_inventory(&messages), Some(vec![loaf.clone(), loaf.clone(), loaf]));
    assert_eq!(latest_miasma(&messages), Some(15));

    // Bread isn't eaten at full health.
    let bread = slot_of(&messages, "bread").unwrap();
    client.send(&ClientMessage::UseItem { slot: bread });
    server.run(2);
    assert_eq!(notices(&client.receive()), ["You're not hurt."]);

    // Selling a loaf pays half its value of 4.
    client.send(&ClientMessage::Sell { slot: bread, count: 1 });
    client.send(&ClientMessage::Buy { index: 0, count: 5 });
    server.run(2);
    let messages = client.receive();
    assert_eq!(notices(&messages), ["You sell: Bread for 2 Miasma.", "You don't have enough Miasma for that."]);
    assert_eq!(latest_miasma(&messages), Some(17));

    // Walking away closes the shop, and then nothing more can be bought.
    client.send(&ClientMessage::MoveTo { x: 90.0, y: 100.0 });
    server.run(40);
    assert!(client.receive().contains(&ServerMessage::ShopClosed));
    client.send(&ClientMessage::Buy { index: 0, count: 1 });
    server.run(2);
    assert!(notices(&client.receive()).is_empty());
}

const ERRAND: &str = "title: Tom
---
<<if quest_stage(\"errand\") == \"\">>
Tom: Could you bring me a loaf?
<<set_quest errand asked>>
<<elseif quest_stage(\"errand\") == \"asked\" and item_count(\"bread\") > 0>>
<<take_item bread>>
<<set_quest errand done>>
<<give_miasma 30>>
Tom: Thank you!
<<else>>
Tom: Hello again.
<<endif>>
===
";
const CELEBRATION: &str = r#"fn on_quest_stage(npc, player, quest, stage) { if stage == "done" { npc.say("Hooray for " + player.name + "!"); } }"#;

/// The latest journal in a batch of messages: (name, journal, complete) for each quest.
fn latest_journal(messages: &[ServerMessage]) -> Option<Vec<(String, String, bool)>> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Quests { quests } => {
            Some(quests.iter().map(|q| (q.name.clone(), q.journal.clone(), q.complete)).collect())
        }
        _ => None,
    })
}

#[test]
fn quests_start_complete_and_reward_once() {
    let npcs = r#"[(id: "tom", name: "Tom", appearance: "villager", at: (102.0, 100.0), script: Some("greeter.rhai"), dialogue: Some("Tom"))]"#;
    let mut server = TestServer::with_content("quest", content_with_dialogue(npcs, CELEBRATION, Some(ERRAND)));
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    client.send(&ClientMessage::CreateCharacter {
        name: "Helper".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
    server.run(2);
    assert_eq!(latest_journal(&client.receive()), Some(vec![]), "an empty journal to start with");
    let npc = server.npc();

    client.send(&ClientMessage::Interact { target: npc });
    server.run(2);
    let messages = client.receive();
    assert_eq!(notices(&messages), ["New quest: An Errand."]);
    let asked = ("An Errand".to_string(), "Tom wants a loaf.".to_string(), false);
    assert_eq!(latest_journal(&messages), Some(vec![asked]));

    // Handing over the loaf completes it, pays 30 Miasma, and Tom's script celebrates out loud.
    client.send(&ClientMessage::Interact { target: npc });
    server.run(3);
    let messages = client.receive();
    assert_eq!(notices(&messages), ["You hand over: Bread.", "Quest complete: An Errand!", "You receive: 30 Miasma."]);
    assert_eq!(latest_inventory(&messages), Some(vec![]));
    assert_eq!(latest_miasma(&messages), Some(55));
    let done = ("An Errand".to_string(), "I gave Tom a loaf.".to_string(), true);
    assert_eq!(latest_journal(&messages), Some(vec![done.clone()]));
    let cheered = messages.iter().any(|m| {
        matches!(m, ServerMessage::Chat { from, text, .. }
        if *from == npc && text == "Hooray for Helper!")
    });
    assert!(cheered, "the NPC's script reacts: {messages:?}");

    // No second reward.
    client.send(&ClientMessage::Interact { target: npc });
    server.run(2);
    let messages = client.receive();
    assert!(notices(&messages).is_empty());
    assert_eq!(pages(&messages)[0].0, ["Hello again."]);

    // The journal is kept.
    client.disconnect();
    server.run(2);
    let pack = content_with_dialogue(npcs, CELEBRATION, Some(ERRAND));
    let mut server = TestServer::start(server.shut_down(), pack);
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    assert_eq!(latest_journal(&client.receive()), Some(vec![done]), "the journal comes back after a restart");
}

/// The latest character sheet in a batch of messages.
fn latest_sheet(messages: &[ServerMessage]) -> Option<venture_protocol::CharacterSheet> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Character(sheet) => Some(sheet.clone()),
        _ => None,
    })
}

/// Connects and makes a new character of a class, returning the messages
/// received (the welcome and all).
fn create(server: &mut TestServer, token: &str, name: &str, class: &str) -> (TestClient, Vec<ServerMessage>) {
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: token.to_string() });
    server.run(2);
    client.receive();
    client.send(&ClientMessage::CreateCharacter {
        name: name.to_string(),
        class: class.to_string(),
        body: BodyType::Feminine,
        features: Features::default(),
    });
    server.run(2);
    let messages = client.receive();
    (client, messages)
}

#[test]
fn new_characters_choose_a_class_and_a_unique_name() {
    let mut server = TestServer::with_content("classes", content_with("[]", ""));
    let (_, messages) = create(&mut server, TOKEN_A, "Aldric", "mage");
    let sheet = latest_sheet(&messages).expect("a character sheet");
    assert_eq!((sheet.class.as_str(), sheet.level, sheet.stats.intelligence), ("Mage", 1, 15));
    assert_eq!(sheet.max_health, 7, "d6 + CON 12 (+1)");

    // The same name, in any case, is taken; an unknown class is refused.
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_B.to_string() });
    server.run(2);
    let offered = client.receive().into_iter().find_map(|m| match m {
        ServerMessage::CharacterNeeded { classes, .. } => Some(classes.into_iter().map(|c| c.name).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(offered, Some(vec!["Fighter".to_string(), "Mage".to_string()]));
    for (name, class) in [("ALDRIC", "fighter"), ("Brienne", "necromancer")] {
        client.send(&ClientMessage::CreateCharacter {
            name: name.to_string(),
            class: class.to_string(),
            body: BodyType::Feminine,
            features: Features::default(),
        });
        server.run(2);
        assert!(matches!(client.receive()[..], [ServerMessage::CharacterRejected { .. }]), "{name} as a {class}");
    }
    client.send(&ClientMessage::CreateCharacter {
        name: "Brienne".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
        features: Features::default(),
    });
    server.run(2);
    assert!(client.receive().iter().any(|m| matches!(m, ServerMessage::Welcome { admin: false, .. })));
}

#[test]
fn only_admins_can_use_admin_commands() {
    let mut server = TestServer::with_content("admin", content_with("[]", ""));
    let (boss, messages) = create(&mut server, TOKEN_A, "Boss", "fighter");
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::Welcome { admin: true, .. })), "Boss is an admin");
    let (visitor, _) = create(&mut server, TOKEN_B, "Visitor", "fighter");
    boss.receive();

    visitor.send(&ClientMessage::Admin(AdminCommand::SetLevel { level: 30 }));
    server.run(2);
    assert_eq!(notices(&visitor.receive()), ["Only admins can use that command."]);

    // Levelling up: the points come with it, and everyone sees the celebration.
    boss.send(&ClientMessage::Admin(AdminCommand::SetLevel { level: 5 }));
    server.run(2);
    let messages = boss.receive();
    let sheet = latest_sheet(&messages).unwrap();
    assert_eq!((sheet.level, sheet.stat_points, sheet.ability_points), (5, 8, 5));
    assert!(
        visitor.receive().iter().any(|m| matches!(m, ServerMessage::PlayerChanged { level: 5, celebrate: true, .. }))
    );

    // Changing class keeps the level and hands back every point.
    boss.send(&ClientMessage::Admin(AdminCommand::SetClass { class: "MAGE".to_string() }));
    server.run(2);
    let sheet = latest_sheet(&boss.receive()).unwrap();
    assert_eq!((sheet.class.as_str(), sheet.level, sheet.stats.intelligence, sheet.stat_points), ("Mage", 5, 15, 8));

    boss.send(&ClientMessage::Admin(AdminCommand::SetClass { class: "bard".to_string() }));
    boss.send(&ClientMessage::Admin(AdminCommand::Reset));
    server.run(2);
    let messages = boss.receive();
    assert!(notices(&messages)[0].starts_with("There's no class called \"bard\""));
    let sheet = latest_sheet(&messages).unwrap();
    assert_eq!((sheet.level, sheet.stat_points, sheet.ability_points), (1, 0, 0));
    assert!(
        visitor.receive().iter().any(|m| matches!(m, ServerMessage::PlayerChanged { level: 1, celebrate: false, .. }))
    );
}

#[test]
fn waves_are_seen_by_everyone_nearby() {
    let mut server = TestServer::with_content("wave", content_with("[]", ""));
    let (waver, me) = server.join(TOKEN_A, "Waver");
    let (friend, _) = server.join(TOKEN_B, "Friend");
    waver.receive();
    waver.send(&ClientMessage::Emote(venture_protocol::Emote::Wave));
    server.run(2);
    let waved = |messages: Vec<ServerMessage>| {
        messages.contains(&ServerMessage::Emoted { id: me, emote: venture_protocol::Emote::Wave })
    };
    assert!(waved(friend.receive()), "a friend nearby sees the wave");
    assert!(waved(waver.receive()), "and so does the one waving");
}

#[test]
fn jumps_are_seen_nearby_one_at_a_time() {
    let mut server = TestServer::with_content("jump", content_with("[]", ""));
    let (jumper, me) = server.join(TOKEN_A, "Jumper");
    let (friend, _) = server.join(TOKEN_B, "Friend");
    jumper.receive();
    friend.receive();
    let jumps =
        |messages: Vec<ServerMessage>| messages.iter().filter(|m| **m == ServerMessage::Jumped { id: me }).count();

    // A second jump before the first has landed is ignored.
    jumper.send(&ClientMessage::Jump);
    jumper.send(&ClientMessage::Jump);
    server.run(2);
    assert_eq!(jumps(friend.receive()), 1, "a friend nearby sees one jump");
    assert_eq!(jumps(jumper.receive()), 0, "the jumper's own client already showed it");

    // Once it has landed, they can jump again.
    server.run((venture_simulation::JUMP_SECONDS * TICK_RATE) as usize + 1);
    jumper.send(&ClientMessage::Jump);
    server.run(2);
    assert_eq!(jumps(friend.receive()), 1);
}

#[test]
fn admins_can_change_their_body_type() {
    let mut server = TestServer::with_content("body", content_with("[]", ""));
    let (boss, messages) = create(&mut server, TOKEN_A, "Boss", "fighter");
    let welcome_body = messages.iter().find_map(|m| match m {
        ServerMessage::Welcome { look, .. } => Some(look.body),
        _ => None,
    });
    assert_eq!(welcome_body, Some(BodyType::Feminine));
    boss.send(&ClientMessage::Admin(AdminCommand::SetBody { body: BodyType::Masculine }));
    server.run(2);
    let messages = boss.receive();
    assert_eq!(notices(&messages), ["Your character is now masculine."]);
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::PlayerChanged { look, celebrate: false, .. }
        if look.body == BodyType::Masculine)));
}

#[test]
fn classes_start_with_their_gear_and_abilities() {
    let mut server = TestServer::with_content("kit", content_with("[]", ""));
    let (_, messages) = create(&mut server, TOKEN_A, "Aldric", "fighter");
    let sheet = latest_sheet(&messages).unwrap();
    assert_eq!(sheet.abilities, ["rest", "whack"]);
    assert_eq!(
        sheet.equipment,
        [(EquipSlot::DominantHand, "sword".to_string()), (EquipSlot::Body, "chestguard".to_string())]
    );
    assert_eq!(sheet.damage.to_string(), "1d6+2", "the sword, plus STR 15's +2");
    assert_eq!(sheet.armour_class, 13, "10, +1 DEX, +2 chestguard");
    let worn = messages.iter().find_map(|m| match m {
        ServerMessage::Welcome { look, .. } => Some(look.worn.len()),
        _ => None,
    });
    assert_eq!(worn, Some(2), "the sword and chestguard show");

    // The class choices show each class's gear, for the preview.
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_B.to_string() });
    server.run(2);
    let robes = client.receive().into_iter().find_map(|m| match m {
        ServerMessage::CharacterNeeded { classes, .. } => Some(classes[1].worn.len()),
        _ => None,
    });
    assert_eq!(robes, Some(1), "the mage's robe (one item, though it fills two slots)");
}

#[test]
fn gear_comes_off_into_the_bag_and_goes_back_on() {
    let mut server = TestServer::with_content("gear", content_with("[]", ""));
    let (client, _) = create(&mut server, TOKEN_A, "Aldric", "fighter");
    client.send(&ClientMessage::Unequip { slot: EquipSlot::DominantHand });
    server.run(2);
    let messages = client.receive();
    assert_eq!(notices(&messages), ["You take off: Sword."]);
    assert_eq!(latest_sheet(&messages).unwrap().damage.to_string(), "3", "fists again");
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::PlayerChanged { look, .. } if look.worn.len() == 1)));
    let sword = slot_of(&messages, "sword").expect("the sword is in the bag");

    client.send(&ClientMessage::Equip { slot: sword });
    server.run(2);
    let messages = client.receive();
    assert_eq!(notices(&messages), ["You equip: Sword."]);
    assert_eq!(latest_sheet(&messages).unwrap().damage.to_string(), "1d6+2");
    let bread = slot_of(&messages, "bread").unwrap();
    client.send(&ClientMessage::Equip { slot: bread });
    server.run(2);
    assert_eq!(notices(&client.receive()), ["You can't equip that."]);
}

#[test]
fn points_are_spent_on_stats_and_abilities() {
    let mut server = TestServer::with_content("points", content_with("[]", ""));
    let (boss, _) = create(&mut server, TOKEN_A, "Boss", "fighter");
    boss.send(&ClientMessage::LearnAbility { ability: "bash".to_string() });
    server.run(2);
    assert_eq!(notices(&boss.receive()), ["You need to be level 2 to learn that ability."]);

    boss.send(&ClientMessage::Admin(AdminCommand::SetLevel { level: 2 }));
    server.run(2);
    boss.send(&ClientMessage::SpendStatPoint { stat: venture_rpg::Stat::Constitution });
    boss.send(&ClientMessage::LearnAbility { ability: "bash".to_string() });
    server.run(2);
    let messages = boss.receive();
    assert!(notices(&messages).contains(&"You learn Bash.".to_string()));
    let sheet = latest_sheet(&messages).unwrap();
    assert_eq!((sheet.stats.constitution, sheet.stat_points, sheet.ability_points), (15, 1, 0));
    assert_eq!(sheet.abilities, ["rest", "whack", "bash"]);
}

#[test]
fn character_sheets_list_every_skill_starting_at_level_1() {
    let mut server = TestServer::with_content("skills", content_with("[]", ""));
    let (_, messages) = create(&mut server, TOKEN_A, "Angler", "fighter");
    let sheet = latest_sheet(&messages).unwrap();
    let skills: Vec<_> =
        sheet.skills.iter().map(|s| (s.skill.as_str(), s.level, s.experience, s.experience_to_next)).collect();
    assert_eq!(skills, [("fishing", 1, 0, 100), ("mining", 1, 0, 100)]);
}

/// A pack with one monster near the spawn point (at (103, 100)) and a
/// respawn point at a camp.
fn monster_pack(monster: &str) -> ContentPack {
    let spawner = r#"[(id: "one", monster: "beast", at: (103.0, 100.0), respawn_seconds: 1.0)]"#;
    let respawn = r#"[(name: "the camp", at: (80.0, 80.0)), (name: "far away", at: (10.0, 10.0))]"#;
    let extra = [("monsters/beast.ron", monster), ("spawners/one.ron", spawner), ("respawn_points.ron", respawn)];
    content_with_files("[]", "", None, &extra)
}

/// The (first) monster's id.
fn monster_id(server: &mut TestServer) -> EntityId {
    let world = server.app.world_mut();
    let mut monsters = world.query_filtered::<&NetworkId, With<venture_server::monsters::Monster>>();
    monsters.iter(world).next().expect("a monster").0
}

#[test]
fn killing_a_monster_gives_experience_and_it_comes_back() {
    // Easy to hit (armour class 0), and worth a level.
    let rabbit = r#"[(id: "beast", name: "Rabbit", level: 1, health: 4, armour_class: 0,
        attack: (damage: "1", bonus: 0, speed: 0.5), experience: 150,
        look: (shape: Rabbit, main: (0.5, 0.4, 0.3), trim: (0.9, 0.9, 0.9)))]"#;
    let mut server = TestServer::with_content("hunt", monster_pack(rabbit));
    server.run(2);
    let (hunter, messages) = create(&mut server, TOKEN_A, "Hunter", "fighter");
    let me = welcome_id(&messages);
    let rabbit = monster_id(&mut server);

    hunter.send(&ClientMessage::Attack { target: rabbit });
    server.run(200);
    let messages = hunter.receive();
    assert!(messages.iter().any(
        |m| matches!(m, ServerMessage::Attacked { attacker, target, .. } if *attacker == me && *target == rabbit)
    ));
    assert!(messages.contains(&ServerMessage::Died { id: rabbit }));
    let notices = notices(&messages);
    assert!(notices.contains(&"You defeat the Rabbit. (+150 experience)".to_string()), "{notices:?}");
    assert!(notices.contains(&"You reached level 2!".to_string()));
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::PlayerChanged { level: 2, celebrate: true, .. })));
    assert!(messages.contains(&ServerMessage::EntityLeft { id: rabbit }), "the body goes");
    let back = messages.iter().any(
        |m| matches!(m, ServerMessage::EntityAppeared { id, kind: EntityKind::Monster { .. }, .. } if *id != rabbit),
    );
    assert!(back, "and a new rabbit comes");
}

#[test]
fn aggressive_monsters_attack_and_knocked_out_players_wake_at_the_nearest_respawn_point() {
    // It always hits, for far more than anyone's health.
    let goblin = r#"[(id: "beast", name: "Goblin", level: 2, health: 50, armour_class: 12,
        attack: (damage: "100", bonus: 50, speed: 2.0), aggressive: true, experience: 30,
        look: (shape: Goblin, main: (0.3, 0.5, 0.2), trim: (0.4, 0.3, 0.2)))]"#;
    let mut server = TestServer::with_content("ambush", monster_pack(goblin));
    server.run(2);
    let (victim, messages) = create(&mut server, TOKEN_A, "Victim", "fighter");
    let me = welcome_id(&messages);
    assert!(latest_vitals(&messages).is_some_and(|(health, max)| health == max), "full health to start");

    server.run(60);
    let messages = victim.receive();
    assert!(messages.contains(&ServerMessage::Died { id: me }));
    assert!(notices(&messages).contains(&"You were knocked out by the Goblin. You wake up at the camp.".to_string()));
    let position = messages.iter().rev().find_map(|m| match m {
        ServerMessage::Snapshot { entities, .. } => entities.iter().find(|e| e.id == me).map(|e| (e.x, e.y)),
        _ => None,
    });
    assert_eq!(position, Some((80.0, 80.0)), "at the camp, the nearer of the two");
    // (One blow took them from full health to none, and they woke at full
    // health in the same tick, so their vitals may not have visibly changed.)
    assert!(latest_vitals(&messages).is_none_or(|(health, max)| health == max), "with full health again");
}

/// The latest vitals in a batch of messages: (health, most health).
fn latest_vitals(messages: &[ServerMessage]) -> Option<(u32, u32)> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Vitals { health, max_health, .. } => Some((*health, *max_health)),
        _ => None,
    })
}

/// The latest mana in a batch of messages: (mana, most mana).
fn latest_mana(messages: &[ServerMessage]) -> Option<(u32, u32)> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Vitals { mana, max_mana, .. } => Some((*mana, *max_mana)),
        _ => None,
    })
}

/// A monster that's easy to hit and hard to kill, and barely fights back.
const PUNCHBAG: &str = r#"[(id: "beast", name: "Punchbag", level: 1, health: 500, armour_class: 0,
    attack: (damage: "1", bonus: -20, speed: 0.2), experience: 1,
    look: (shape: Rabbit, main: (0.5, 0.4, 0.3), trim: (0.9, 0.9, 0.9)))]"#;

/// Whether a batch of messages has an attack by `attacker` on `target` with this ability.
fn attacked_with(messages: &[ServerMessage], attacker: EntityId, target: EntityId, ability: Option<&str>) -> bool {
    messages.iter().any(|m| {
        matches!(m, ServerMessage::Attacked { attacker: a, target: t, ability: s, .. }
            if *a == attacker && *t == target && s.as_deref() == ability)
    })
}

#[test]
fn weapon_abilities_run_up_then_hit_and_cool_down() {
    let mut server = TestServer::with_content("whack", monster_pack(PUNCHBAG));
    server.run(2);
    let (fighter, messages) = create(&mut server, TOKEN_A, "Hitter", "fighter");
    let me = welcome_id(&messages);
    assert_eq!(latest_mana(&messages), Some((3, 3)));
    let beast = monster_id(&mut server);

    // It's 3 m away, a little out of reach: run up, then whack it.
    fighter.send(&ClientMessage::UseAbility { ability: "whack".to_string(), target: Some(beast) });
    server.run(30);
    let messages = fighter.receive();
    let used = ServerMessage::AbilityUsed { user: me, ability: "whack".to_string(), target: Some(beast) };
    assert!(messages.contains(&used));
    assert!(attacked_with(&messages, me, beast, Some("whack")));
    assert_eq!(latest_mana(&messages), Some((1, 3)), "it cost 2 mana");

    // Then they keep attacking with the weapon, and the ability needs a rest.
    server.run(30);
    assert!(attacked_with(&fighter.receive(), me, beast, None));
    fighter.send(&ClientMessage::UseAbility { ability: "whack".to_string(), target: Some(beast) });
    server.run(2);
    assert_eq!(notices(&fighter.receive()), ["Whack isn't ready yet."]);

    // Abilities they don't know are refused.
    fighter.send(&ClientMessage::UseAbility { ability: "blast".to_string(), target: Some(beast) });
    server.run(2);
    assert_eq!(notices(&fighter.receive()), ["You don't know Blast."]);
}

#[test]
fn spells_fly_to_their_target_and_land_when_they_arrive() {
    let mut server = TestServer::with_content("blast", monster_pack(PUNCHBAG));
    server.run(2);
    let (mage, messages) = create(&mut server, TOKEN_A, "Caster", "mage");
    let me = welcome_id(&messages);
    let beast = monster_id(&mut server);

    // 3 m at 10 m/s: it lands 0.3 seconds (6 ticks) later.
    mage.send(&ClientMessage::UseAbility { ability: "blast".to_string(), target: Some(beast) });
    server.run(3);
    let messages = mage.receive();
    assert!(messages.contains(&ServerMessage::AbilityUsed {
        user: me,
        ability: "blast".to_string(),
        target: Some(beast)
    }));
    assert!(!attacked_with(&messages, me, beast, Some("blast")), "still flying");
    assert_eq!(latest_mana(&messages), Some((7, 10)));
    server.run(10);
    assert!(attacked_with(&mage.receive(), me, beast, Some("blast")));
}

#[test]
fn resting_sits_down_until_moving_but_not_in_a_fight() {
    let mut server = TestServer::with_content("rest", monster_pack(PUNCHBAG));
    server.run(2);
    let (rester, messages) = create(&mut server, TOKEN_A, "Sleepy", "fighter");
    let me = welcome_id(&messages);
    let resting = |messages: &[ServerMessage]| {
        messages.iter().rev().find_map(|m| match m {
            ServerMessage::Snapshot { entities, .. } => entities.iter().find(|e| e.id == me).map(|e| e.resting),
            _ => None,
        })
    };

    rester.send(&ClientMessage::UseAbility { ability: "rest".to_string(), target: None });
    server.run(3);
    assert_eq!(resting(&rester.receive()), Some(true));
    rester.send(&ClientMessage::MoveTo { x: 100.0, y: 98.0 });
    server.run(3);
    assert_eq!(resting(&rester.receive()), Some(false), "moving stands them up");

    let beast = monster_id(&mut server);
    rester.send(&ClientMessage::Attack { target: beast });
    server.run(40);
    rester.send(&ClientMessage::UseAbility { ability: "rest".to_string(), target: None });
    server.run(2);
    assert!(notices(&rester.receive()).contains(&"You can't rest in the middle of a fight.".to_string()));
}

#[test]
fn a_monster_dies_only_once_however_many_blows_land_together() {
    let frail = r#"[(id: "beast", name: "Moth", level: 1, health: 1, armour_class: 0,
        attack: (damage: "1", bonus: 0, speed: 0.1), experience: 5,
        look: (shape: Rabbit, main: (0.5, 0.4, 0.3), trim: (0.9, 0.9, 0.9)))]"#;
    let mut server = TestServer::with_content("overkill", monster_pack(frail));
    server.run(2);
    let (hunter, _) = create(&mut server, TOKEN_A, "Swatter", "fighter");
    let world = server.app.world_mut();
    let player = world.query_filtered::<Entity, With<venture_server::entities::Player>>().single(world).unwrap();
    let moth = world.query_filtered::<Entity, With<venture_server::monsters::Monster>>().single(world).unwrap();
    // Five blows in one tick: (almost certainly) at least one hits, but it only dies once.
    for _ in 0..5 {
        world.write_message(venture_server::combat::Strike { attacker: player, target: moth, ability: None });
    }
    server.run(2);
    let defeats = notices(&hunter.receive()).into_iter().filter(|notice| notice.starts_with("You defeat")).count();
    assert_eq!(defeats, 1);
}

/// The latest hotbar in a batch of messages.
fn latest_hotbar(messages: &[ServerMessage]) -> Option<Vec<Option<HotbarEntry>>> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Hotbar { slots } => Some(slots.clone()),
        _ => None,
    })
}

#[test]
fn hotbars_start_with_attack_and_abilities_keep_only_known_abilities_and_are_saved() {
    let pack = content_with("[]", "");
    let mut server = TestServer::with_content("hotbar", pack.clone());
    let (fighter, messages) = create(&mut server, TOKEN_A, "Keys", "fighter");
    let ability = |id: &str| Some(HotbarEntry::Ability(id.to_string()));
    let mut expected = vec![Some(HotbarEntry::Action(HotbarAction::Attack)), ability("rest"), ability("whack")];
    expected.resize(10, None);
    assert_eq!(latest_hotbar(&messages), Some(expected));

    // Blast isn't theirs, so it comes off (and they're told).
    let wave = Some(HotbarEntry::Action(HotbarAction::Wave));
    fighter.send(&ClientMessage::SetHotbar { slots: vec![ability("whack"), ability("blast"), wave.clone()] });
    server.run(2);
    let mut expected = vec![ability("whack"), None, wave];
    expected.resize(10, None);
    assert_eq!(latest_hotbar(&fighter.receive()), Some(expected.clone()));

    fighter.disconnect();
    server.run(2);
    let mut server = TestServer::start(server.shut_down(), pack);
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    assert_eq!(latest_hotbar(&client.receive()), Some(expected), "kept");
}

#[test]
fn learned_abilities_go_on_the_hotbar() {
    let mut server = TestServer::with_content("learning", content_with("[]", ""));
    let (boss, _) = create(&mut server, TOKEN_A, "Boss", "fighter");
    boss.send(&ClientMessage::Admin(AdminCommand::SetLevel { level: 2 }));
    server.run(2);
    boss.receive();
    boss.send(&ClientMessage::LearnAbility { ability: "bash".to_string() });
    server.run(2);
    let slots = latest_hotbar(&boss.receive()).expect("a new hotbar");
    assert_eq!(slots[3], Some(HotbarEntry::Ability("bash".to_string())));
}

/// The player's own id, from the welcome in a batch of messages.
fn welcome_id(messages: &[ServerMessage]) -> EntityId {
    messages
        .iter()
        .find_map(|m| if let ServerMessage::Welcome { you, .. } = m { Some(*you) } else { None })
        .expect("a welcome")
}

/// The latest party a player was sent (`Some(None)`: they were told they're in none).
fn latest_party(messages: &[ServerMessage]) -> Option<Option<venture_protocol::PartyInfo>> {
    messages.iter().rev().find_map(|m| match m {
        ServerMessage::Party(party) => Some(party.clone()),
        _ => None,
    })
}

#[test]
fn players_form_parties_by_invitation() {
    use venture_protocol::PartyCommand;
    let mut server = TestServer::new("parties");
    let (ada, _) = server.join(TOKEN_A, "Ada");
    let (bram, bram_id) = server.join(TOKEN_B, "Bram");
    server.run(1);
    ada.receive();
    bram.receive();

    // A party name that isn't allowed is explained.
    ada.send(&ClientMessage::Party(PartyCommand::Create { name: "<b>".to_string() }));
    server.run(1);
    assert_eq!(notices(&ada.receive()), ["Party names must start with a letter or a digit."]);

    ada.send(&ClientMessage::Party(PartyCommand::Create { name: "Wanderers".to_string() }));
    server.run(1);
    let messages = ada.receive();
    assert_eq!(notices(&messages), ["You have created the party Wanderers."]);
    let party = latest_party(&messages).flatten().expect("Ada's party");
    assert_eq!((party.name.as_str(), party.members.len()), ("Wanderers", 1));

    // Inviting by the entity (the "Invite target" button).
    ada.send(&ClientMessage::Party(PartyCommand::Invite { target: bram_id }));
    server.run(1);
    assert_eq!(notices(&ada.receive()), ["Inviting Bram to party Wanderers..."]);
    let messages = bram.receive();
    assert_eq!(notices(&messages), ["You have been invited to party Wanderers."]);
    assert!(messages.iter().any(|m| matches!(m, ServerMessage::PartyInvite(Some(invite)) if invite.from == "Ada")));

    bram.send(&ClientMessage::Party(PartyCommand::Accept));
    server.run(1);
    let messages = bram.receive();
    assert!(notices(&messages).contains(&"You have joined Wanderers.".to_string()));
    assert!(messages.contains(&ServerMessage::PartyInvite(None)));
    let party = latest_party(&messages).flatten().expect("Bram's party");
    let names: Vec<&str> = party.members.iter().map(|member| member.name.as_str()).collect();
    assert_eq!(names, ["Ada", "Bram"]);
    assert!(party.members.iter().all(|member| member.max_health > 0 && member.level == 1));
    assert!(notices(&ada.receive()).contains(&"Bram accepted the party invite.".to_string()));

    // Leaving the game leaves the party.
    bram.disconnect();
    server.run(2);
    let messages = ada.receive();
    assert!(notices(&messages).contains(&"Bram has left the party.".to_string()));
    let party = latest_party(&messages).flatten().expect("Ada's party");
    assert_eq!(party.members.len(), 1);
}

#[test]
fn party_invitations_run_out() {
    use venture_protocol::PartyCommand;
    let mut server = TestServer::new("party-invitations");
    let (ada, _) = server.join(TOKEN_A, "Ada");
    let (bram, _) = server.join(TOKEN_B, "Bram");
    ada.send(&ClientMessage::Party(PartyCommand::Create { name: "Wanderers".to_string() }));
    // Inviting by name, as `/invite bram` does.
    ada.send(&ClientMessage::Party(PartyCommand::InviteNamed { name: "bram".to_string() }));
    server.run(1);
    bram.receive();
    ada.receive();
    server.run((15.0 * TICK_RATE) as usize + 1);
    let messages = bram.receive();
    assert!(notices(&messages).contains(&"Your invitation to join Wanderers has expired.".to_string()));
    assert!(messages.contains(&ServerMessage::PartyInvite(None)));
    assert!(notices(&ada.receive()).contains(&"Your party invite to Bram has expired.".to_string()));
    bram.send(&ClientMessage::Party(PartyCommand::Accept));
    server.run(1);
    assert_eq!(notices(&bram.receive()), ["You don't have a party invitation."]);
}

#[test]
fn party_members_nearby_share_experience_equally() {
    use venture_protocol::PartyCommand;
    let rabbit = r#"[(id: "beast", name: "Rabbit", level: 1, health: 4, armour_class: 0,
        attack: (damage: "1", bonus: 0, speed: 0.5), experience: 31,
        look: (shape: Rabbit, main: (0.5, 0.4, 0.3), trim: (0.9, 0.9, 0.9)))]"#;
    let mut server = TestServer::with_content("party-experience", monster_pack(rabbit));
    server.run(2);
    let (ada, _) = create(&mut server, TOKEN_A, "Ada", "fighter");
    let (bram, _) = create(&mut server, TOKEN_B, "Bram", "fighter");
    let (cole, _) = create(&mut server, "test-token-cccccccccccccccccccccccccccccccc", "Cole", "fighter");
    ada.send(&ClientMessage::Party(PartyCommand::Create { name: "Hunters".to_string() }));
    server.run(1);
    for (name, member) in [("Bram", &bram), ("Cole", &cole)] {
        ada.send(&ClientMessage::Party(PartyCommand::InviteNamed { name: name.to_string() }));
        server.run(1);
        member.send(&ClientMessage::Party(PartyCommand::Accept));
        server.run(1);
    }
    // Cole wanders off, beyond sharing distance (all three start at (100, 100)).
    // (One click goes at most 30 m in these tests.)
    for y in [128.0, 156.0, 184.0] {
        cole.send(&ClientMessage::MoveTo { x: 100.0, y });
        server.run(150);
    }
    for client in [&ada, &bram, &cole] {
        client.receive();
    }

    let rabbit = monster_id(&mut server);
    ada.send(&ClientMessage::Attack { target: rabbit });
    server.run(200);
    // 31 between the two nearby: the killer gets the odd point.
    let told = notices(&ada.receive());
    assert!(told.contains(&"You defeat the Rabbit. (+16 experience)".to_string()), "{told:?}");
    assert!(notices(&bram.receive()).contains(&"Ada defeats the Rabbit. (+15 experience)".to_string()));
    assert!(!notices(&cole.receive()).iter().any(|notice| notice.contains("Rabbit")), "too far away to share");
}

#[test]
fn a_player_catches_a_fish_with_rod_reel_and_worm() {
    let mut server = TestServer::fishing("fishing");
    let (client, _) = server.join(TOKEN_A, "Boss");
    for (item, count) in [("ledger_rod", 1), ("wooden_reel", 1), ("worm", 3)] {
        client.send(&ClientMessage::Admin(AdminCommand::Give { item: item.to_string(), count }));
    }
    server.run(2);
    let slot = client
        .receive()
        .into_iter()
        .rev()
        .find_map(|m| match m {
            ServerMessage::Inventory { slots, .. } => {
                slots.iter().position(|slot| slot.as_ref().is_some_and(|stack| stack.item == "ledger_rod"))
            }
            _ => None,
        })
        .expect("the rod is in the bag");

    // Casting needs the rod in hand.
    let cast = ClientMessage::Fishing(FishingCommand::Cast { x: 106.0, y: 100.0, tackle: "worm".to_string() });
    client.send(&cast);
    server.run(1);
    assert!(notices(&client.receive()).iter().any(|text| text.contains("rod in your hand")));
    client.send(&ClientMessage::Equip { slot: slot as u32 });
    server.run(1);
    // And water.
    client.send(&ClientMessage::Fishing(FishingCommand::Cast { x: 101.0, y: 100.0, tackle: "worm".to_string() }));
    server.run(1);
    assert!(notices(&client.receive()).iter().any(|text| text.contains("into water")));

    client.send(&cast);
    server.run(1);
    assert!(client.receive().contains(&ServerMessage::Fishing(FishingState::Waiting {
        at: (106.0, 100.0),
        method: venture_rpg::fishing::Method::Ledger
    })));

    // Wait for the bite (a dace, the only fish a beginner can catch there),
    // strike, then reel only while the fish rests.
    let mut caught = None;
    for _ in 0..(TICK_RATE as usize * 120) {
        server.run(1);
        for message in client.receive() {
            match message {
                ServerMessage::Fishing(FishingState::Bite) => {
                    client.send(&ClientMessage::Fishing(FishingCommand::Strike));
                }
                ServerMessage::Fishing(FishingState::Fight(fight)) => {
                    client.send(&ClientMessage::Fishing(FishingCommand::Reel(!fight.running)));
                }
                ServerMessage::Fishing(FishingState::Caught { item }) => caught = Some(item),
                ServerMessage::Fishing(FishingState::Ended) => {}
                ServerMessage::Notice { text } if caught.is_some() => {
                    assert!(text.starts_with("You catch a dace! (+110 fishing experience)"), "{text}");
                }
                ServerMessage::Notice { text } => panic!("fishing ended: {text}"),
                ServerMessage::Character(sheet) if caught.is_some() => {
                    let fishing = sheet.skills.iter().find(|skill| skill.skill == "fishing").unwrap();
                    assert_eq!(
                        (fishing.level, fishing.experience),
                        (2, 10),
                        "110 experience: level 2, and 10 towards 3"
                    );
                    return;
                }
                _ => {}
            }
        }
    }
    panic!("no fish landed in two minutes (caught: {caught:?})");
}

