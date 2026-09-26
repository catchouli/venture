//! Players joining and leaving. The only code that creates and removes
//! players, and that loads and saves their profiles.

use bevy::prelude::*;
use venture_navigation::Point;
use venture_protocol::{BodyType, ClassChoice, EntityKind, ServerMessage};

use crate::character::{Admin, Character, class_or_default};
use crate::combat::{Health, max_health};
use crate::content::LoadedContent;
use crate::creatures::{Creature, Dead};
use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::equipment::{Equipment, player_look};
use crate::inventory::{Change, Inventory};
use crate::link::{ConnectionId, Link};
use crate::names::random_name;
use crate::npcs::Npc;
use crate::profiles::{Profile, ProfileKey, ProfileStore};
use crate::quests::QuestLog;
use crate::requests::{Connections, SessionRequest, Stage};
use crate::world::ServerWorld;
use crate::{Admins, GameRules, ServerSystems};

/// How often everyone's profile is saved while they play, in seconds.
const AUTOSAVE_INTERVAL: f32 = 60.0;
/// The item that was money before Miasma: old profiles' coins become Miasma.
const OLD_COINS: &str = "coins";

/// Handles joining, leaving and saving.
pub struct SessionsPlugin;

impl Plugin for SessionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnlineProfiles>()
            .init_resource::<AutosaveTimer>()
            .add_systems(FixedUpdate, (handle_session_requests, autosave).chain().in_set(ServerSystems::Sessions))
            .add_systems(Last, save_everyone_on_exit);
    }
}

/// Where new characters start, if not the world's spawn point.
#[derive(Resource)]
pub struct SpawnPoint(pub Option<Point>);

/// The profiles currently in the game, so a character can't be logged in twice.
#[derive(Resource, Default)]
struct OnlineProfiles(std::collections::HashSet<ProfileKey>);

#[derive(Resource)]
struct AutosaveTimer(Timer);

impl Default for AutosaveTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(AUTOSAVE_INTERVAL, TimerMode::Repeating))
    }
}

/// Everything saved about a player in the game.
type SavedParts = (
    &'static Player,
    &'static DisplayName,
    &'static Position,
    &'static Inventory,
    &'static QuestLog,
    &'static Character,
    &'static Equipment,
);

/// The resources joining and leaving need.
#[derive(bevy::ecs::system::SystemParam)]
struct Sessions<'w, 's> {
    commands: Commands<'w, 's>,
    link: ResMut<'w, Link>,
    connections: ResMut<'w, Connections>,
    store: Res<'w, ProfileStore>,
    world: Res<'w, ServerWorld>,
    spawn_point: Res<'w, SpawnPoint>,
    ids: ResMut<'w, EntityIds>,
    online: ResMut<'w, OnlineProfiles>,
    rules: Res<'w, GameRules>,
    admins: Res<'w, Admins>,
    content: Res<'w, LoadedContent>,
    players: Query<'w, 's, (Entity, &'static NetworkId, SavedParts)>,
    npcs: Query<'w, 's, (&'static Npc, &'static NetworkId, &'static DisplayName, &'static Position)>,
    creatures:
        Query<'w, 's, (&'static Creature, &'static NetworkId, &'static DisplayName, &'static Position), Without<Dead>>,
}

fn handle_session_requests(mut requests: MessageReader<SessionRequest>, mut sessions: Sessions) {
    for request in requests.read() {
        match request {
            SessionRequest::Hello { connection, profile } => sessions.log_in(*connection, profile),
            SessionRequest::CreateCharacter { connection, profile, name, class, body } => {
                sessions.create_character(*connection, profile, name, class, *body);
            }
            SessionRequest::Leave { connection } => sessions.leave(*connection),
        }
    }
}

impl Sessions<'_, '_> {
    /// Loads the player's profile and enters the game, or asks for a new
    /// character if they don't have one.
    fn log_in(&mut self, connection: ConnectionId, profile: &ProfileKey) {
        if self.online.0.contains(profile) {
            let rejected = ServerMessage::Rejected { reason: "This character is already in the game.".to_string() };
            self.link.send(connection, &rejected);
            self.link.close(connection);
            return;
        }
        match self.store.load(profile) {
            Some(saved) => self.enter_game(connection, profile.clone(), saved),
            None => self.ask_for_character(connection, profile),
        }
    }

    fn ask_for_character(&mut self, connection: ConnectionId, profile: &ProfileKey) {
        self.set_stage(connection, Stage::AwaitingName { profile: profile.clone() });
        let classes = if self.content.pack.classes.is_empty() {
            vec![class_or_default(&self.content.pack, None)]
        } else {
            self.content.pack.classes.clone()
        };
        let classes = classes
            .into_iter()
            .map(|class| ClassChoice {
                worn: Equipment::starting(&class, &self.content.pack.items).looks(&self.content.pack.items),
                id: class.id,
                name: class.name,
                description: class.description,
            })
            .collect();
        self.link.send(connection, &ServerMessage::CharacterNeeded { suggestion: random_name(), classes });
    }

