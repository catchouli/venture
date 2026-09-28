//! Admin powers: commands only admins may use. The only code that carries
//! them out, and it checks every time that the player really is an admin
//! (listed by name in `server.ron`; see [`crate::character::Admin`]).
//!
//! - `/setlevel N`: set your level, with the points the new levels bring.
//! - `/reset`: start your character over at level 1.
//! - `/setclass CLASS`: change class, with every point your level has earned
//!   given back to spend again.
//! - `/setgender BODY`: change your body type (masculine, feminine or androgynous).
//! - `/time HOUR`: set the time of day, for everyone.
//! - `/timescale N`: set how fast the days go, for everyone (1 the usual
//!   pace, 10 ten times faster, 0 stopped).
//! - `/give ITEM [COUNT]`: put items in your bag.
//! - `/setskill SKILL LEVEL`: set your level in a skill (such as fishing).
//! - `/tp NAME`, `/tp NAME OTHER`, `/tp X Y`, `/tp NAME X Y`: move yourself
//!   (or a player) to a player or a place (the nearest ground to stand on).
//! - `/setspeed N`: run N times the usual speed (until you leave the game).

use bevy::prelude::*;
use venture_protocol::{AdminCommand, ServerMessage, TeleportTo};
use venture_rpg::{MAX_LEVEL, MAX_SKILL_LEVEL};
use venture_simulation::{Movement, RUN_SPEED};

use crate::abilities::PendingAbility;
use crate::character::{Admin, Character, class_or_default, find_class};
use crate::clock::WorldClock;
use crate::combat::{Attacking, Resting};
use crate::content::LoadedContent;
use crate::entities::{DisplayName, Following, Mover, NetworkId, PendingInteraction, Player, Position, RunSpeed};
use crate::equipment::{Equipment, player_look};
use crate::inventory::{Change, Inventory, describe_items};
use crate::link::Link;
use crate::requests::AdminRequest;
use crate::world::ServerWorld;
use crate::{GameRules, ServerSystems};

/// The most of an item `/give` gives at once.
const MOST_GIVEN: u32 = 1000;

/// The fastest an admin can make the days go (a whole day in under ten
/// seconds, at the usual two hours a day).
const MOST_TIMESCALE: f32 = 1000.0;

/// The slowest and fastest an admin can make themselves run, as times the
/// usual speed.
const SPEEDS: std::ops::RangeInclusive<f32> = 0.1..=20.0;

/// How far from where `/tp` aims it may put a player, to find ground to
/// stand on (out of water, off a cliff), in metres.
const TELEPORT_SEARCH: f32 = 40.0;

/// Carries out admin commands.
pub struct AdminPlugin;

impl Plugin for AdminPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, carry_out_admin_commands.in_set(ServerSystems::Gameplay));
    }
}

