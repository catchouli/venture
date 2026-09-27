//! Actions: the things a player can do, each with a name.
//!
//! Every way of doing something asks for an [`Action`] with a
//! [`PerformAction`] message, and [`perform_actions`] carries it out. Clicking
//! a selected character, typing a chat command (`/follow`, `/party Wanderers`),
//! pressing a button, and the hotbar all behave the same. To add an action, add
//! it to [`Action`] and [`Action::ALL`], and handle it in [`perform_actions`].
//!
//! Actions on the selected target are sent to the server even when they
//! can't work (such as following an NPC): the server decides, and explains
//! in chat. They're only predicted locally when they will work.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, Emote, HotbarAction, PartyCommand};

use crate::characters::nameplates::Nameplate;
use crate::characters::player::LocalPlayer;
use crate::interaction::admin_commands::ADMIN_COMMANDS;
use crate::interaction::orders::Orders;
use crate::interaction::targeting::{NEARBY_TARGET_RANGE, Target, TargetKind, Targetable, nearest};
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
    /// Attack the selected monster.
    Attack,
    /// Select something: by name, or the nearest monster within reach.
    Target,
    /// Follow the selected player.
    Follow,
    /// Talk to the selected NPC.
    Talk,
    /// Go back to the starting town.
    Town,
    /// Wave hello.
    Wave,
    /// Start a party.
    Party,
    /// Invite a player to your party.
    Invite,
    /// Accept a party invitation.
    Accept,
    /// Decline a party invitation.
    Decline,
    /// Leave your party.
    Leave,
    /// List the chat commands.
    Help,
}

impl Action {
    /// Every action, in the order `/help` lists them.
    pub const ALL: [Action; 12] = [
        Action::Attack,
        Action::Target,
        Action::Follow,
        Action::Talk,
        Action::Wave,
        Action::Town,
        Action::Party,
        Action::Invite,
        Action::Accept,
        Action::Decline,
        Action::Leave,
        Action::Help,
    ];

    /// Its name, as typed in a chat command (`/follow`).
    pub fn name(self) -> &'static str {
        match self {
            Action::Attack => "attack",
            Action::Target => "target",
            Action::Follow => "follow",
            Action::Talk => "talk",
            Action::Town => "town",
            Action::Wave => "wave",
            Action::Party => "party",
            Action::Invite => "invite",
            Action::Accept => "accept",
            Action::Decline => "decline",
            Action::Leave => "leave",
            Action::Help => "help",
        }
    }

    /// What follows its name in a chat command, for `/help` ("NAME", or
    /// "[NAME]" if it can be left out).
    pub fn usage(self) -> &'static str {
        match self {
            Action::Party => "NAME",
            Action::Target | Action::Invite => "[NAME]",
            _ => "",
        }
    }

    /// What it does, in a few words.
    pub fn description(self) -> &'static str {
        match self {
            Action::Attack => "attack the selected monster",
            Action::Target => "select the nearest monster within 5 m, or anyone nearby by name",
            Action::Follow => "follow the selected player",
            Action::Talk => "talk to the selected NPC",
            Action::Town => "go back to the starting town",
            Action::Wave => "wave hello to everyone nearby",
            Action::Party => "start a party",
            Action::Invite => "invite the selected player (or a player by name) to your party",
            Action::Accept => "join the party you've been invited to",
            Action::Decline => "turn down a party invitation",
            Action::Leave => "leave your party",
            Action::Help => "list the commands",
        }
    }

    /// The action with this name (ignoring case), if there is one.
    pub fn named(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|action| action.name().eq_ignore_ascii_case(name))
    }

    /// The action as it goes on the hotbar (not all of them can).
    pub fn on_hotbar(self) -> Option<HotbarAction> {
        match self {
            Action::Attack => Some(HotbarAction::Attack),
            Action::Target => Some(HotbarAction::Target),
            Action::Follow => Some(HotbarAction::Follow),
            Action::Talk => Some(HotbarAction::Talk),
            Action::Wave => Some(HotbarAction::Wave),
            Action::Town => Some(HotbarAction::Town),
            Action::Party | Action::Invite | Action::Accept | Action::Decline | Action::Leave | Action::Help => None,
        }
    }

    /// The action a hotbar slot does.
    pub fn from_hotbar(action: HotbarAction) -> Action {
        match action {
            HotbarAction::Attack => Action::Attack,
            HotbarAction::Target => Action::Target,
            HotbarAction::Follow => Action::Follow,
            HotbarAction::Talk => Action::Talk,
            HotbarAction::Wave => Action::Wave,
            HotbarAction::Town => Action::Town,
        }
    }

    /// Its picture: an icon file in the content's `icons/`, if it has one.
    pub fn icon(self) -> Option<&'static str> {
        match self {
            Action::Attack => Some("action_attack.png"),
            Action::Target => Some("action_target.png"),
            Action::Talk => Some("action_talk.png"),
            Action::Follow => Some("action_follow.png"),
            Action::Wave => Some("emote_wave.png"),
            Action::Town => Some("action_town.png"),
            _ => None,
        }
    }

    /// Its name with a capital letter, for tooltips ("Attack").
    pub fn title(self) -> String {
        let mut title = self.name().to_string();
        title[..1].make_ascii_uppercase();
        title
    }
}

/// Asks for an action to be carried out, with what was written after its
/// name in chat, if anything (`/party Wanderers`).
#[derive(Message, Clone, Debug)]
pub struct PerformAction {
    /// The action.
    pub action: Action,
    /// What follows it (trimmed, and not empty).
    pub argument: Option<String>,
}

impl PerformAction {
    /// The action, with nothing after it.
    pub fn new(action: Action) -> Self {
        Self { action, argument: None }
    }

