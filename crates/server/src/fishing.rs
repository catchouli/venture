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

use std::collections::BTreeMap;

use bevy::prelude::*;
use venture_content::{FishDef, FishingPart, ItemDef};
use venture_navigation::Point;
use venture_protocol::{FishingState, ServerMessage};
use venture_rpg::fishing::{self, Method};
use venture_simulation::fishing::{Fight, FightEnd, FightSetup};
use venture_world::{Biome, FluidBody};

use crate::ServerSystems;
use crate::character::Character;
use crate::combat::{Attacking, Health};
use crate::config::FishingConfig;
use crate::content::LoadedContent;
use crate::entities::{Mover, Player, Position};
use crate::equipment::Equipment;
use crate::inventory::{Change, Inventory, describe_items};
use crate::link::Link;
use crate::requests::{FishingAction, FishingRequest};
use crate::world::ServerWorld;

/// The fishing skill's id (in the content's skills).
pub const SKILL: &str = "fishing";

/// How much further than a rod's reach a cast may land (for where the
/// player stands being a little off on the server).
const REACH_ALLOWANCE: f32 = 1.0;

/// Runs fishing.
pub struct FishingPlugin;

impl Plugin for FishingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (act_on_requests, stop_when_busy, run_lines).chain().in_set(ServerSystems::Fishing),
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
    Fighting { fish: String, fight: Box<Fight>, reeling: bool },
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

/// What bites a line (if anything), when, and the nibbles before, from
/// three rolls (0 to 1) and the time it's cast.
fn plan_bite(candidates: &[(&str, &FishDef, f32)], method: Method, now: f32, patience: f32, rolls: [f32; 3]) -> Stage {
    let weights: Vec<f32> = candidates.iter().map(|(_, _, weight)| *weight).collect();
    let Some(chosen) = fishing::choose(&weights, rolls[0]) else {
        return Stage::Waiting { fish: None, bite_at: now + patience, nibbles: Vec::new() };
    };
    let (id, fish, weight) = candidates[chosen];
    // A bait or lure that draws the fish poorly waits longer (up to twice as long).
    let wait = fishing::bite_wait(fish.traits.temperament, rolls[1]) / weight.clamp(0.5, 1.0);
    let bite_at = now + wait;
    let count = fishing::nibbles(fish.traits.temperament, method, rolls[2]);
    // Nibbles spread over the second half of the wait, soonest last.
    let nibbles = (1..=count).map(|n| bite_at - wait * 0.5 * n as f32 / (count + 1) as f32).collect();
    Stage::Waiting { fish: Some(id.to_string()), bite_at, nibbles }
}

/// A fight with a fish, hooked on a line.
fn hook(line: &Line, fish: &FishDef) -> Fight {
    let size = fish.traits.size;
    Fight::new(FightSetup {
        pull: fishing::pull(size),
        run: fishing::run(size),
        stamina: fishing::stamina(size),
        strength: line.reel.0,
        capacity: line.reel.1,
        distance: line.distance.max(venture_simulation::fishing::LAND_DISTANCE + 1.0),
        seed: fastrand::u64(..),
    })
}

