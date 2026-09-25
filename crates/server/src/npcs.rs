//! NPCs: created from the content pack, and driven by their scripts. The only
//! code that creates, removes and moves NPCs.
//!
//! Scripts ask for things by queueing [`ScriptCommand`]s (see
//! [`crate::scripting`]); [`apply_script_commands`] checks each one before
//! acting on it.

use std::ops::Range;

use bevy::prelude::*;
use venture_content::script_hooks::{ON_IDLE, ON_INTERACT, ON_QUEST_STAGE, ON_SPAWN};
use venture_content::{NpcDef, Placement};
use venture_navigation::Point;
use venture_protocol::{EntityKind, ServerMessage, clean_chat_text};
use venture_simulation::plan_route;
use venture_world::WorldManifest;

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::link::Link;
use crate::quests::QuestAdvanced;
use crate::scripting::{NpcView, PlayerView, ScriptCommand, Scripts};
use crate::world::ServerWorld;

/// Players within this distance (in metres) hear what an NPC says.
const SPEECH_RANGE: f32 = 40.0;
/// How long an idle NPC waits before its `on_idle` hook runs, in seconds.
const IDLE_SECONDS: Range<f32> = 4.0..10.0;
/// How many random spots to try when looking for somewhere to wander.
const WANDER_ATTEMPTS: usize = 10;

/// Creates NPCs from content and runs their behaviour.
pub struct NpcsPlugin;

impl Plugin for NpcsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<NpcInteraction>().add_systems(
            FixedUpdate,
            (match_content, run_interactions, run_quest_hooks, run_idle_hooks, apply_script_commands)
                .chain()
                .in_set(ServerSystems::Npcs),
        );
    }
}

/// An NPC.
#[derive(Component)]
pub struct Npc {
    /// The content definition it was made from.
    pub definition: NpcDef,
    /// Where it lives, in metres.
    pub home: Point,
    /// Counts down to its next `on_idle` hook.
    idle: Timer,
}

impl Npc {
    /// What kind of entity this is, as clients are told.
    pub fn kind(&self) -> EntityKind {
        EntityKind::Npc { appearance: self.definition.appearance.clone() }
    }
}

/// A player has started talking to an NPC (they're close enough).
#[derive(Message)]
pub struct NpcInteraction {
    /// The NPC.
    pub npc: Entity,
    /// The player.
    pub player: Entity,
}

/// Where a placement is in the world, if its settlement exists.
pub fn resolve_placement(placement: &Placement, manifest: &WorldManifest) -> Option<Point> {
    let (settlement, (dx, dy)) = match placement {
        Placement::Position(position) => return Some(*position),
        Placement::StartingSettlement { offset } => (manifest.starting_settlement()?, offset),
        Placement::Settlement { index, offset } => (manifest.settlements.get(*index)?, offset),
    };
    Some((settlement.position.0 + dx, settlement.position.1 + dy))
}

/// Makes the NPCs in the world match the content pack whenever it changes:
/// new NPCs appear, removed ones leave, and changed ones are replaced. An NPC
/// whose only change is its script keeps going, with the new script.
fn match_content(
    mut commands: Commands,
    content: Res<LoadedContent>,
    world: Res<ServerWorld>,
    scripts: Res<Scripts>,
    mut ids: ResMut<EntityIds>,
    mut link: ResMut<Link>,
    players: Query<&Player>,
    mut npcs: Query<(Entity, &mut Npc, &NetworkId)>,
) {
    if !content.is_changed() {
        return;
    }
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    let mut kept = std::collections::HashSet::new();

    for (entity, mut npc, &NetworkId(id)) in &mut npcs {
        let wanted = content.pack.npcs.iter().find(|def| def.id == npc.definition.id);
        match wanted {
            Some(def) if *def == npc.definition => {
                kept.insert(def.id.clone());
            }
            Some(def) if NpcDef { script: npc.definition.script.clone(), ..def.clone() } == npc.definition => {
                npc.definition.script.clone_from(&def.script);
                kept.insert(def.id.clone());
            }
            _ => {
                link.broadcast(everyone.iter().copied(), &ServerMessage::EntityLeft { id });
                commands.entity(entity).despawn();
            }
        }
    }

    for def in content.pack.npcs.iter().filter(|def| !kept.contains(&def.id)) {
        let Some(home) = resolve_placement(&def.at, &world.manifest) else {
            warn!("NPC \"{}\" is placed at a settlement the world doesn't have; skipping it.", def.id);
            continue;
        };
        if world.is_blocked(home.0.floor() as i32, home.1.floor() as i32) {
            warn!("NPC \"{}\" stands somewhere that can't be walked on (at {home:?}).", def.id);
        }
        let id = ids.allocate();
        let name = DisplayName(def.name.clone());
        let npc = Npc { definition: def.clone(), home, idle: idle_timer() };
        link.broadcast(everyone.iter().copied(), &appeared(id, &name, npc.kind(), Position(home)));
        let entity = commands.spawn((npc, NetworkId(id), name.clone(), Position(home), Mover::default())).id();
        if let Some(script) = &def.script {
            let view = NpcView { entity, name: name.0, position: home, home };
            scripts.call(script, ON_SPAWN, (view,));
        }
    }
}