    /// The action, with this after it (nothing, if it's blank).
    pub fn with(action: Action, argument: &str) -> Self {
        let argument = argument.trim();
        Self { action, argument: (!argument.is_empty()).then(|| argument.to_string()) }
    }
}

fn perform_actions(
    mut actions: MessageReader<PerformAction>,
    mut target: ResMut<Target>,
    targetables: Query<&Targetable>,
    candidates: Query<(Entity, &Targetable, &Nameplate, &Transform), Without<LocalPlayer>>,
    mut orders: Orders,
    mut system: MessageWriter<SystemMessage>,
) {
    for PerformAction { action, argument } in actions.read() {
        let selected = target.0.and_then(|entity| Some((entity, *targetables.get(entity).ok()?)));
        let mut say = |text: &str| {
            system.write(SystemMessage(text.to_string()));
        };
        match (*action, selected, argument.as_deref()) {
            (Action::Follow, None, _) => say("Select a player to follow first."),
            (Action::Follow, Some((entity, Targetable { id, kind: TargetKind::Player })), _) => {
                orders.follow(entity, id);
            }
            (Action::Follow, Some((_, Targetable { id, .. })), _) => orders.ask(ClientMessage::Follow { target: id }),
            (Action::Talk, None, _) => say("Select someone to talk to first."),
            (Action::Talk, Some((entity, Targetable { id, kind: TargetKind::Npc })), _) => orders.talk_to(entity, id),
            (Action::Talk, Some((_, Targetable { id, .. })), _) => orders.ask(ClientMessage::Interact { target: id }),
            (Action::Attack, None, _) => say("Select a monster to attack first."),
            (Action::Attack, Some((entity, Targetable { id, kind: TargetKind::Enemy })), _) => {
                orders.attack(entity, id);
            }
            (Action::Attack, Some((_, Targetable { id, .. })), _) => orders.ask(ClientMessage::Attack { target: id }),
            (Action::Target, _, wanted) => {
                let Some(from) = orders.position() else { continue };
                let found = select(&candidates, from, wanted);
                match (found, wanted) {
                    (Some(entity), _) => target.0 = Some(entity),
                    (None, Some(name)) => say(&format!("There's nobody called {name} nearby.")),
                    (None, None) => say(&format!(
                        "There are no monsters within {NEARBY_TARGET_RANGE} m. Write /target NAME to select someone by name."
                    )),
                }
            }
            (Action::Town, ..) => orders.ask(ClientMessage::ReturnToTown),
            (Action::Wave, ..) => orders.ask(ClientMessage::Emote(Emote::Wave)),
            (Action::Party, _, Some(name)) => {
                orders.ask(ClientMessage::Party(PartyCommand::Create { name: name.to_string() }));
            }
            (Action::Party, _, None) => say("Write it as /party NAME, to start a party called NAME."),
            (Action::Invite, _, Some(name)) => {
                orders.ask(ClientMessage::Party(PartyCommand::InviteNamed { name: name.to_string() }));
            }
            (Action::Invite, Some((_, Targetable { id, kind: TargetKind::Player })), None) => {
                orders.ask(ClientMessage::Party(PartyCommand::Invite { target: id }));
            }
            (Action::Invite, Some(_), None) => say("You can only invite players to a party."),
            (Action::Invite, None, None) => say("Select a player to invite, or write /invite NAME."),
            (Action::Accept, ..) => orders.ask(ClientMessage::Party(PartyCommand::Accept)),
            (Action::Decline, ..) => orders.ask(ClientMessage::Party(PartyCommand::Decline)),
            (Action::Leave, ..) => orders.ask(ClientMessage::Party(PartyCommand::Leave)),
            (Action::Help, ..) => {
                // Admins also get the admin commands, marked as such.
                let with_usage = |name: &str, usage: &str| format!("{name} {usage}").trim_end().to_string();
                let mut commands: Vec<(String, &str)> = Action::ALL
                    .iter()
                    .map(|action| (with_usage(action.name(), action.usage()), action.description()))
                    .collect();
                let actions = commands.len();
                if orders.is_admin() {
                    let admin =
                        ADMIN_COMMANDS.iter().map(|(name, usage, description)| (with_usage(name, usage), *description));
                    commands.extend(admin);
                }
                let width = commands.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
                say("Commands:");
                for (index, (name, description)) in commands.iter().enumerate() {
                    if index == actions {
                        say("Admin commands:");
                    }
                    let marker = if index >= actions { " [admin]" } else { "" };
                    say(&format!("  /{name:width$}  {description}{marker}"));
                }
            }
        }
    }
}

/// What `/target` selects: the nearest NPC or monster called `name`
/// (ignoring case), or without a name, the nearest monster within
/// [`NEARBY_TARGET_RANGE`] (only that near, so it can't be used to pull
/// fights from afar).
fn select(
    candidates: &Query<(Entity, &Targetable, &Nameplate, &Transform), Without<LocalPlayer>>,
    from: Vec3,
    name: Option<&str>,
) -> Option<Entity> {
    let fits = |targetable: &Targetable, nameplate: &Nameplate| match name {
        Some(name) => targetable.kind != TargetKind::Player && nameplate.0.eq_ignore_ascii_case(name),
        None => targetable.kind == TargetKind::Enemy,
    };
    let found = nearest(
        from,
        candidates
            .iter()
            .filter(|(_, targetable, nameplate, _)| fits(targetable, nameplate))
            .map(|(entity, _, _, transform)| (entity, transform.translation)),
    )?;
    (name.is_some() || found.1 <= NEARBY_TARGET_RANGE).then_some(found.0)
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
            if let Some(on_hotbar) = action.on_hotbar() {
                assert_eq!(Action::from_hotbar(on_hotbar), action, "hotbar actions go both ways");
            }
        }
    }
}
