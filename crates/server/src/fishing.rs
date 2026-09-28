//! Fishing: casting, bites and strikes, the fight, and the catch. The only
//! code that changes a player's fishing (their [`Fishing`]).
//!
//! A player with a rod in hand casts at water within its reach, with bait
//! or a lure from their bag (bait is used up by the cast). What bites is
//! chosen then, from the fish that live in that water and biome, that the
//! player's fishing level can catch, and that the bait or lure draws (see
//! `venture_rpg::fishing`); nothing may. A few nibbles come before the
//! bite. With a ledger or float the player must strike in time (the window
//! plus an allowance for the network, from `server.ron`); a lure's fish
//! hooks itself. Then the fight (`venture_simulation::fishing`) is stepped
//! every tick from whether the player is reeling, and ends with the fish
//! landed (into the bag through [`Inventory::apply`], its experience
//! through [`Character::add_skill_experience`]) or lost. Moving, attacking
//! or dying ends fishing.
//!
//! The player is told how it's going ([`ServerMessage::Fishing`]), and why
//! it ended (a [`ServerMessage::Notice`]).
//!
//! **Traps** are the other half. A player sets one (a trap item from their
//! bag, with bait if they have some) in water within reach; it's saved with
//! them ([`PlacedTrap`], in their profile) and soaks by the real clock, so it
//! fills while they're away. While they're in the game, everyone sees it
//! ([`TrapInWater`]). When they collect it, its catches are drawn from the
//! fish traps catch in that water (a quarter of them bycatch; bait makes the
//! fish it appeals to likelier), go into their bag with their experience,
//! and the trap comes back with them.

use std::collections::BTreeMap;

use bevy::prelude::*;
use venture_content::{FishDef, FishingPart, ItemDef, fish_living_in};
use venture_navigation::Point;
use venture_protocol::{EntityId, EntityKind, FishingState, ServerMessage};
use venture_rpg::fishing::{self, Method};
use venture_simulation::fishing::{
    Fight, FightEnd, FightSetup, FightTimeline, most_common_water, region_cells, shoal_region, shoals,
};
use venture_world::{Biome, FluidBody};

use crate::character::Character;
use crate::clock::WorldClock;
use crate::combat::{Attacking, Health};
use crate::config::FishingConfig;
use crate::content::LoadedContent;
use crate::entities::{DisplayName, EntityIds, Mover, NetworkId, Player, Position, appeared};
use crate::equipment::Equipment;
use crate::inventory::{Change, Inventory, describe_items};
use crate::link::Link;
use crate::profiles::SavedTrap;
use crate::requests::{FishingAction, FishingRequest};
use crate::world::ServerWorld;
use crate::{GameRules, ServerSystems};

/// The fishing skill's id (in the content's skills).
pub const SKILL: &str = "fishing";

/// How far from a player (in metres) they can set a trap, and collect one.
const TRAP_REACH: f32 = 5.0;

/// How much further than a rod's reach a cast may land (for where the
/// player stands being a little off on the server).
const REACH_ALLOWANCE: f32 = 1.0;

/// How much further than when they cast a player may be from their line
/// before it's left behind (only by being moved: walking stops fishing).
const LEFT_THE_LINE: f32 = 5.0;

/// Runs fishing.
pub struct FishingPlugin;

impl Plugin for FishingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (act_on_requests, stop_when_busy, run_lines, show_traps, tell_others)
                .chain()
                .in_set(ServerSystems::Fishing),
        );
    }
}

/// The rules for fishing from `server.ron`.
#[derive(Resource, Clone)]
pub struct FishingRules(pub FishingConfig);

/// A player fishing.
#[derive(Component, Debug)]
pub struct Fishing {
    line: Line,
    stage: Stage,
    /// When the line was cast (seconds of game time).
    cast_at: f32,
}

/// The trap a player has set, if any (saved with them).
#[derive(Component, Default, Debug)]
pub struct PlacedTrap(pub Option<SavedTrap>);

/// A trap in the water, for everyone to see: the entity standing for a
/// player's [`PlacedTrap`] while they're in the game.
#[derive(Component)]
pub struct TrapInWater {
    /// Whose it is.
    owner: Entity,
    /// Their id, for clients.
    owner_id: EntityId,
    /// Where it is.
    at: Point,
}

impl TrapInWater {
    /// How clients are told about it.
    pub fn kind(&self) -> EntityKind {
        EntityKind::Trap { owner: self.owner_id }
    }
}

/// Seconds since 1970, by the real clock.
fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// Checks setting a trap and works out what's saved. `trap` and `bait` are
/// item ids from the request. Returns why not, in words for the player.
pub fn check_trap(
    items: &BTreeMap<String, ItemDef>,
    inventory: &Inventory,
    placed: &PlacedTrap,
    skill: u32,
    from: Point,
    at: Point,
    (trap, bait): (&str, Option<&str>),
    water: Option<(FluidBody, Biome)>,
    now: u64,
) -> Result<SavedTrap, String> {
    if placed.0.is_some() {
        return Err("You already have a trap set. Collect it first.".to_string());
    }
    let Some(&FishingPart::Trap { level, .. }) = items.get(trap).and_then(|item| item.fishing.as_ref()) else {
        return Err("That isn't a trap.".to_string());
    };
    if inventory.count(trap) == 0 {
        return Err(format!("You have no {}.", named(trap, items)));
    }
    if skill < level {
        return Err(format!("You need fishing level {level} to set the {}.", named(trap, items)));
    }
    if water.is_none() {
        return Err("Traps go in the water.".to_string());
    }
    if (at.0 - from.0).hypot(at.1 - from.1) > TRAP_REACH {
        return Err("Stand closer to the water to set a trap.".to_string());
    }
    if let Some(bait) = bait {
        let is_bait = matches!(items.get(bait).and_then(|item| item.fishing.as_ref()), Some(FishingPart::Bait(_)));
        if !is_bait || inventory.count(bait) == 0 {
            return Err(format!("You have no {} to bait it with.", named(bait, items)));
        }
    }
    Ok(SavedTrap { item: trap.to_string(), at, set_at: now, bait: bait.map(str::to_string) })
}