fn act_on_requests(
    mut commands: Commands,
    mut requests: MessageReader<FishingRequest>,
    mut link: ResMut<Link>,
    content: Res<LoadedContent>,
    world: Res<ServerWorld>,
    rules: Res<FishingRules>,
    time: Res<Time>,
    mut players: Query<(&Player, &Position, &Equipment, &Character, &mut Inventory, Option<&mut Fishing>)>,
) {
    let now = time.elapsed_secs();
    let items = &content.pack.items;
    for request in requests.read() {
        let Ok((player, &Position(from), equipment, character, mut inventory, fishing)) =
            players.get_mut(request.player)
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
                let candidates = candidates(items, &line, water, skill);
                let rolls = [fastrand::f32(), fastrand::f32(), fastrand::f32()];
                let stage = plan_bite(&candidates, line.method, now, rules.0.patience, rolls);
                link.send(
                    connection,
                    &ServerMessage::Fishing(FishingState::Waiting { at: line.at, method: line.method }),
                );
                commands.entity(request.player).insert(Fishing { line, stage });
            }
            FishingAction::Strike => {
                let Some(mut fishing) = fishing else { continue };
                let method = fishing.line.method;
                match &fishing.stage {
                    Stage::Biting { fish, since } => {
                        if fishing::strike_hooks(method, now - since, rules.0.strike_allowance) {
                            let Some(Some(FishingPart::Fish(def))) = items.get(fish).map(|item| &item.fishing) else {
                                continue;
                            };
                            let fight = Box::new(hook(&fishing.line, def));
                            fishing.stage = Stage::Fighting { fish: fish.clone(), fight, reeling: false };
                        }
                        // (Too late is noticed by `run_lines`.)
                    }
                    Stage::Waiting { .. } => {
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
            FishingAction::Reel(reel) => {
                if let Some(mut fishing) = fishing
                    && let Stage::Fighting { reeling, .. } = &mut fishing.stage
                {
                    *reeling = *reel;
                }
            }
            FishingAction::Stop => {
                if fishing.is_some() {
                    end(&mut commands, &mut link, request.player, connection, "You reel in.".to_string());
                }
            }
        }
    }
}

/// Ends fishing for players who've moved, started a fight or died.
fn stop_when_busy(
    mut commands: Commands,
    mut link: ResMut<Link>,
    players: Query<(Entity, &Player, &Mover, &Health, Has<Attacking>), With<Fishing>>,
) {
    for (entity, player, mover, health, attacking) in &players {
        if mover.0.is_moving() || attacking || health.current() == 0 {
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
    mut players: Query<(Entity, &Player, &mut Fishing, &mut Inventory, &mut Character)>,
) {
    let now = time.elapsed_secs();
    let items = &content.pack.items;
    for (entity, player, mut fishing, mut inventory, mut character) in &mut players {
        let connection = player.connection;
        let send = |link: &mut Link, state: FishingState| link.send(connection, &ServerMessage::Fishing(state));
        let method = fishing.line.method;
        let tackle_name = named(&fishing.line.tackle, items);
        let fish_def = |fish: &str| match items.get(fish).map(|item| &item.fishing) {
            Some(Some(FishingPart::Fish(def))) => Some(def),
            _ => None,
        };
        let line = fishing.line.clone();
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
                    let text = format!("Nothing here takes the {tackle_name}.");
                    end(&mut commands, &mut link, entity, connection, text);
                    continue;
                };
                send(&mut link, FishingState::Bite);
                if method == Method::Lure {
                    // Fish strike lures hard enough to hook themselves.
                    let Some(def) = fish_def(&fish) else { continue };
                    fishing.stage = Stage::Fighting { fight: Box::new(hook(&line, def)), fish, reeling: false };
                } else {
                    fishing.stage = Stage::Biting { fish, since: now };
                }
            }
            Stage::Biting { since, .. } => {
                let window = fishing::strike_window(method).unwrap_or(0.0) + rules.0.strike_allowance;
                if now - *since > window {
                    let text = format!("Too late: the fish got away with your {tackle_name}.");
                    end(&mut commands, &mut link, entity, connection, text);
                }
            }
            Stage::Fighting { fish, fight, reeling } => {
                let outcome = fight.step(*reeling);
                send(&mut link, FishingState::Fight(**fight));
                let Some(outcome) = outcome else { continue };
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
    fn bites_come_after_nibbles_or_not_at_all() {
        let items = items();
        let Some(FishingPart::Fish(roach)) = &items["roach"].fishing else { panic!("roach is a fish") };
        let candidates = [("roach", roach, 1.0)];
        let Stage::Waiting { fish, bite_at, nibbles } = plan_bite(&candidates, Method::Float, 100.0, 20.0, [0.5; 3])
        else {
            panic!("a new line waits")
        };
        assert_eq!(fish.as_deref(), Some("roach"));
        assert!((110.0..=125.0).contains(&bite_at), "a skittish fish takes 10 to 25 seconds: {bite_at}");
        assert_eq!(nibbles.len(), 2, "a skittish fish nibbles more");
        assert!(nibbles.windows(2).all(|pair| pair[0] > pair[1]), "soonest last");
        assert!(nibbles.iter().all(|&nibble| (100.0..bite_at).contains(&nibble)));

        let Stage::Waiting { fish, bite_at, .. } = plan_bite(&[], Method::Float, 100.0, 20.0, [0.5; 3]) else {
            panic!("a new line waits")
        };
        assert_eq!((fish, bite_at), (None, 120.0), "nothing bites, and the player is told after a while");
    }
}
