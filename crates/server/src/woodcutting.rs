//! Woodcutting: felling trees and cutting them up for logs (part of the
//! Woodworking skill; see `docs/plan-trees.md`). The only code that changes
//! a player's woodcutting, or trees falling and growing back.
//!
//! - **Felling:** a player with an axe in hand asks to fell a standing tree
//!   ([`WoodcuttingCommand::Fell`]) beside them. If their woodworking level
//!   is the tree's or more (small, young trees' is lower than their
//!   species', and they give half the experience), they chop: a swing at
//!   once, and then one every
//!   [`WoodcuttingConfig::swing_seconds`]. Each swing may bite
//!   (`venture_rpg::woodcutting`), and everyone nearby sees it
//!   ([`ServerMessage::Chopped`]). After enough bites the tree falls, away
//!   from them: it's felled ([`FelledTrees`], which everyone is told about)
//!   and its trunk becomes a **fallen tree**, an entity like any other
//!   ([`EntityKind::FallenTree`]). Felling gives experience too.
//! - **Cutting up:** chopping a fallen tree ([`WoodcuttingCommand::Cut`])
//!   gives a log a bite (through [`Inventory::apply`]), with its experience
//!   (through [`Character::add_skill_experience`]), until its logs run out,
//!   when it goes.
//! - **Growing back:** a fallen tree goes after
//!   [`WoodcuttingConfig::fallen_seconds`], and the tree grows back
//!   [`WoodcuttingConfig::regrow_seconds`] after that.
//!
//! Moving, attacking or dying stops chopping. Standing trees are the baked
//! world's (see [`ServerWorld::tree`]); only what's changed is kept here.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;
use venture_content::WoodworkingPart;
use venture_content::trees::{TreeWoodcutting, common_name, fallen_name};
use venture_protocol::{ChopTarget, EntityKind, ServerMessage, TreeId, WoodcuttingCommand};
use venture_rpg::woodcutting::{
    FALLEN_LENGTHS, TreeSize, bite_chance, bites_to_fell, can_fell, logs_in, tree_experience, tree_level,
};

use crate::ServerSystems;
use crate::character::Character;
use crate::combat::{Attacking, Health};
use crate::config::WoodcuttingConfig;
use crate::content::LoadedContent;
use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::equipment::Equipment;
use crate::inventory::{Change, Inventory};
use crate::link::Link;
use crate::requests::WoodcuttingRequest;
use crate::world::{ServerWorld, WorldTree};

/// The skill woodcutting is part of.
pub const SKILL: &str = "woodworking";
/// Players this near see a swing, in metres.
const SWINGS_SEEN: f32 = 60.0;

/// Runs woodcutting.
pub struct WoodcuttingPlugin;

impl Plugin for WoodcuttingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FelledTrees>().add_systems(
            FixedUpdate,
            (welcome_newcomers, act_on_requests, stop_when_busy, chop, clear_away_and_grow_back)
                .chain()
                .in_set(ServerSystems::Woodcutting),
        );
    }
}

/// The rules for woodcutting from `server.ron`.
#[derive(Resource, Clone)]
pub struct WoodcuttingRules(pub WoodcuttingConfig);

/// The trees felled (and not grown back yet): their fallen tree while it
/// lies, and when (in seconds of game time) it goes and the tree grows back.
#[derive(Resource, Default)]
pub struct FelledTrees(HashMap<TreeId, Felled>);

struct Felled {
    fallen: Option<Entity>,
    fallen_until: f32,
    grows_back_at: f32,
}

/// A fallen tree (the trunk of a felled one), lying where it fell.
#[derive(Component)]
pub struct FallenTree {
    tree: TreeId,
    /// Which way it fell (see [`EntityKind::FallenTree`]).
    toward: f32,
    /// How big it was.
    size: TreeSize,
    /// What it is to woodworking.
    woodcutting: TreeWoodcutting,
    /// The level it takes to cut it up (small trees' is lower than their species').
    level: u32,
    /// The logs left in it.
    logs_left: u32,
}

impl FallenTree {
    /// How clients are told about it.
    pub fn kind(&self) -> EntityKind {
        EntityKind::FallenTree { tree: self.tree, toward: self.toward, level: self.level }
    }

    /// The nearest point to `point` on its trunk, lying from `base` the way it fell.
    fn nearest_point(&self, base: (f32, f32), point: (f32, f32)) -> (f32, f32) {
        let length = FALLEN_LENGTHS[self.size as usize];
        let (dx, dy) = (self.toward.sin(), self.toward.cos());
        let along = ((point.0 - base.0) * dx + (point.1 - base.1) * dy).clamp(0.0, length);
        (base.0 + dx * along, base.1 + dy * along)
    }

