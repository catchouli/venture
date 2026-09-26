//! End-to-end tests of the game server: real server systems, driven through
//! the in-memory link by test clients speaking the real protocol.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use venture_content::ContentPack;
use venture_protocol::{
    AdminCommand, BodyType, ClientMessage, EntityId, EntityKind, EquipSlot, PROTOCOL_VERSION, ServerMessage,
};
use venture_server::content::LoadedContent;
use venture_server::entities::NetworkId;
use venture_server::npcs::Npc;
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
        let admins = vec!["Boss".to_string()];
        GameServer { world: flat_world(), profiles, link, rules, content, spawn_override: None, admins }
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
    });
    server.run(2);
    assert!(matches!(client.receive()[..], [ServerMessage::CharacterRejected { .. }]));

    client.send(&ClientMessage::CreateCharacter {
        name: "  Aldric  ".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
    });
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
    content_with_dialogue(npcs, script, None)
}

/// Like [`content_with`], plus one dialogue file. Every test pack also has
/// bread, a bakery selling bread for 5 Miasma, a two-stage quest ("errand"),
/// and new characters start with 25 Miasma and a loaf. There are two
/// classes, Fighter and Mage.
fn content_with_dialogue(npcs: &str, script: &str, dialogue: Option<&str>) -> ContentPack {
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
    write("skills/skills.ron", SKILLS.as_bytes());
    write("shops/bakery.ron", br#"[(id: "bakery", name: "The Bakery", stock: [(item: "bread", price: 5)])]"#);
    write(
        "quests/errand.ron",
        br#"[(id: "errand", name: "An Errand", stages: [(id: "asked", journal: "Tom wants a loaf."), (id: "done", journal: "I gave Tom a loaf.")])]"#,
    );
    write("new_characters.ron", br#"(miasma: 25, items: [(item: "bread", count: 1)], skills: ["rest"])"#);
    // Only the start of a PNG, but that's all the content check looks at.
    write("icons/thing.png", b"\x89PNG\r\n\x1a\n");
    if let Some(dialogue) = dialogue {
        write("dialogue/town.yarn", dialogue.as_bytes());
    }
    let pack = ContentPack::load(&folder).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    pack
}

const ITEMS: &str = r#"[
    (id: "bread", name: "Bread", description: "A loaf.", icon: "thing.png", value: 4),
    (id: "sword", name: "Sword", description: "Sharp.", icon: "thing.png", value: 10,
     equip: Some((slots: [Weapon], weapon: Some((damage: "1d6", speed: 1.0, ability: Strength)),
                  look: (style: Sword, main: (0.7, 0.7, 0.7), trim: (0.3, 0.2, 0.1))))),
    (id: "chestguard", name: "Chestguard", description: "Wooden.", icon: "thing.png", value: 6,
     equip: Some((slots: [Body], armour: 2, look: (style: Plates, main: (0.6, 0.4, 0.3), trim: (0.3, 0.2, 0.1))))),
    (id: "robe", name: "Robe", description: "Long.", icon: "thing.png", value: 6,
     equip: Some((slots: [Body, Legs], look: (style: Robe, main: (0.3, 0.3, 0.5), trim: (0.8, 0.7, 0.4))))),
]"#;

const CLASSES: &str = r#"[
    (id: "fighter", name: "Fighter", description: "Hits things.",
     rules: (hit_die: 10, mana_per_level: 3, casting_ability: Wisdom),
     stats: (strength: 15, dexterity: 12, constitution: 14, intelligence: 8, wisdom: 10),
     starting_equipment: ["sword", "chestguard"], starting_skills: ["whack"],
     skills: [(skill: "bash", level: 2, cost: 1)]),
    (id: "mage", name: "Mage", description: "Casts things.",
     rules: (hit_die: 6, mana_per_level: 8, casting_ability: Intelligence),
     stats: (strength: 8, dexterity: 12, constitution: 12, intelligence: 15, wisdom: 13),
     starting_equipment: ["robe"], starting_skills: ["blast"]),
]"#;