/// Runs `on_interact` when a player talks to an NPC.
fn run_interactions(
    mut interactions: MessageReader<NpcInteraction>,
    scripts: Res<Scripts>,
    npcs: Query<(&Npc, &DisplayName, &Position)>,
    players: Query<(&DisplayName, &Position), With<Player>>,
) {
    for interaction in interactions.read() {
        let Ok((npc, npc_name, &Position(position))) = npcs.get(interaction.npc) else { continue };
        let Ok((player_name, &Position(player_position))) = players.get(interaction.player) else { continue };
        let Some(script) = &npc.definition.script else { continue };
        let npc_view = NpcView { entity: interaction.npc, name: npc_name.0.clone(), position, home: npc.home };
        let player_view = PlayerView { name: player_name.0.clone(), position: player_position };
        scripts.call(script, ON_INTERACT, (npc_view, player_view));
    }
}

/// Runs `on_quest_stage` when a conversation with an NPC moved a player's quest on.
fn run_quest_hooks(
    mut advances: MessageReader<QuestAdvanced>,
    scripts: Res<Scripts>,
    npcs: Query<(&Npc, &DisplayName, &Position)>,
    players: Query<(&DisplayName, &Position), With<Player>>,
) {
    for advance in advances.read() {
        let Ok((npc, npc_name, &Position(position))) = npcs.get(advance.npc) else { continue };
        let Ok((player_name, &Position(player_position))) = players.get(advance.player) else { continue };
        let Some(script) = &npc.definition.script else { continue };
        let npc_view = NpcView { entity: advance.npc, name: npc_name.0.clone(), position, home: npc.home };
        let player_view = PlayerView { name: player_name.0.clone(), position: player_position };
        let arguments = (npc_view, player_view, advance.quest.clone(), advance.stage.clone());
        scripts.call(script, ON_QUEST_STAGE, arguments);
    }
}

/// Runs `on_idle` every few seconds for NPCs standing still.
fn run_idle_hooks(
    time: Res<Time>,
    scripts: Res<Scripts>,
    mut npcs: Query<(Entity, &mut Npc, &DisplayName, &Position, &Mover)>,
) {
    for (entity, mut npc, name, &Position(position), mover) in &mut npcs {
        if mover.0.is_moving() || !npc.idle.tick(time.delta()).is_finished() {
            continue;
        }
        npc.idle = idle_timer();
        if let Some(script) = &npc.definition.script {
            let view = NpcView { entity, name: name.0.clone(), position, home: npc.home };
            scripts.call(script, ON_IDLE, (view,));
        }
    }
}

/// Checks and applies what scripts asked for.
fn apply_script_commands(
    scripts: Res<Scripts>,
    world: Res<ServerWorld>,
    mut link: ResMut<Link>,
    mut npcs: Query<(&Npc, &NetworkId, &DisplayName, &Position, &mut Mover)>,
    players: Query<(&Player, &Position)>,
) {
    for command in scripts.take_commands() {
        match command {
            ScriptCommand::Say { npc, text } => {
                let Ok((_, &NetworkId(from), name, &Position(position), _)) = npcs.get(npc) else { continue };
                let Some(text) = clean_chat_text(&text) else { continue };
                let nearby = players
                    .iter()
                    .filter(|(_, player_position)| distance(player_position.0, position) <= SPEECH_RANGE)
                    .map(|(player, _)| player.connection);
                link.broadcast(nearby, &ServerMessage::Chat { from, name: name.0.clone(), text });
            }
            ScriptCommand::Wander { npc } => {
                let Ok((npc, _, _, &Position(position), mut mover)) = npcs.get_mut(npc) else { continue };
                if let Some(spot) = wander_spot(npc, &world) {
                    mover.0 = plan_route(position, spot, |x, y| world.is_blocked(x, y));
                }
            }
            ScriptCommand::MoveTo { npc, x, y } => {
                let Ok((npc, _, _, &Position(position), mut mover)) = npcs.get_mut(npc) else { continue };
                if !x.is_finite() || !y.is_finite() || npc.definition.wander_radius <= 0.0 {
                    continue;
                }
                // Keep it within the NPC's wander radius of home.
                let goal = venture_simulation::clamp_move_goal(npc.home, (x, y), npc.definition.wander_radius);
                mover.0 = plan_route(position, goal, |x, y| world.is_blocked(x, y));
            }
        }
    }
}

/// A random walkable spot within the NPC's wander radius of home, if one turns up.
fn wander_spot(npc: &Npc, world: &ServerWorld) -> Option<Point> {
    let radius = npc.definition.wander_radius;
    if radius <= 0.0 {
        return None;
    }
    (0..WANDER_ATTEMPTS).find_map(|_| {
        let angle = fastrand::f32() * std::f32::consts::TAU;
        let reach = radius * fastrand::f32().sqrt();
        let spot = (npc.home.0 + angle.cos() * reach, npc.home.1 + angle.sin() * reach);
        (!world.is_blocked(spot.0.floor() as i32, spot.1.floor() as i32)).then_some(spot)
    })
}

fn idle_timer() -> Timer {
    let seconds = IDLE_SECONDS.start + fastrand::f32() * (IDLE_SECONDS.end - IDLE_SECONDS.start);
    Timer::from_seconds(seconds, TimerMode::Once)
}

fn distance(a: Point, b: Point) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