/// How many catches a trap has soaked long enough for.
fn trap_fill(items: &BTreeMap<String, ItemDef>, trap: &SavedTrap, now: u64) -> u32 {
    match items.get(&trap.item).and_then(|item| item.fishing.as_ref()) {
        Some(&FishingPart::Trap { capacity, soak, .. }) => {
            fishing::trap_catches(now.saturating_sub(trap.set_at) as f32, soak, capacity)
        }
        _ => 0,
    }
}

/// What a trap has caught (item ids, one each), from `rolls` (two a catch,
/// 0 to 1): how many catches from how long it's soaked, each drawn from the
/// fish traps catch in its water that the player's level can, a quarter of
/// them bycatch, and those its bait appeals to likelier.
pub fn trap_catches<'a>(
    items: &'a BTreeMap<String, ItemDef>,
    tuning: &fishing::Tuning,
    trap: &SavedTrap,
    (body, biome): (FluidBody, Biome),
    skill: u32,
    now: u64,
    rolls: &mut impl FnMut() -> f32,
) -> Vec<&'a str> {
    let caught = trap_fill(items, trap, now);
    let bait = match trap.bait.as_ref().and_then(|bait| items.get(bait)?.fishing.as_ref()) {
        Some(FishingPart::Bait(bait)) => Some(bait),
        _ => None,
    };
    let living: Vec<(&str, &FishDef)> = fish_living_in(items.values(), body, biome)
        .into_iter()
        .filter(|(_, fish)| fish.trap > 0.0 && fish.level <= skill)
        .collect();
    let pool = |bycatch: bool| -> Vec<(&str, f32)> {
        living
            .iter()
            .filter(|(_, fish)| fish.bycatch == bycatch)
            .map(|(id, fish)| (*id, tuning.trap_weight(&fish.traits, fish.trap, bait)))
            .collect()
    };
    let (main, bycatch) = (pool(false), pool(true));
    (0..caught)
        .filter_map(|_| {
            let wants_bycatch = rolls() < tuning.bycatch_share;
            let from = match (wants_bycatch, main.is_empty(), bycatch.is_empty()) {
                (true, _, false) | (false, true, false) => &bycatch,
                (_, false, _) => &main,
                _ => return None,
            };
            let weights: Vec<f32> = from.iter().map(|(_, weight)| *weight).collect();
            fishing::choose(&weights, rolls()).map(|index| from[index].0)
        })
        .collect()
}

/// Keeps a [`TrapInWater`] for every trap set by a player in the game, and
/// tells everyone when they come and go.
fn show_traps(
    mut commands: Commands,
    mut link: ResMut<Link>,
    mut ids: ResMut<EntityIds>,
    players: Query<(Entity, &Player, &NetworkId, &DisplayName, &PlacedTrap)>,
    shown: Query<(Entity, &NetworkId, &TrapInWater)>,
) {
    let everyone = || players.iter().map(|(_, player, ..)| player.connection).collect::<Vec<_>>();
    for (entity, &NetworkId(id), trap) in &shown {
        let still_there = players
            .get(trap.owner)
            .is_ok_and(|(.., placed)| placed.0.as_ref().is_some_and(|saved| saved.at == trap.at));
        if !still_there {
            commands.entity(entity).despawn();
            link.broadcast(everyone(), &ServerMessage::EntityLeft { id });
        }
    }
    for (owner, _, &NetworkId(owner_id), name, placed) in &players {
        let Some(saved) = &placed.0 else { continue };
        if shown.iter().any(|(_, _, trap)| trap.owner == owner && trap.at == saved.at) {
            continue;
        }
        let id = ids.allocate();
        let name = DisplayName(format!("{}'s trap", name.0));
        let trap = TrapInWater { owner, owner_id, at: saved.at };
        link.broadcast(everyone(), &appeared(id, &name, trap.kind(), Position(saved.at)));
        commands.spawn((trap, NetworkId(id), name, Position(saved.at)));
    }
}

/// A cast line: what the player fishes with, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// How they're fishing.
    pub method: Method,
    /// The bait or lure (an item id).
    pub tackle: String,
    /// Where it is.
    pub at: Point,
    /// How far out, in metres.
    pub distance: f32,
    /// The reel's line strength (kilograms) and capacity (metres).
    pub reel: (f32, f32),
}