    /// How far a point is from its trunk, lying from `base` the way it fell.
    fn distance_from(&self, base: (f32, f32), point: (f32, f32)) -> f32 {
        let nearest = self.nearest_point(base, point);
        (point.0 - nearest.0).hypot(point.1 - nearest.1)
    }
}

/// A player chopping.
#[derive(Component)]
pub struct Chopping {
    target: ChopTarget,
    /// When the next swing is (seconds of game time).
    next_swing: f32,
    /// Bites so far (towards felling).
    bites: u32,
    /// The axe's power.
    power: f32,
}

/// Tells players who've just joined which trees are felled, and about the fallen ones.
fn welcome_newcomers(
    mut link: ResMut<Link>,
    felled: Res<FelledTrees>,
    newcomers: Query<&Player, Added<Player>>,
    fallen: Query<(&FallenTree, &NetworkId, &DisplayName, &Position)>,
) {
    for player in &newcomers {
        let trees = felled.0.keys().copied().collect();
        link.send(player.connection, &ServerMessage::TreesFelled { trees });
        for (tree, &NetworkId(id), name, &position) in &fallen {
            link.send(player.connection, &appeared(id, name, tree.kind(), position));
        }
    }
}

/// Starts (or stops) chopping.
fn act_on_requests(
    mut commands: Commands,
    mut requests: MessageReader<WoodcuttingRequest>,
    mut link: ResMut<Link>,
    world: Res<ServerWorld>,
    rules: Res<WoodcuttingRules>,
    content: Res<LoadedContent>,
    time: Res<Time>,
    felled: Res<FelledTrees>,
    players: Query<(&Player, &Position, &Equipment, &Character, Option<&Chopping>)>,
    fallen: Query<(Entity, &NetworkId, &FallenTree, &Position)>,
) {
    let pack = &content.pack;
    for request in requests.read() {
        let Ok((player, &Position(at), equipment, character, chopping)) = players.get(request.player) else {
            continue;
        };
        let connection = player.connection;
        let mut tell = |text: String| link.send(connection, &ServerMessage::Notice { text });
        let target = match request.command {
            WoodcuttingCommand::Stop => {
                if chopping.is_some() {
                    commands.entity(request.player).remove::<Chopping>();
                    link.send(connection, &ServerMessage::Chopping { at: None });
                }
                continue;
            }
            WoodcuttingCommand::Fell { tree } => ChopTarget::Standing(tree),
            WoodcuttingCommand::Cut { fallen } => ChopTarget::Fallen(fallen),
        };
        // Asking again to chop what they're chopping already carries on (clicks can repeat).
        if chopping.is_some_and(|chopping| chopping.target == target) {
            continue;
        }
        // An axe in hand, that they can use.
        let axe = equipment.in_hand().and_then(|item| pack.items.get(item)).and_then(|item| item.woodworking);
        let Some(WoodworkingPart::Axe { power, level: axe_level }) = axe else {
            tell("You need an axe in your hand to chop wood.".to_string());
            continue;
        };
        let skill = character.skill(SKILL).level;
        if skill < axe_level {
            tell(format!("You need woodworking level {axe_level} to use that axe."));
            continue;
        }
        // The tree: there, near enough, and not beyond them.
        let reach = rules.0.reach;
        let (name, level) = match target {
            ChopTarget::Standing(id) => {
                let Some(tree) = world.tree(id).filter(|_| !felled.0.contains_key(&id)) else { continue };
                let Some((species_name, entry)) = woodcutting_of(&world, pack, &tree) else {
                    tell("That tree can't be felled.".to_string());
                    continue;
                };
                let size = TreeSize::of_variant(tree.variant);
                let name = sized_name(&species_name, size);
                if (tree.at.0 - at.0).hypot(tree.at.1 - at.1) > reach {
                    tell(format!("You're too far from the {name} to chop it."));
                    continue;
                }
                (name, tree_level(entry.level, size))
            }
            ChopTarget::Fallen(id) => {
                let Some((_, _, tree, &Position(base))) = fallen.iter().find(|(_, network_id, ..)| network_id.0 == id)
                else {
                    continue;
                };
                if tree.distance_from(base, at) > reach {
                    tell("You're too far from the fallen tree to chop it.".to_string());
                    continue;
                }
                if tree.logs_left == 0 {
                    tell("There's no more wood worth taking from it.".to_string());
                    continue;
                }
                let species_name = species_display(pack, &world, tree.tree);
                (format!("fallen {}", sized_name(&species_name, tree.size)), tree.level)
            }
        };
        if !can_fell(skill, level) {
            tell(format!("You need woodworking level {level} to chop the {name}."));
            continue;
        }
        let now = time.elapsed_secs();
        commands.entity(request.player).insert(Chopping {
            target,
            // The first swing straight away, so it's clear chopping has started.
            next_swing: now,
            bites: 0,
            power,
        });
        tell(format!("You swing your axe at the {name}."));
        link.send(connection, &ServerMessage::Chopping { at: Some(target) });
    }
}

