//! Admin powers: commands only admins may use. The only code that carries
//! them out, and it checks every time that the player really is an admin
//! (listed by name in `server.ron`; see [`crate::character::Admin`]).
//!
//! - `/setlevel N`: set your level, with the points the new levels bring.
//! - `/reset`: start your character over at level 1.
//! - `/setclass CLASS`: change class, with every point your level has earned
//!   given back to spend again.

use bevy::prelude::*;
use venture_protocol::{AdminCommand, ServerMessage};
use venture_rpg::MAX_LEVEL;

use crate::ServerSystems;
use crate::character::{Admin, Character, class_or_default, find_class};
use crate::content::LoadedContent;
use crate::entities::{NetworkId, Player};
use crate::link::Link;
use crate::requests::AdminRequest;

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
    mut players: Query<(&Player, &NetworkId, &mut Character, Has<Admin>)>,
    everyone: Query<&Player>,
) {
    let pack = &content.pack;
    for request in requests.read() {
        let Ok((player, &NetworkId(id), mut character, admin)) = players.get_mut(request.player) else { continue };
        let connection = player.connection;
        let mut tell = |text: String| link.send(connection, &ServerMessage::Notice { text });
        if !admin {
            tell("Only admins can use that command.".to_string());
            continue;
        }
        let old_level = character.level();
        let class = class_or_default(pack, Some(&character.class));
        let old_look = character.look(&class);
        match &request.command {
            AdminCommand::SetLevel { level } => {
                if !(1..=MAX_LEVEL).contains(level) {
                    tell(format!("Levels go from 1 to {MAX_LEVEL}."));
                    continue;
                }
                character.set_level(*level, &class);
                tell(format!("You are now level {level}."));
            }
            AdminCommand::Reset => {
                character.reset(&class);
                tell(format!("Your character has been reset to a level 1 {}.", class.name));
            }
            AdminCommand::SetClass { class: wanted } => {
                let Some(new_class) = find_class(pack, wanted) else {
                    let names: Vec<&str> = pack.classes.iter().map(|class| class.name.as_str()).collect();
                    tell(format!("There's no class called \"{wanted}\". The classes are: {}.", names.join(", ")));
                    continue;
                };
                character.change_class(new_class);
                tell(format!(
                    "You are now a {}. Your stats are back to a {}'s, and your points are yours to spend again.",
                    new_class.name, new_class.name
                ));
            }
        }
        // Everyone sees the new level and look (and a level-up celebrates).
        let level = character.level();
        let look = character.look(&class_or_default(pack, Some(&character.class)));
        if level != old_level || look != old_look {
            let message = ServerMessage::PlayerChanged { id, level, look, celebrate: level > old_level };
            link.broadcast(everyone.iter().map(|player| player.connection), &message);
        }
    }
}