const SKILLS: &str = r#"[
    (id: "rest", name: "Rest", description: "Sit down.", icon: "thing.png", effect: Rest),
    (id: "whack", name: "Whack", description: "A heavy blow.", icon: "thing.png", mana: 2, effect: Strike(damage: "1d8")),
    (id: "bash", name: "Bash", description: "Another blow.", icon: "thing.png", effect: Strike(damage: "1d6")),
    (id: "blast", name: "Blast", description: "Whoosh.", icon: "thing.png", effect: Bolt(damage: "1d8")),
]"#;

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
    client.send(&ClientMessage::CreateCharacter {
        name: "Visitor".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
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
    let npcs = r#"[(id: "crier", name: "Town Crier", appearance: "villager", at: Position((110.0, 100.0)), script: Some("greeter.rhai"))]"#;
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
    r#"[(id: "talker", name: "Tom", appearance: "villager", at: Position((102.0, 100.0)), dialogue: Some("Tom"))]"#;
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

    // Selling a loaf pays half its value of 4.
    let bread = slot_of(&messages, "bread").unwrap();
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
    let npcs = r#"[(id: "tom", name: "Tom", appearance: "villager", at: Position((102.0, 100.0)), script: Some("greeter.rhai"), dialogue: Some("Tom"))]"#;
    let mut server = TestServer::with_content("quest", content_with_dialogue(npcs, CELEBRATION, Some(ERRAND)));
    let client = server.connect();
    client.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: TOKEN_A.to_string() });
    server.run(2);
    client.send(&ClientMessage::CreateCharacter {
        name: "Helper".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
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
        });
        server.run(2);
        assert!(matches!(client.receive()[..], [ServerMessage::CharacterRejected { .. }]), "{name} as a {class}");
    }
    client.send(&ClientMessage::CreateCharacter {
        name: "Brienne".to_string(),
        class: "fighter".to_string(),
        body: BodyType::Androgynous,
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
    assert_eq!((sheet.level, sheet.stat_points, sheet.skill_points), (5, 8, 5));
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
    assert_eq!((sheet.level, sheet.stat_points, sheet.skill_points), (1, 0, 0));
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
fn classes_start_with_their_gear_and_skills() {
    let mut server = TestServer::with_content("kit", content_with("[]", ""));
    let (_, messages) = create(&mut server, TOKEN_A, "Aldric", "fighter");
    let sheet = latest_sheet(&messages).unwrap();
    assert_eq!(sheet.skills, ["rest", "whack"]);
    assert_eq!(
        sheet.equipment,
        [(EquipSlot::Weapon, "sword".to_string()), (EquipSlot::Body, "chestguard".to_string())]
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
    client.send(&ClientMessage::Unequip { slot: EquipSlot::Weapon });
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
fn points_are_spent_on_stats_and_skills() {
    let mut server = TestServer::with_content("points", content_with("[]", ""));
    let (boss, _) = create(&mut server, TOKEN_A, "Boss", "fighter");
    boss.send(&ClientMessage::LearnSkill { skill: "bash".to_string() });
    server.run(2);
    assert_eq!(notices(&boss.receive()), ["You need to be level 2 to learn that skill."]);

    boss.send(&ClientMessage::Admin(AdminCommand::SetLevel { level: 2 }));
    server.run(2);
    boss.send(&ClientMessage::SpendStatPoint { ability: venture_rpg::Ability::Constitution });
    boss.send(&ClientMessage::LearnSkill { skill: "bash".to_string() });
    server.run(2);
    let messages = boss.receive();
    assert!(notices(&messages).contains(&"You learn Bash.".to_string()));
    let sheet = latest_sheet(&messages).unwrap();
    assert_eq!((sheet.stats.constitution, sheet.stat_points, sheet.skill_points), (15, 1, 0));
    assert_eq!(sheet.skills, ["rest", "whack", "bash"]);
}