/// A tree's species name and woodworking entry, if it has one.
fn woodcutting_of(
    world: &ServerWorld,
    pack: &venture_content::ContentPack,
    tree: &WorldTree,
) -> Option<(String, TreeWoodcutting)> {
    let id = world.species_id(tree.species)?;
    let entry = pack.tree_woodcutting.iter().find(|entry| entry.species == id)?.clone();
    let name = pack.tree_species.iter().find(|species| species.id == id).map_or(id.to_string(), |s| s.name.clone());
    Some((name, entry))
}

/// How a tree reads in a sentence: "oak", or "small oak" if it's young.
fn sized_name(species_name: &str, size: TreeSize) -> String {
    let name = common_name(species_name);
    if size == TreeSize::Young { format!("small {name}") } else { name }
}

/// A species' name, by a tree's id.
fn species_display(pack: &venture_content::ContentPack, world: &ServerWorld, tree: TreeId) -> String {
    world
        .tree(tree)
        .and_then(|tree| world.species_id(tree.species))
        .map(|id| pack.tree_species.iter().find(|species| species.id == id).map_or(id.to_string(), |s| s.name.clone()))
        .unwrap_or_else(|| "tree".to_string())
}

/// Stops players chopping when they move, fight or fall.
fn stop_when_busy(
    mut commands: Commands,
    mut link: ResMut<Link>,
    players: Query<(Entity, &Player, &Mover, &Health, Has<Attacking>), With<Chopping>>,
) {
    for (entity, player, mover, health, attacking) in &players {
        if mover.0.is_moving() || attacking || health.current() == 0 {
            commands.entity(entity).remove::<Chopping>();
            link.send(player.connection, &ServerMessage::Chopping { at: None });
        }
    }
}

