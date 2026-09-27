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

use bevy::prelude::*;
use venture_protocol::{AdminCommand, ServerMessage};
use venture_rpg::MAX_LEVEL;

use crate::character::{Admin, Character, class_or_default, find_class};
use crate::clock::WorldClock;
use crate::content::LoadedContent;
use crate::entities::{NetworkId, Player};
use crate::equipment::{Equipment, player_look};
use crate::link::Link;
use crate::requests::AdminRequest;
use crate::{GameRules, ServerSystems};

/// The fastest an admin can make the days go (a whole day in under ten
/// seconds, at the usual two hours a day).
const MOST_TIMESCALE: f32 = 1000.0;

/// Carries out admin commands.
pub struct AdminPlugin;

impl Plugin for AdminPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, carry_out_admin_commands.in_set(ServerSystems::Gameplay));
    }
}

fn carry_out_admin_commands(
    mut requests: MessageReader<AdminRequest>,
    content: Res<LoadedContent>,
    mut link: ResMut<Link>,
    mut players: Query<(&Player, &NetworkId, &mut Character, &Equipment, Has<Admin>)>,
    everyone: Query<&Player>,
    mut clock: ResMut<WorldClock>,
    rules: Res<GameRules>,
) {
    let pack = &content.pack;
    for request in requests.read() {
        let Ok((player, &NetworkId(id), mut character, equipment, admin)) = players.get_mut(request.player) else {
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