fn carry_out_admin_commands(
    mut commands: Commands,
    mut requests: MessageReader<AdminRequest>,
    world: Res<ServerWorld>,
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &NetworkId, &mut Character, &Equipment, &mut Inventory, Has<Admin>)>,
    everyone: Query<&Player>,
    mut places: Query<(Entity, &Player, &DisplayName, &mut Position, &mut Mover)>,
    mut clock: ResMut<WorldClock>,
    rules: Res<GameRules>,
) {
    let pack = &content.pack;
    for request in requests.read() {
        let Ok((player, &NetworkId(id), mut character, equipment, mut inventory, admin)) =
            players.get_mut(request.player)
        else {
            continue;
        };
        let common = &pack.new_characters.abilities;
        let connection = player.connection;
        let mut tell = |text: String| link.send(connection, &ServerMessage::Notice { text });
        if !admin {
            tell("Only admins can use that command.".to_string());
            continue;
        }
        let old_level = character.level();
        let class = class_or_default(pack, Some(&character.class));
        let old_look = player_look(&character, equipment, &pack.items);
        match &request.command {
            AdminCommand::SetLevel { level } => {
                if !(1..=MAX_LEVEL).contains(level) {
                    tell(format!("Levels go from 1 to {MAX_LEVEL}."));
                    continue;
                }
                character.set_level(*level, &class, common);
                tell(format!("You are now level {level}."));
            }
            AdminCommand::Reset => {
                character.reset(&class, common);
                tell(format!("Your character has been reset to a level 1 {}.", class.name));
            }
            AdminCommand::SetClass { class: wanted } => {
                let Some(new_class) = find_class(pack, wanted) else {
                    let names: Vec<&str> = pack.classes.iter().map(|class| class.name.as_str()).collect();
                    tell(format!("There's no class called \"{wanted}\". The classes are: {}.", names.join(", ")));
                    continue;
                };
                character.change_class(new_class, common);
                tell(format!(
                    "You are now a {}. Your stats and abilities are back to a {}'s, and your points are yours to spend again.",
                    new_class.name, new_class.name
                ));
            }
            AdminCommand::SetBody { body } => {
                character.body = *body;
                tell(format!("Your character is now {}.", body.name().to_lowercase()));
            }
            AdminCommand::SetTime { hour } => {
                if !(0.0..24.0).contains(hour) {
                    tell("The time is an hour from 0 to 24.".to_string());
                    continue;
                }
                clock.set(*hour);
                let (hours, minutes) = (hour.floor() as u32, (hour.fract() * 60.0).round() as u32 % 60);
                tell(format!("The time is now {hours:02}:{minutes:02}."));
                let message = clock.message(&rules);
                link.broadcast(everyone.iter().map(|player| player.connection), &message);
                continue;
            }
            AdminCommand::Give { item, count } => {
                let found = pack
                    .items
                    .get(item)
                    .or_else(|| pack.items.values().find(|def| def.name.eq_ignore_ascii_case(item)));
                let Some(def) = found else {
                    tell(format!("There's no item called \"{item}\"."));
                    continue;
                };
                let count = (*count).clamp(1, MOST_GIVEN);
                match inventory.apply(&Change::give(&def.id, count), &pack.items) {
                    Ok(()) => tell(format!("Added to your bag: {}.", describe_items(&def.id, count, &pack.items))),
                    Err(_) => tell("There isn't room in your bag.".to_string()),
                }
                continue;
            }
            AdminCommand::SetSkill { skill, level } => {
                let found = pack.skills.iter().find(|def| def.id == *skill || def.name.eq_ignore_ascii_case(skill));
                let Some(def) = found else {
                    let names: Vec<&str> = pack.skills.iter().map(|skill| skill.name.as_str()).collect();
                    tell(format!("There's no skill called \"{skill}\". The skills are: {}.", names.join(", ")));
                    continue;
                };
                if !(1..=MAX_SKILL_LEVEL).contains(level) {
                    tell(format!("Skill levels go from 1 to {MAX_SKILL_LEVEL}."));
                    continue;
                }
                character.set_skill_level(&def.id, *level);
                tell(format!("Your {} level is now {level}.", def.name.to_lowercase()));
                continue;
            }
            AdminCommand::Teleport { who, to } => {
                let find = |name: &str| {
                    places
                        .iter()
                        .find(|(_, _, shown, ..)| shown.0.eq_ignore_ascii_case(name))
                        .map(|(entity, ..)| entity)
                };
                let moved = match who {
                    None => request.player,
                    Some(name) => match find(name) {
                        Some(entity) => entity,
                        None => {
                            tell(format!("There's nobody called \"{name}\" in the game."));
                            continue;
                        }
                    },
                };
                let (aim, place) = match to {
                    TeleportTo::Player(name) => {
                        let Some(target) = find(name) else {
                            tell(format!("There's nobody called \"{name}\" in the game."));
                            continue;
                        };
                        if target == moved {
                            tell("They're already there.".to_string());
                            continue;
                        }
                        let Ok((_, _, shown, position, _)) = places.get(target) else { continue };
                        (position.0, shown.0.clone())
                    }
                    TeleportTo::Place { x, y } => {
                        if !world.contains((*x, *y)) {
                            let size = world.manifest.world_size;
                            tell(format!("That's outside the world, which goes from 0 to {size} each way."));
                            continue;
                        }
                        ((*x, *y), format!("{x:.0}, {y:.0}"))
                    }
                };
                let Some(spot) = world.open_ground_near(aim, TELEPORT_SEARCH) else {
                    tell(format!("There's nowhere to stand near {place}."));
                    continue;
                };
                let Ok((_, mover_player, mover_name, mut position, mut mover)) = places.get_mut(moved) else {
                    continue;
                };
                position.0 = spot;
                mover.0 = Movement::Idle;
                commands.entity(moved).remove::<(Following, PendingInteraction, Attacking, PendingAbility, Resting)>();
                let text = if moved == request.player {
                    format!("You teleport to {place}.")
                } else {
                    let notice = ServerMessage::Notice { text: format!("An admin moved you to {place}.") };
                    link.send(mover_player.connection, &notice);
                    format!("You moved {} to {place}.", mover_name.0)
                };
                link.send(connection, &ServerMessage::Notice { text });
                continue;
            }
            AdminCommand::SetSpeed { scale } => {
                if !scale.is_finite() || !SPEEDS.contains(scale) {
                    tell(format!("Speeds go from {} to {} times the usual.", SPEEDS.start(), SPEEDS.end()));
                    continue;
                }
                if *scale == 1.0 {
                    commands.entity(request.player).remove::<RunSpeed>();
                } else {
                    commands.entity(request.player).insert(RunSpeed(RUN_SPEED * scale));
                }
                link.send(connection, &ServerMessage::RunSpeed { speed: RUN_SPEED * scale });
                link.send(
                    connection,
                    &ServerMessage::Notice { text: format!("You now run {scale} times the usual speed.") },
                );
                continue;
            }
            AdminCommand::SetTimescale { scale } => {
                if !scale.is_finite() || !(0.0..=MOST_TIMESCALE).contains(scale) {
                    tell(format!("The timescale is from 0 (stopped) to {MOST_TIMESCALE}."));
                    continue;
                }
                clock.set_scale(*scale, &rules);
                tell(if *scale == 0.0 {
                    "The clock has stopped.".to_string()
                } else {
                    format!("Days now go {scale} times the usual pace.")
                });
                let message = clock.message(&rules);
                link.broadcast(everyone.iter().map(|player| player.connection), &message);
                continue;
            }
        }
        // Everyone sees the new level and look (and a level-up celebrates).
        let level = character.level();
        let look = player_look(&character, equipment, &pack.items);
        if level != old_level || look != old_look {
            let message = ServerMessage::PlayerChanged { id, level, look, celebrate: level > old_level };
            link.broadcast(everyone.iter().map(|player| player.connection), &message);
        }
    }
}