/// Swings: bites, trees falling, and logs.
fn chop(
    mut commands: Commands,
    mut link: ResMut<Link>,
    mut ids: ResMut<EntityIds>,
    mut felled: ResMut<FelledTrees>,
    world: Res<ServerWorld>,
    rules: Res<WoodcuttingRules>,
    content: Res<LoadedContent>,
    time: Res<Time>,
    mut choppers: Query<(
        Entity,
        &Player,
        &NetworkId,
        &DisplayName,
        &Position,
        &mut Chopping,
        &mut Character,
        &mut Inventory,
    )>,
    mut fallen: Query<(Entity, &NetworkId, &mut FallenTree, &Position)>,
    everyone: Query<(&Player, &Position)>,
) {
    let now = time.elapsed_secs();
    let pack = &content.pack;
    for (entity, player, &NetworkId(id), player_name, &Position(at), mut chopping, mut character, mut inventory) in
        &mut choppers
    {
        if now < chopping.next_swing {
            continue;
        }
        chopping.next_swing = now + rules.0.swing_seconds;
        let connection = player.connection;
        let stop = |link: &mut Link, commands: &mut Commands, text: String| {
            commands.entity(entity).remove::<Chopping>();
            link.send(connection, &ServerMessage::Chopping { at: None });
            link.send(connection, &ServerMessage::Notice { text });
        };
        let skill = character.skill(SKILL).level;
        match chopping.target {
            ChopTarget::Standing(tree_id) => {
                let tree = world.tree(tree_id).filter(|_| !felled.0.contains_key(&tree_id));
                let Some((tree, (species_name, entry))) =
                    tree.and_then(|tree| Some((tree, woodcutting_of(&world, pack, &tree)?)))
                else {
                    stop(&mut link, &mut commands, "The tree's already down.".to_string());
                    continue;
                };
                let size = TreeSize::of_variant(tree.variant);
                let level = tree_level(entry.level, size);
                let bit = fastrand::f32() < bite_chance(skill, level, chopping.power);
                tell_nearby(&mut link, &everyone, at, &ServerMessage::Chopped { id, bit, at: tree.at });
                if !bit {
                    continue;
                }
                chopping.bites += 1;
                if chopping.bites < bites_to_fell(size, entry.hardness) {
                    continue;
                }
                // It falls, away from the player.
                let (dx, dy) = (tree.at.0 - at.0, tree.at.1 - at.1);
                let toward = if dx.hypot(dy) > 0.01 { dx.atan2(dy) } else { fastrand::f32() * TAU };
                let fallen_id = ids.allocate();
                let fallen_tree = FallenTree {
                    tree: tree_id,
                    toward,
                    size,
                    woodcutting: entry.clone(),
                    level,
                    logs_left: logs_in(size),
                };
                let name = DisplayName(fallen_name(&species_name, size == TreeSize::Young));
                let everyone_connection: Vec<_> = everyone.iter().map(|(player, _)| player.connection).collect();
                link.broadcast(everyone_connection.iter().copied(), &ServerMessage::TreeFelled { tree: tree_id });
                link.broadcast(everyone_connection, &appeared(fallen_id, &name, fallen_tree.kind(), Position(tree.at)));
                let fallen_entity = commands.spawn((fallen_tree, NetworkId(fallen_id), name, Position(tree.at))).id();
                felled.0.insert(
                    tree_id,
                    Felled {
                        fallen: Some(fallen_entity),
                        fallen_until: now + rules.0.fallen_seconds,
                        grows_back_at: now + rules.0.fallen_seconds + rules.0.regrow_seconds,
                    },
                );
                info!("Woodcutting: {} felled a {} ({size:?}).", player_name.0, species_name);
                let experience = tree_experience(entry.felling_experience, size).round() as u64;
                let levels = character.add_skill_experience(SKILL, experience);
                let mut text =
                    format!("The {} falls! (+{experience} woodworking experience)", sized_name(&species_name, size));
                if levels > 0 {
                    text += &format!(" Your woodworking level is now {}.", character.skill(SKILL).level);
                }
                stop(&mut link, &mut commands, text);
            }
            ChopTarget::Fallen(fallen_id) => {
                let Some((fallen_entity, _, mut tree, &Position(base))) =
                    fallen.iter_mut().find(|(_, network_id, ..)| network_id.0 == fallen_id)
                else {
                    stop(&mut link, &mut commands, "The fallen tree has gone.".to_string());
                    continue;
                };
                let bit = fastrand::f32() < bite_chance(skill, tree.level, chopping.power);
                let aim = tree.nearest_point(base, at);
                tell_nearby(&mut link, &everyone, at, &ServerMessage::Chopped { id, bit, at: aim });
                if !bit {
                    continue;
                }
                let log = tree.woodcutting.log.clone();
                if inventory.apply(&Change::give(&log, 1), &pack.items).is_err() {
                    stop(&mut link, &mut commands, "Your bag is full.".to_string());
                    continue;
                }
                tree.logs_left -= 1;
                let experience = tree_experience(tree.woodcutting.experience, tree.size).round() as u64;
                let levels = character.add_skill_experience(SKILL, experience);
                let log_name = pack.items.get(&log).map_or(log.clone(), |item| item.name.to_lowercase());
                let mut text = format!("You cut some {log_name}. (+{experience} woodworking experience)");
                if levels > 0 {
                    text += &format!(" Your woodworking level is now {}.", character.skill(SKILL).level);
                }
                link.send(connection, &ServerMessage::Notice { text });
                if tree.logs_left == 0 {
                    // Cut up: it goes, and the tree starts growing back.
                    stop(&mut link, &mut commands, "You've cut up all the wood worth taking from it.".to_string());
                    let everyone_connection = everyone.iter().map(|(player, _)| player.connection);
                    link.broadcast(everyone_connection, &ServerMessage::EntityLeft { id: fallen_id });
                    commands.entity(fallen_entity).despawn();
                    if let Some(state) = felled.0.get_mut(&tree.tree) {
                        state.fallen = None;
                        state.grows_back_at = now + rules.0.regrow_seconds;
                    }
                }
            }
        }
    }
}

/// Tells players near `at` something.
fn tell_nearby(link: &mut Link, everyone: &Query<(&Player, &Position)>, at: (f32, f32), message: &ServerMessage) {
    let near = everyone
        .iter()
        .filter(|(_, Position(other))| (other.0 - at.0).hypot(other.1 - at.1) <= SWINGS_SEEN)
        .map(|(player, _)| player.connection);
    link.broadcast(near, message);
}

/// Fallen trees go when their time's up, and felled trees grow back after that.
fn clear_away_and_grow_back(
    mut commands: Commands,
    mut link: ResMut<Link>,
    mut felled: ResMut<FelledTrees>,
    time: Res<Time>,
    fallen: Query<&NetworkId, With<FallenTree>>,
    players: Query<&Player>,
) {
    let now = time.elapsed_secs();
    let everyone: Vec<_> = players.iter().map(|player| player.connection).collect();
    felled.0.retain(|&tree, state| {
        if let Some(entity) = state.fallen
            && now >= state.fallen_until
        {
            if let Ok(&NetworkId(id)) = fallen.get(entity) {
                link.broadcast(everyone.iter().copied(), &ServerMessage::EntityLeft { id });
            }
            commands.entity(entity).despawn();
            state.fallen = None;
        }
        let grows_back = now >= state.grows_back_at;
        if grows_back {
            link.broadcast(everyone.iter().copied(), &ServerMessage::TreeGrewBack { tree });
        }
        !grows_back
    });
}