    /// Makes a new character, if the name is free and the class exists.
    fn create_character(
        &mut self,
        connection: ConnectionId,
        profile: &ProfileKey,
        name: &str,
        class_id: &str,
        body: BodyType,
    ) {
        let pack = &self.content.pack;
        let class_exists = pack.classes.iter().any(|class| class.id == class_id) || pack.classes.is_empty();
        let problem = if !class_exists {
            Some("Please choose one of the classes.".to_string())
        } else if !self.store.claim_name(name, profile) {
            Some(format!("Someone is already called {name}. Please choose another name."))
        } else {
            None
        };
        if let Some(reason) = problem {
            self.set_stage(connection, Stage::AwaitingName { profile: profile.clone() });
            self.link.send(connection, &ServerMessage::CharacterRejected { reason });
            return;
        }

        let class = class_or_default(pack, Some(class_id));
        let position = self.spawn_point.0.unwrap_or(self.world.manifest.spawn_point);
        let mut new_profile = Profile::new(name.to_string(), position);
        let character = Character::new(&class, body, &pack.new_characters.skills);
        write_character(&mut new_profile, &character);
        new_profile.equipment = Some(Equipment::starting(&class, &pack.items).saved());
        let inventory = self.starting_inventory();
        new_profile.inventory = inventory.slots().to_vec();
        new_profile.miasma = inventory.miasma();
        self.store.save(profile, &new_profile);
        self.enter_game(connection, profile.clone(), new_profile);
    }

    /// Puts a character into the game and tells everyone.
    fn enter_game(&mut self, connection: ConnectionId, profile: ProfileKey, mut saved: Profile) {
        convert_old_coins(&mut saved);
        let id = self.ids.allocate();
        let position = Position(if self.world.contains(saved.position) {
            saved.position
        } else {
            self.world.manifest.spawn_point
        });
        let name = DisplayName(saved.name.clone());
        let pack = &self.content.pack;
        let class = class_or_default(pack, saved.class.as_deref());
        let character = read_character(&saved, pack);
        // Characters made before equipment existed get their class's starting kit.
        let equipment = match saved.equipment.take() {
            Some(saved) => Equipment::from_saved(saved),
            None => Equipment::starting(&class, &pack.items),
        };
        let look = player_look(&character, &equipment, &pack.items);
        let health = Health::full(max_health(&character, &equipment, pack));
        let inventory = Inventory::from_saved(saved.inventory, saved.miasma);
        let quests = QuestLog::from_saved(saved.quests);
        let admin = self.admins.0.iter().any(|admin| admin.eq_ignore_ascii_case(&name.0));
        let kind = EntityKind::Player { level: character.level(), look: look.clone() };

        // Tell the newcomer about themselves, and everyone and everything already here.
        let rules = self.rules.0;
        let welcome =
            ServerMessage::Welcome { you: id, name: name.0.clone(), position: position.0, rules, admin, look };
        self.link.send(connection, &welcome);
        for (_, &NetworkId(other), (_, other_name, &other_position, _, _, other_character, other_equipment)) in
            &self.players
        {
            let other_look = player_look(other_character, other_equipment, &self.content.pack.items);
            let other_kind = EntityKind::Player { level: other_character.level(), look: other_look };
            self.link.send(connection, &appeared(other, other_name, other_kind, other_position));
        }
        for (npc, &NetworkId(npc_id), npc_name, &npc_position) in &self.npcs {
            self.link.send(connection, &appeared(npc_id, npc_name, npc.kind(), npc_position));
        }
        for (creature, &NetworkId(creature_id), creature_name, &creature_position) in &self.creatures {
            self.link.send(connection, &appeared(creature_id, creature_name, creature.kind(), creature_position));
        }
        // Tell everyone else about the newcomer.
        let others: Vec<ConnectionId> = self.players.iter().map(|(_, _, (player, ..))| player.connection).collect();
        self.link.broadcast(others.iter().copied(), &appeared(id, &name, kind, position));
        let notice = ServerMessage::Notice { text: format!("{} has joined.", name.0) };
        self.link.broadcast(others.into_iter().chain([connection]), &notice);

        info!("{} joined as {id:?}{}.", name.0, if admin { " (an admin)" } else { "" });
        self.online.0.insert(profile.clone());
        let mut player = self.commands.spawn((
            Player { connection, profile },
            NetworkId(id),
            name,
            position,
            Mover::default(),
            (inventory, quests, character, equipment, health),
        ));
        if admin {
            player.insert(Admin);
        }
        let player = player.id();
        self.set_stage(connection, Stage::Playing { player });
    }

