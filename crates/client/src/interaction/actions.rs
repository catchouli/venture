//! Actions: the things a player can do, each with a name.
//!
//! Every way of doing something asks for an [`Action`] with a
//! [`PerformAction`] message, and [`perform_actions`] carries it out. Clicking
//! a selected character, typing a chat command (`/follow`), and later an
//! action bar or a key binding all behave the same. To add an action, add it
//! to [`Action`] and [`Action::ALL`], and handle it in [`perform_actions`].
//!
//! Actions on the selected target are sent to the server even when they
//! can't work (such as following an NPC): the server decides, and explains
//! in chat. They're only predicted locally when they will work.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, Emote};

use crate::interaction::admin_commands::ADMIN_COMMANDS;
use crate::interaction::orders::Orders;
use crate::interaction::targeting::{Target, TargetKind, Targetable};
use crate::network::ClientState;
use crate::ui::chat::SystemMessage;

/// Carries out actions.
pub struct ActionsPlugin;

impl Plugin for ActionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PerformAction>().add_systems(Update, perform_actions.run_if(in_state(ClientState::Playing)));
    }
}

/// Something the player can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Follow the selected player.
    Follow,
    /// Talk to the selected NPC.
    Talk,
    /// Go back to the starting town.
    Town,
    /// Wave hello.
    Wave,
    /// List the chat commands.
    Help,
}

impl Action {
    /// Every action, in the order `/help` lists them.
    pub const ALL: [Action; 5] = [Action::Follow, Action::Talk, Action::Wave, Action::Town, Action::Help];

    /// Its name, as typed in a chat command (`/follow`).
    pub fn name(self) -> &'static str {
        match self {
            Action::Follow => "follow",
            Action::Talk => "talk",
            Action::Town => "town",
            Action::Wave => "wave",
            Action::Help => "help",
        }
    }

    /// What it does, in a few words.
    pub fn description(self) -> &'static str {
        match self {
            Action::Follow => "follow the selected player",
            Action::Talk => "talk to the selected NPC",
            Action::Town => "go back to the starting town",
            Action::Wave => "wave hello to everyone nearby",
            Action::Help => "list the commands",
        }
    }

    /// The action with this name (ignoring case), if there is one.
    pub fn named(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|action| action.name().eq_ignore_ascii_case(name))
    }
}

/// Asks for an action to be carried out.
#[derive(Message, Clone, Copy, Debug)]
pub struct PerformAction(pub Action);

fn perform_actions(
    mut actions: MessageReader<PerformAction>,
    target: Res<Target>,
    targetables: Query<&Targetable>,
    mut orders: Orders,
    mut system: MessageWriter<SystemMessage>,
) {
    let selected = target.0.and_then(|entity| Some((entity, *targetables.get(entity).ok()?)));
    for &PerformAction(action) in actions.read() {
        match (action, selected) {
            (Action::Follow, None) => {
                system.write(SystemMessage("Select a player to follow first.".to_string()));
            }
            (Action::Follow, Some((entity, Targetable { id, kind: TargetKind::Player }))) => orders.follow(entity, id),
            (Action::Follow, Some((_, Targetable { id, .. }))) => orders.ask(ClientMessage::Follow { target: id }),
            (Action::Talk, None) => {
                system.write(SystemMessage("Select someone to talk to first.".to_string()));
            }
            (Action::Talk, Some((entity, Targetable { id, kind: TargetKind::Npc }))) => orders.talk_to(entity, id),
            (Action::Talk, Some((_, Targetable { id, .. }))) => orders.ask(ClientMessage::Interact { target: id }),
            (Action::Town, _) => orders.ask(ClientMessage::ReturnToTown),
            (Action::Wave, _) => orders.ask(ClientMessage::Emote(Emote::Wave)),
            (Action::Help, _) => {
                // Admins also get the admin commands, marked as such.
                let mut commands: Vec<(String, &str)> =
                    Action::ALL.iter().map(|action| (action.name().to_string(), action.description())).collect();
                let actions = commands.len();
                if orders.is_admin() {
                    let admin = ADMIN_COMMANDS.iter().map(|(name, usage, description)| {
                        (format!("{name} {usage}").trim_end().to_string(), *description)
                    });
                    commands.extend(admin);
                }
                let width = commands.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
                system.write(SystemMessage("Commands:".to_string()));
                for (index, (name, description)) in commands.iter().enumerate() {
                    if index == actions {
                        system.write(SystemMessage("Admin commands:".to_string()));
                    }
                    let marker = if index >= actions { " [admin]" } else { "" };
                    system.write(SystemMessage(format!("  /{name:width$}  {description}{marker}")));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_found_by_name() {
        assert_eq!(Action::named("follow"), Some(Action::Follow));
        assert_eq!(Action::named("TALK"), Some(Action::Talk));
        assert_eq!(Action::named("fly"), None);
        for action in Action::ALL {
            assert_eq!(Action::named(action.name()), Some(action), "names are unique");
        }
    }
}