/// Where a line is in its life.
#[derive(Debug)]
enum Stage {
    /// Waiting for `fish` (or nothing) to bite at `bite_at`, with nibbles at
    /// these times first (soonest last).
    Waiting { fish: Option<String>, bite_at: f32, nibbles: Vec<f32> },
    /// A fish bit at `since`: strike now.
    Biting { fish: String, since: f32 },
    /// A fish is hooked.
    Fighting { fish: String, fight: Box<FightTimeline> },
}

/// Checks a cast and works out its line. `hand` is the item in the
/// player's hand, `skill` their fishing level, `from` where they stand.
/// Returns why not, in words for the player.
pub fn check_cast(
    items: &BTreeMap<String, ItemDef>,
    hand: Option<&str>,
    inventory: &Inventory,
    skill: u32,
    from: Point,
    at: Point,
    tackle: &str,
    water: Option<(FluidBody, Biome)>,
) -> Result<Line, String> {
    let name = |id: &str| named(id, items);
    let rod = hand.and_then(|id| Some((id, items.get(id)?.fishing.as_ref()?)));
    let Some((rod, &FishingPart::Rod { method, reach, level })) = rod else {
        return Err("You need a fishing rod in your hand to fish.".to_string());
    };
    if skill < level {
        return Err(format!("You need fishing level {level} to use the {}.", name(rod)));
    }
    let distance = (at.0 - from.0).hypot(at.1 - from.1);
    if distance > reach + REACH_ALLOWANCE {
        return Err(format!("That's too far: the {} casts {reach:.0} metres.", name(rod)));
    }
    if water.is_none() {
        return Err("You can only cast into water.".to_string());
    }
    if inventory.count(tackle) == 0 {
        return Err(format!("You have no {} left.", name(tackle)));
    }
    match (method, items.get(tackle).and_then(|item| item.fishing.as_ref())) {
        (Method::Lure, Some(FishingPart::Lure(_))) | (Method::Ledger | Method::Float, Some(FishingPart::Bait(_))) => {}
        (Method::Lure, _) => return Err(capitalised(&format!("the {} needs a lure.", name(rod)))),
        _ => return Err(capitalised(&format!("the {} needs bait.", name(rod)))),
    }
    // The strongest reel in the bag they can use.
    let reel = inventory
        .slots()
        .iter()
        .flatten()
        .filter_map(|stack| match items.get(&stack.item)?.fishing.as_ref()? {
            &FishingPart::Reel { strength, capacity, level } if level <= skill => Some((strength, capacity)),
            _ => None,
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let Some(reel) = reel else {
        return Err("You need a reel in your bag to fish.".to_string());
    };
    Ok(Line { method, tackle: tackle.to_string(), at, distance, reel })
}

/// The fish that might take a line in this water, with how strongly each
/// is drawn: those that live there, rods catch, the player's level can
/// catch, and the bait or lure draws with a hook that suits them.
pub fn candidates<'a>(
    items: &'a BTreeMap<String, ItemDef>,
    line: &Line,
    (body, biome): (FluidBody, Biome),
    skill: u32,
) -> Vec<(&'a str, &'a FishDef, f32)> {
    let offer = items.get(&line.tackle).and_then(|item| item.fishing.as_ref());
    items
        .values()
        .filter_map(|item| match &item.fishing {
            Some(FishingPart::Fish(fish))
                if fish.rods && fish.level <= skill && fish.waters.contains(&body) && fish.biomes.contains(&biome) =>
            {
                let traits = &fish.traits;
                let weight = match offer? {
                    FishingPart::Bait(bait) => {
                        fishing::bait_attraction(traits, bait, line.method) * fishing::hook_fit(bait.hook, traits.size)
                    }
                    FishingPart::Lure(lure) => {
                        fishing::lure_attraction(traits, lure) * fishing::hook_fit(lure.hook, traits.size)
                    }
                    _ => 0.0,
                };
                (weight > 0.0).then_some((item.id.as_str(), fish, weight))
            }
            _ => None,
        })
        .collect()
}

/// The kind of water most of a shoaling region is (as the client works it
/// out, to show the shadows there).
pub fn region_water(world: &ServerWorld, region: (i32, i32)) -> Option<(FluidBody, Biome)> {
    let kinds: Vec<_> =
        region_cells(region).filter_map(|(x, y)| world.water_at((x as f32 + 0.5, y as f32 + 0.5))).collect();
    most_common_water(&kinds)
}

/// The fish shoaling where a line goes in (in the same shoaling region, of
/// the water most of the region is), by id, at an hour of the day (see
/// `venture_simulation::fishing::shoals`).
pub fn shoaling_at(
    items: &BTreeMap<String, ItemDef>,
    (body, biome): (FluidBody, Biome),
    at: Point,
    world_seed: u32,
    hour: u32,
) -> Vec<&str> {
    let living = fish_living_in(items.values(), body, biome);
    let weights = vec![1.0; living.len()];
    shoals(u64::from(world_seed), shoal_region(at), hour, &weights).iter().map(|shoal| living[shoal.fish].0).collect()
}

/// What bites a line (if anything), when, and the nibbles before, from
/// three rolls (0 to 1) and the time it's cast.
fn plan_bite(
    tuning: &fishing::Tuning,
    candidates: &[(&str, &FishDef, f32)],
    method: Method,
    now: f32,
    patience: f32,
    rolls: [f32; 3],
) -> Stage {
    let weights: Vec<f32> = candidates.iter().map(|(_, _, weight)| *weight).collect();
    let Some(chosen) = fishing::choose(&weights, rolls[0]) else {
        return Stage::Waiting { fish: None, bite_at: now + patience, nibbles: Vec::new() };
    };
    let (id, fish, weight) = candidates[chosen];
    // A bait or lure that draws the fish poorly waits longer (up to twice as long).
    let wait = tuning.bite_wait(fish.traits.temperament, rolls[1]) / weight.clamp(0.5, 1.0);
    let bite_at = now + wait;
    let count = fishing::nibbles(fish.traits.temperament, method, rolls[2]);
    // Nibbles spread over the second half of the wait, soonest last.
    let nibbles = (1..=count).map(|n| bite_at - wait * 0.5 * n as f32 / (count + 1) as f32).collect();
    Stage::Waiting { fish: Some(id.to_string()), bite_at, nibbles }
}

/// A fight with a fish, hooked on a line.
fn hook(rules: &FishingConfig, line: &Line, fish: &FishDef) -> Box<FightTimeline> {
    let size = rules.tuning.size(fish.traits.size);
    Box::new(FightTimeline::new(FightSetup {
        pull: size.pull,
        run: size.run,
        stamina: size.stamina,
        strength: line.reel.0,
        capacity: line.reel.1,
        distance: line.distance.max(venture_simulation::fishing::LAND_DISTANCE + 1.0),
        seed: fastrand::u64(..),
        tuning: rules.fight,
    }))
}

fn act_on_requests(
    mut commands: Commands,
    mut requests: MessageReader<FishingRequest>,
    mut link: ResMut<Link>,
    content: Res<LoadedContent>,
    world: Res<ServerWorld>,
    rules: Res<FishingRules>,
    time: Res<Time>,
    clock: Res<WorldClock>,
    game_rules: Res<GameRules>,
    mut players: Query<(
        &Player,
        &DisplayName,
        &Position,
        &Equipment,
        &mut Character,
        &mut Inventory,
        &mut PlacedTrap,
        Option<&mut Fishing>,
    )>,
) {
    let now = time.elapsed_secs();
    let items = &content.pack.items;
    for request in requests.read() {
        let Ok((
            player,
            DisplayName(player_name),
            &Position(from),
            equipment,
            mut character,
            mut inventory,
            mut placed,
            fishing,
        )) = players.get_mut(request.player)
        else {
            continue;
        };
        let connection = player.connection;
        let tell = |link: &mut Link, text: String| link.send(connection, &ServerMessage::Notice { text });
        match &request.action {
            FishingAction::Cast { at, tackle } => {
                let skill = character.skill(SKILL).level;
                let water = world.water_at(*at);
                let line = match check_cast(items, equipment.in_hand(), &inventory, skill, from, *at, tackle, water) {
                    Ok(line) => line,
                    Err(text) => {
                        tell(&mut link, text);
                        continue;
                    }
                };
                // Bait is used up by the cast (a lure isn't).
                if line.method != Method::Lure && inventory.apply(&Change::take(tackle, 1), items).is_err() {
                    continue;
                }
                let Some(water) = water else { continue };
                let hour = clock.hour(&game_rules) as u32 % 24;
                let region = region_water(&world, shoal_region(line.at)).unwrap_or(water);
                let shoaling = shoaling_at(items, region, line.at, world.manifest.seed, hour);
                let mut candidates = candidates(items, &line, water, skill);
                for (id, _, weight) in &mut candidates {
                    if shoaling.contains(id) {
                        *weight *= rules.0.shoal_bonus;
                    }
                }
                let rolls = [fastrand::f32(), fastrand::f32(), fastrand::f32()];
                let stage = plan_bite(&rules.0.tuning, &candidates, line.method, now, rules.0.patience, rolls);
                link.send(
                    connection,
                    &ServerMessage::Fishing(FishingState::Waiting { at: line.at, method: line.method }),
                );
                commands.entity(request.player).insert(Fishing { line, stage, cast_at: now });
            }
            FishingAction::Strike => {
                let Some(mut fishing) = fishing else { continue };
                let method = fishing.line.method;
                match &fishing.stage {
                    Stage::Biting { fish, since } => {
                        if rules.0.tuning.strike_hooks(method, now - since, rules.0.strike_allowance) {
                            let Some(Some(FishingPart::Fish(def))) = items.get(fish).map(|item| &item.fishing) else {
                                continue;
                            };
                            let fight = hook(&rules.0, &fishing.line, def);
                            fishing.stage = Stage::Fighting { fish: fish.clone(), fight };
                        }
                        // (Too late is noticed by `run_lines`.)
                    }
                    Stage::Waiting { .. } => {
                        info!("Fishing: {} ({method:?}): struck too early.", player_name);
                        let text = if method == Method::Lure {
                            "You strike at nothing.".to_string()
                        } else {
                            format!("You strike too soon, and lose your {}.", named(&fishing.line.tackle, items))
                        };
                        end(&mut commands, &mut link, request.player, connection, text);
                    }
                    Stage::Fighting { .. } => {}
                }
            }
            FishingAction::Reel { reeling, tick } => {
                if let Some(mut fishing) = fishing
                    && let Stage::Fighting { fight, .. } = &mut fishing.stage
                {
                    fight.reel(*reeling, *tick);
                }
            }
            FishingAction::SetTrap { at, trap, bait } => {
                let skill = character.skill(SKILL).level;
                let water = world.water_at(*at);
                let saved = check_trap(
                    items,
                    &inventory,
                    &placed,
                    skill,
                    from,
                    *at,
                    (trap, bait.as_deref()),
                    water,
                    unix_now(),
                );
                let saved = match saved {
                    Ok(saved) => saved,
                    Err(text) => {
                        tell(&mut link, text);
                        continue;
                    }
                };
                let mut change = Change::take(trap, 1);
                if let Some(bait) = bait {
                    change = change.and_take(bait, 1);
                }
                if inventory.apply(&change, items).is_err() {
                    continue;
                }
                let minutes = match items.get(trap).and_then(|item| item.fishing.as_ref()) {
                    Some(&FishingPart::Trap { soak, .. }) => (soak / 60.0).round(),
                    _ => 0.0,
                };
                let baited =
                    bait.as_ref().map_or(String::new(), |bait| format!(", baited with {}", named(bait, items)));
                tell(
                    &mut link,
                    format!("You set your {}{baited}. It fills over {minutes} minutes.", named(trap, items)),
                );
                placed.0 = Some(saved);
            }
            FishingAction::CollectTrap => {
                let Some(saved) = placed.0.clone() else {
                    tell(&mut link, "You haven't set a trap.".to_string());
                    continue;
                };
                if (saved.at.0 - from.0).hypot(saved.at.1 - from.1) > TRAP_REACH {
                    tell(&mut link, "Your trap is too far away to collect.".to_string());
                    continue;
                }
                let skill = character.skill(SKILL).level;
                let water = world.water_at(saved.at).unwrap_or((FluidBody::River, Biome::Grassland));
                let caught =
                    trap_catches(items, &rules.0.tuning, &saved, water, skill, unix_now(), &mut || fastrand::f32());
                if caught.is_empty() && trap_fill(items, &saved, unix_now()) == 0 {
                    tell(&mut link, "Nothing in it yet. Give it longer.".to_string());
                    continue;
                }
                if caught.is_empty() {
                    // It's soaked, but nothing here goes into traps for them yet.
                    if inventory.apply(&Change::give(&saved.item, 1), items).is_ok() {
                        let text = "It's empty: nothing here will go into your trap yet. Try the coast, or \
                                    come back with more fishing experience.";
                        tell(&mut link, text.to_string());
                        placed.0 = None;
                    }
                    continue;
                }
                let mut change = Change::give(&saved.item, 1);
                for fish in &caught {
                    change = change.and_give(fish, 1);
                }
                if inventory.apply(&change, items).is_err() {
                    tell(&mut link, "Your bag is too full to empty the trap.".to_string());
                    continue;
                }
                let experience: u64 = caught
                    .iter()
                    .filter_map(|fish| match items.get(*fish)?.fishing.as_ref()? {
                        FishingPart::Fish(def) => Some(u64::from(def.experience)),
                        _ => None,
                    })
                    .sum();
                let levels = character.add_skill_experience(SKILL, experience);
                info!("Fishing: {player_name} emptied a trap: {caught:?} (+{experience} experience).");
                let names: Vec<String> = caught.iter().map(|fish| a(&named(fish, items))).collect();
                let mut text = format!("You empty your trap: {}. (+{experience} fishing experience)", names.join(", "));
                if levels > 0 {
                    text += &format!(" Your fishing level is now {}.", character.skill(SKILL).level);
                }
                tell(&mut link, text);
                placed.0 = None;
            }
            FishingAction::Stop => {
                if fishing.is_some() {
                    end(&mut commands, &mut link, request.player, connection, "You reel in.".to_string());
                }
            }
        }
    }
}

/// Ends fishing for players who've moved (or been moved), started a fight
/// or died.
fn stop_when_busy(
    mut commands: Commands,
    mut link: ResMut<Link>,
    players: Query<(Entity, &Player, &Fishing, &Position, &Mover, &Health, Has<Attacking>)>,
) {
    for (entity, player, fishing, &Position(at), mover, health, attacking) in &players {
        // (Moved away from the line without walking: by `/town`, or an admin.)
        let (line_x, line_y) = fishing.line.at;
        let left = (line_x - at.0).hypot(line_y - at.1) > fishing.line.distance + LEFT_THE_LINE;
        if mover.0.is_moving() || attacking || health.current() == 0 || left {
            end(&mut commands, &mut link, entity, player.connection, "You stop fishing.".to_string());
        }
    }
}

/// Runs each line: nibbles, bites, strikes too late, and fights.
fn run_lines(
    mut commands: Commands,
    mut link: ResMut<Link>,
    content: Res<LoadedContent>,
    rules: Res<FishingRules>,
    time: Res<Time>,
    mut players: Query<(Entity, &Player, &DisplayName, &mut Fishing, &mut Inventory, &mut Character)>,
) {
    let now = time.elapsed_secs();
    let items = &content.pack.items;
    for (entity, player, name, mut fishing, mut inventory, mut character) in &mut players {
        let connection = player.connection;
        let send = |link: &mut Link, state: FishingState| link.send(connection, &ServerMessage::Fishing(state));
        let method = fishing.line.method;
        let tackle_name = named(&fishing.line.tackle, items);
        let fish_def = |fish: &str| match items.get(fish).map(|item| &item.fishing) {
            Some(Some(FishingPart::Fish(def))) => Some(def),
            _ => None,
        };
        let line = fishing.line.clone();
        let since_cast = now - fishing.cast_at;
        // For playtests: one line for each outcome (see docs/plan-fishing.md).
        let log = |outcome: &str, fish: &str, fight: Option<&Fight>| {
            let fight = fight.map_or(String::new(), |fight| {
                format!(", fought {:.1} s", fight.ticks as f32 / venture_simulation::TICK_RATE)
            });
            info!(
                "Fishing: {} ({method:?}, {}, level {}): {outcome} {fish} {since_cast:.1} s after casting{fight}.",
                name.0,
                line.tackle,
                character.skill(SKILL).level
            );
        };
        match &mut fishing.stage {
            Stage::Waiting { fish, bite_at, nibbles } => {
                if nibbles.last().is_some_and(|&nibble| now >= nibble) {
                    nibbles.pop();
                    send(&mut link, FishingState::Nibble);
                }
                if now < *bite_at {
                    continue;
                }
                let Some(fish) = fish.take() else {
                    log("nothing bit", "", None);
                    let text = format!("Nothing here takes the {tackle_name}.");
                    end(&mut commands, &mut link, entity, connection, text);
                    continue;
                };
                send(&mut link, FishingState::Bite);
                if method == Method::Lure {
                    // Fish strike lures hard enough to hook themselves.
                    let Some(def) = fish_def(&fish) else { continue };
                    fishing.stage = Stage::Fighting { fight: hook(&rules.0, &line, def), fish };
                } else {
                    fishing.stage = Stage::Biting { fish, since: now };
                }
            }
            Stage::Biting { since, .. } => {
                let window = rules.0.tuning.strike_window(method).unwrap_or(0.0) + rules.0.strike_allowance;
                if now - *since > window {
                    log("struck too late for", &fishing_fish(&fishing.stage), None);
                    let text = format!("Too late: the fish got away with your {tackle_name}.");
                    end(&mut commands, &mut link, entity, connection, text);
                }
            }
            Stage::Fighting { fish, fight } => {
                let outcome = fight.step();
                send(&mut link, FishingState::Fight(*fight.now()));
                let Some(outcome) = outcome else { continue };
                let what = match outcome {
                    FightEnd::Landed => "landed",
                    FightEnd::Snapped => "snapped the line on",
                    FightEnd::SpooledOut => "was spooled out by",
                };
                log(what, fish, Some(fight.now()));
                let fish_name = format!("the {}", named(fish, items));
                let text = match outcome {
                    FightEnd::Landed => {
                        let Some(def) = fish_def(fish) else { continue };
                        if inventory.apply(&Change::give(fish, 1), items).is_err() {
                            format!("You land {}, but your bag is full, so you let it go.", a(&named(fish, items)))
                        } else {
                            send(&mut link, FishingState::Caught { item: fish.clone() });
                            let levels = character.add_skill_experience(SKILL, u64::from(def.experience));
                            let level = character.skill(SKILL).level;
                            let mut text = format!(
                                "You catch {}! (+{} fishing experience)",
                                a(&named(fish, items)),
                                def.experience
                            );
                            if levels > 0 {
                                text += &format!(" Your fishing level is now {level}.");
                            }
                            text
                        }
                    }
                    FightEnd::Snapped => format!("The line snaps, and {fish_name} is gone."),
                    FightEnd::SpooledOut => format!("{fish_name} runs out all your line, and is gone."),
                };
                end(&mut commands, &mut link, entity, connection, capitalised(&text));
            }
        }
    }
}

/// Tells everyone else when a player casts a line, or stops fishing, so they
/// see the line.
fn tell_others(
    mut link: ResMut<Link>,
    players: Query<(&Player, &NetworkId)>,
    lines: Query<(Entity, &NetworkId, &Fishing), Changed<Fishing>>,
    mut stopped: RemovedComponents<Fishing>,
    mut told: Local<std::collections::HashMap<Entity, (EntityId, Point)>>,
) {
    let others = |id: EntityId| {
        players.iter().filter(|(_, other)| other.0 != id).map(|(player, _)| player.connection).collect::<Vec<_>>()
    };
    for entity in stopped.read() {
        if let Some((id, _)) = told.remove(&entity) {
            link.broadcast(others(id), &ServerMessage::Angler { id, at: None });
        }
    }
    for (entity, &NetworkId(id), fishing) in &lines {
        if told.get(&entity).is_some_and(|&(_, at)| at == fishing.line.at) {
            continue;
        }
        told.insert(entity, (id, fishing.line.at));
        link.broadcast(others(id), &ServerMessage::Angler { id, at: Some(fishing.line.at) });
    }
}

/// The fish on a line (biting or hooked), if any.
fn fishing_fish(stage: &Stage) -> String {
    match stage {
        Stage::Biting { fish, .. } | Stage::Fighting { fish, .. } => fish.clone(),
        Stage::Waiting { fish, .. } => fish.clone().unwrap_or_default(),
    }
}

/// Ends a player's fishing, telling them why.
fn end(commands: &mut Commands, link: &mut Link, player: Entity, connection: crate::link::ConnectionId, text: String) {
    commands.entity(player).remove::<Fishing>();
    link.send(connection, &ServerMessage::Fishing(FishingState::Ended));
    link.send(connection, &ServerMessage::Notice { text });
}

/// An item's name as it's written in a sentence ("Red swamp crayfish" →
/// "red swamp crayfish").
fn named(item: &str, items: &BTreeMap<String, ItemDef>) -> String {
    let name = describe_items(item, 1, items);
    let mut letters = name.chars();
    letters.next().map(|first| first.to_lowercase().chain(letters).collect()).unwrap_or_default()
}

/// A name with "a" or "an" before it.
fn a(name: &str) -> String {
    let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{article} {name}")
}

/// Text with its first letter a capital ("a dace runs..." → "A dace runs...").
fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use venture_content::ContentPack;

    use super::*;

    fn items() -> BTreeMap<String, ItemDef> {
        let pack = ContentPack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")).unwrap();
        pack.items
    }

    fn bag(items: &BTreeMap<String, ItemDef>, contents: &[(&str, u32)]) -> Inventory {
        let mut inventory = Inventory::empty();
        for (item, count) in contents {
            inventory.apply(&Change::give(item, *count), items).unwrap();
        }
        inventory
    }

    const RIVER: Option<(FluidBody, Biome)> = Some((FluidBody::River, Biome::Grassland));

    #[test]
    fn a_cast_needs_a_rod_in_reach_of_water_bait_and_a_reel() {
        let items = items();
        let kit = bag(&items, &[("worm", 5), ("wooden_reel", 1), ("spinner", 1)]);
        let cast = |hand, skill, at: Point, tackle, water| {
            check_cast(&items, hand, &kit, skill, (0.0, 0.0), at, tackle, water)
        };
        let line = cast(Some("ledger_rod"), 1, (6.0, 0.0), "worm", RIVER).unwrap();
        assert_eq!((line.method, line.distance, line.reel), (Method::Ledger, 6.0, (3.0, 30.0)));

        assert!(cast(None, 1, (6.0, 0.0), "worm", RIVER).unwrap_err().contains("rod in your hand"));
        assert!(cast(Some("float_rod"), 1, (6.0, 0.0), "worm", RIVER).unwrap_err().contains("level 5"));
        assert!(cast(Some("ledger_rod"), 1, (20.0, 0.0), "worm", RIVER).unwrap_err().contains("too far"));
        assert!(cast(Some("ledger_rod"), 1, (6.0, 0.0), "worm", None).unwrap_err().contains("into water"));
        assert!(cast(Some("ledger_rod"), 1, (6.0, 0.0), "sweetcorn", RIVER).unwrap_err().contains("no sweetcorn"));
        assert!(cast(Some("ledger_rod"), 1, (6.0, 0.0), "spinner", RIVER).unwrap_err().contains("needs bait"));
        let no_reel = bag(&items, &[("worm", 5)]);
        let refused = check_cast(&items, Some("ledger_rod"), &no_reel, 1, (0.0, 0.0), (6.0, 0.0), "worm", RIVER);
        assert!(refused.unwrap_err().contains("reel"));
    }

    #[test]
    fn what_bites_depends_on_the_water_the_bait_and_the_level() {
        let items = items();
        let kit = bag(&items, &[("worm", 5), ("wooden_reel", 1), ("paste_ball", 5)]);
        let line = |tackle| check_cast(&items, Some("ledger_rod"), &kit, 99, (0.0, 0.0), (6.0, 0.0), tackle, RIVER);
        let ids = |line: &Line, water, skill| -> Vec<String> {
            candidates(&items, line, water, skill).iter().map(|(id, ..)| id.to_string()).collect()
        };
        let worm = line("worm").unwrap();
        assert_eq!(ids(&worm, RIVER.unwrap(), 1), ["dace"], "at level 1, only dace take worms in the river");
        assert_eq!(ids(&worm, RIVER.unwrap(), 10), ["chub", "dace", "gudgeon", "roach"]);
        assert!(ids(&worm, (FluidBody::Sea, Biome::Grassland), 20).is_empty(), "nothing in the sea eats worms");
        let paste = line("paste_ball").unwrap();
        assert_eq!(ids(&paste, RIVER.unwrap(), 20), ["bream"], "paste is on a medium hook, too big for gudgeon");
        assert!(ids(&worm, RIVER.unwrap(), 3).contains(&"gudgeon".to_string()), "worms take them");
        assert!(!ids(&paste, RIVER.unwrap(), 20).contains(&"signal_crayfish".to_string()), "crayfish are for traps");
    }

    #[test]
    fn fish_shoal_in_the_water_they_live_in() {
        let items = items();
        let river = RIVER.unwrap();
        let everywhere: Vec<&str> =
            (0..100).flat_map(|x| shoaling_at(&items, river, (x as f32 * 12.0, 0.0), 1, 9)).collect();
        assert!(everywhere.len() > 30, "most river regions have fish shoaling");
        let living = fish_living_in(items.values(), river.0, river.1);
        assert!(everywhere.iter().all(|id| living.iter().any(|(fish, _)| fish == id)));
        assert!(everywhere.contains(&"dace") && everywhere.contains(&"signal_crayfish"), "rod and trap fish alike");
        let sea: Vec<&str> = (0..100)
            .flat_map(|x| shoaling_at(&items, (FluidBody::Sea, Biome::Grassland), (x as f32 * 12.0, 0.0), 1, 9))
            .collect();
        assert!(!sea.contains(&"dace"));
    }

    #[test]
    fn traps_are_set_in_water_within_reach() {
        let items = items();
        let kit = bag(&items, &[("cage_trap", 1), ("shrimp_bits", 2)]);
        let none = PlacedTrap(None);
        let set = |placed: &PlacedTrap, at: Point, water, bait| {
            check_trap(&items, &kit, placed, 1, (0.0, 0.0), at, ("cage_trap", bait), water, 1000)
        };
        let saved = set(&none, (3.0, 0.0), RIVER, Some("shrimp_bits")).unwrap();
        assert_eq!(
            saved,
            SavedTrap { item: "cage_trap".into(), at: (3.0, 0.0), set_at: 1000, bait: Some("shrimp_bits".into()) }
        );
        assert!(set(&none, (3.0, 0.0), RIVER, None).unwrap().bait.is_none(), "bait is optional");
        assert!(set(&PlacedTrap(Some(saved)), (3.0, 0.0), RIVER, None).unwrap_err().contains("already"));
        assert!(set(&none, (9.0, 0.0), RIVER, None).unwrap_err().contains("closer"));
        assert!(set(&none, (3.0, 0.0), None, None).unwrap_err().contains("in the water"));
        assert!(set(&none, (3.0, 0.0), RIVER, Some("worm")).unwrap_err().contains("no worm"));
        let empty = Inventory::empty();
        let refused = check_trap(&items, &empty, &none, 1, (0.0, 0.0), (3.0, 0.0), ("cage_trap", None), RIVER, 0);
        assert!(refused.unwrap_err().contains("no cage trap"));
    }

    #[test]
    fn traps_fill_with_what_lives_there() {
        let items = items();
        let trap = |bait: Option<&str>| SavedTrap {
            item: "cage_trap".into(),
            at: (0.0, 0.0),
            set_at: 0,
            bait: bait.map(Into::into),
        };
        let mut rng = fastrand::Rng::with_seed(3);
        let mut rolls = || rng.f32();
        let river = RIVER.unwrap();
        assert!(
            trap_catches(&items, &fishing::Tuning::DEFAULT, &trap(None), river, 99, 60, &mut rolls).is_empty(),
            "nothing in the first minute"
        );
        assert_eq!(
            trap_catches(&items, &fishing::Tuning::DEFAULT, &trap(None), river, 99, 800, &mut rolls).len(),
            2,
            "two thirds full"
        );
        let full = trap_catches(&items, &fishing::Tuning::DEFAULT, &trap(None), river, 99, 100_000, &mut rolls);
        assert_eq!(full.len(), 3, "and no more than it holds");
        let living = fish_living_in(items.values(), river.0, river.1);
        let trap_fish = |id: &str| living.iter().any(|(fish, def)| *fish == id && def.trap > 0.0);
        let mut seen = Vec::new();
        for _ in 0..200 {
            for fish in trap_catches(
                &items,
                &fishing::Tuning::DEFAULT,
                &trap(Some("shrimp_bits")),
                river,
                99,
                100_000,
                &mut rolls,
            ) {
                assert!(trap_fish(fish), "{fish} isn't caught in river traps");
                seen.push(fish);
            }
        }
        let bycatch = seen.iter().filter(|fish| ["river_snail", "freshwater_mussel"].contains(fish)).count();
        assert!((90..210).contains(&bycatch), "about a quarter bycatch: {bycatch} of {}", seen.len());
        // At level 1, only what a beginner can catch.
        let beginner = trap_catches(&items, &fishing::Tuning::DEFAULT, &trap(None), river, 1, 100_000, &mut rolls);
        assert!(beginner.is_empty(), "nothing in the river's traps is level 1: {beginner:?}");
        let coast = trap_catches(
            &items,
            &fishing::Tuning::DEFAULT,
            &trap(None),
            (FluidBody::Sea, Biome::Grassland),
            1,
            100_000,
            &mut rolls,
        );
        assert!(coast.iter().all(|fish| ["brown_shrimp", "shore_crab"].contains(fish)), "{coast:?}");
    }

    #[test]
    fn bites_come_after_nibbles_or_not_at_all() {
        let items = items();
        let Some(FishingPart::Fish(roach)) = &items["roach"].fishing else { panic!("roach is a fish") };
        let candidates = [("roach", roach, 1.0)];
        let Stage::Waiting { fish, bite_at, nibbles } =
            plan_bite(&fishing::Tuning::DEFAULT, &candidates, Method::Float, 100.0, 20.0, [0.5; 3])
        else {
            panic!("a new line waits")
        };
        assert_eq!(fish.as_deref(), Some("roach"));
        assert!((110.0..=125.0).contains(&bite_at), "a skittish fish takes 10 to 25 seconds: {bite_at}");
        assert_eq!(nibbles.len(), 2, "a skittish fish nibbles more");
        assert!(nibbles.windows(2).all(|pair| pair[0] > pair[1]), "soonest last");
        assert!(nibbles.iter().all(|&nibble| (100.0..bite_at).contains(&nibble)));

        let Stage::Waiting { fish, bite_at, .. } =
            plan_bite(&fishing::Tuning::DEFAULT, &[], Method::Float, 100.0, 20.0, [0.5; 3])
        else {
            panic!("a new line waits")
        };
        assert_eq!((fish, bite_at), (None, 120.0), "nothing bites, and the player is told after a while");
    }
}