    /// Saves and removes a player whose connection has closed, and tells everyone.
    fn leave(&mut self, connection: ConnectionId) {
        let Some(state) = self.connections.0.remove(&connection) else { return };
        let Stage::Playing { player } = state.stage else { return };
        let Ok((entity, &NetworkId(id), parts)) = self.players.get(player) else { return };
        let (leaver, name, ..) = parts;

        self.store.save(&leaver.profile, &profile_of(parts));
        self.online.0.remove(&leaver.profile);
        let others: Vec<ConnectionId> =
            self.players.iter().filter(|(e, ..)| *e != entity).map(|(_, _, (player, ..))| player.connection).collect();
        self.link.broadcast(others.iter().copied(), &ServerMessage::EntityLeft { id });
        self.link.broadcast(others, &ServerMessage::Notice { text: format!("{} has left.", name.0) });
        info!("{} left.", name.0);
        self.commands.entity(entity).despawn();
    }

    /// What a new character carries (see `new_characters.ron` in the content).
    fn starting_inventory(&self) -> Inventory {
        let pack = &self.content.pack;
        let mut inventory = Inventory::empty();
        let mut change = Change::earn(pack.new_characters.miasma);
        change.give = pack.new_characters.items.iter().map(|stack| (stack.item.clone(), stack.count)).collect();
        if let Err(problem) = inventory.apply(&change, &pack.items) {
            warn!("New characters can't be given their starting items: {problem:?}");
        }
        inventory
    }

    fn set_stage(&mut self, connection: ConnectionId, stage: Stage) {
        if let Some(state) = self.connections.0.get_mut(&connection) {
            state.stage = stage;
        }
    }
}

/// A saved character, given the default class (and its stats) if it was
/// made before classes existed.
fn read_character(saved: &Profile, pack: &venture_content::ContentPack) -> Character {
    let class = class_or_default(pack, saved.class.as_deref());
    let mut character = Character::new(&class, saved.body, &pack.new_characters.skills);
    character.progress =
        venture_rpg::Progress { level: saved.level.clamp(1, venture_rpg::MAX_LEVEL), experience: saved.experience };
    character.stats = saved.stats.filter(venture_rpg::Abilities::is_valid).unwrap_or(class.stats);
    character.stat_points = saved.stat_points;
    character.skill_points = saved.skill_points;
    if let Some(skills) = &saved.skills {
        character.skills.clone_from(skills);
    }
    character
}

fn write_character(profile: &mut Profile, character: &Character) {
    profile.class = Some(character.class.clone());
    profile.body = character.body;
    profile.level = character.progress.level;
    profile.experience = character.progress.experience;
    profile.stats = Some(character.stats);
    profile.stat_points = character.stat_points;
    profile.skill_points = character.skill_points;
    profile.skills = Some(character.skills.clone());
}

/// Turns coins (the money before Miasma) in an old profile's bag into Miasma.
fn convert_old_coins(profile: &mut Profile) {
    for slot in &mut profile.inventory {
        if let Some(stack) = slot.take_if(|stack| stack.item == OLD_COINS) {
            profile.miasma = profile.miasma.saturating_add(u64::from(stack.count));
        }
    }
}

fn autosave(time: Res<Time>, mut timer: ResMut<AutosaveTimer>, store: Res<ProfileStore>, players: Query<SavedParts>) {
    if timer.0.tick(time.delta()).just_finished() {
        save_all(&store, &players);
    }
}

/// Saves everyone when the server shuts down (such as on Ctrl+C).
fn save_everyone_on_exit(
    mut exits: MessageReader<AppExit>,
    store: Option<Res<ProfileStore>>,
    players: Query<SavedParts>,
) {
    if exits.read().next().is_some()
        && let Some(store) = store
    {
        save_all(&store, &players);
        info!("Saved everyone's profiles.");
    }
}

fn save_all(store: &ProfileStore, players: &Query<SavedParts>) {
    for parts in players {
        store.save(&parts.0.profile, &profile_of(parts));
    }
}

/// What to save about a player.
fn profile_of(
    (_, name, &Position(position), inventory, quests, character, equipment): (
        &Player,
        &DisplayName,
        &Position,
        &Inventory,
        &QuestLog,
        &Character,
        &Equipment,
    ),
) -> Profile {
    let mut profile = Profile::new(name.0.clone(), position);
    write_character(&mut profile, character);
    profile.equipment = Some(equipment.saved());
    profile.inventory = inventory.slots().to_vec();
    profile.miasma = inventory.miasma();
    profile.quests = quests.saved();
    profile
}

#[cfg(test)]
mod tests {
    use venture_protocol::ItemStack;

    use super::*;

    #[test]
    fn old_coins_become_miasma() {
        let mut profile = Profile::new("Aldric".to_string(), (0.0, 0.0));
        let stack = |item: &str, count| Some(ItemStack { item: item.to_string(), count });
        profile.inventory = vec![stack("coins", 25), stack("bread", 1), stack("coins", 5)];
        profile.miasma = 2;
        convert_old_coins(&mut profile);
        assert_eq!(profile.miasma, 32);
        assert_eq!(profile.inventory, vec![None, stack("bread", 1), None]);
    }
}
