//! Parties: groups of players. The only code that changes who's in a party.
//!
//! Everything is kept in memory, in [`Parties`]: the parties (a name and
//! the players in it, in the order they joined) and the open invitations
//! (one per invited player, each running out after `party.invite_seconds`
//! from `server.ron`). Nothing is saved: leaving the game leaves your party,
//! and a party with nobody left in it is gone.
//!
//! - `/party NAME` starts a party (names are unique while it lasts).
//! - `/invite` (or `/invite NAME`) invites a player who isn't in a party and
//!   has no other invitation open. They have until the invitation runs out
//!   to `/accept` or `/decline` it; either way, or when it runs out, both
//!   sides are told in chat.
//! - `/leave` leaves the party.
//!
//! Members are sent their party ([`ServerMessage::Party`]) whenever anything
//! shown about it changes (who's in it, their levels, health and mana), and
//! an invited player is sent the invitation ([`ServerMessage::PartyInvite`])
//! and told when it's over. The client's party window and frames are only
//! a view of these, and its buttons send the same commands as the chat.
//!
//! Members near a kill share its experience equally (see
//! [`crate::combat`], which asks [`Parties::members_of`]).

use std::collections::HashMap;

use bevy::prelude::*;
use venture_protocol::{PartyInfo, PartyInvite, PartyMember, ServerMessage};

use crate::ServerSystems;
use crate::character::Character;
use crate::combat::{Health, Mana};
use crate::config::PartyConfig;
use crate::content::LoadedContent;
use crate::entities::{DisplayName, NetworkId, Player};
use crate::link::Link;
use crate::requests::{PartyAction, PartyRequest};

/// Runs parties.
pub struct PartiesPlugin;

impl Plugin for PartiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Parties>().add_systems(
            FixedUpdate,
            (
                (forget_leavers, handle_party_requests, expire_invitations).chain().in_set(ServerSystems::Gameplay),
                send_party_status.in_set(ServerSystems::Updates),
            ),
        );
    }
}

/// The rules for parties, from `server.ron`.
#[derive(Resource, Clone)]
pub struct PartyRules(pub PartyConfig);

/// Every party and open invitation.
#[derive(Resource, Default)]
pub struct Parties {
    parties: HashMap<u32, Party>,
    /// The id the next party gets.
    next_id: u32,
    /// Open invitations, by the invited player.
    invitations: HashMap<Entity, Invitation>,
}

/// One party.
struct Party {
    name: String,
    /// Everyone in it, in the order they joined.
    members: Vec<Entity>,
    /// What its members were last sent, so it's only sent again when it changes.
    shown: Option<PartyInfo>,
}

/// An invitation to join a party.
struct Invitation {
    party: u32,
    /// Who sent it.
    inviter: Entity,
    /// When it runs out (in the server's seconds).
    expires: f32,
}

/// Something to tell a player, as the result of a change to [`Parties`].
#[derive(Debug, PartialEq)]
pub enum Tell {
    /// A message in their chat.
    Notice(Entity, String),
    /// Their invitation (or that it's over).
    Invitation(Entity, Option<PartyInvite>),
    /// They're no longer in a party.
    NoParty(Entity),
}

impl Parties {
    /// The party a player is in, if any.
    fn party_of(&self, player: Entity) -> Option<u32> {
        self.parties.iter().find(|(_, party)| party.members.contains(&player)).map(|(&id, _)| id)
    }

    /// Everyone in `player`'s party, in the order they joined (just
    /// `player`, if they aren't in one). Only for reading: who's in a party
    /// only changes here.
    pub fn members_of(&self, player: Entity) -> Vec<Entity> {
        self.party_of(player).map_or_else(|| vec![player], |id| self.parties[&id].members.clone())
    }

    /// Starts a party called `name` (already a valid name), with `player` in it.
    pub fn create(&mut self, player: Entity, name: &str) -> Vec<Tell> {
        let notice = |text: String| vec![Tell::Notice(player, text)];
        if self.party_of(player).is_some() {
            return notice("You are already in a party.".to_string());
        }
        if let Some(existing) = self.parties.values().find(|party| party.name.eq_ignore_ascii_case(name)) {
            return notice(format!("There's already a party called {}.", existing.name));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.parties.insert(id, Party { name: name.to_string(), members: vec![player], shown: None });
        notice(format!("You have created the party {name}."))
    }

    /// Invites `invitee` (a player) to `inviter`'s party, open until
    /// `expires`. `name` gives players' names.
    pub fn invite(
        &mut self,
        inviter: Entity,
        invitee: Entity,
        rules: &PartyConfig,
        now: f32,
        name: impl Fn(Entity) -> String,
    ) -> Vec<Tell> {
        let notice = |text: String| vec![Tell::Notice(inviter, text)];
        let Some(id) = self.party_of(inviter) else {
            return notice("You aren't in a party. Start one with /party NAME.".to_string());
        };
        let party = &self.parties[&id];
        let who = name(invitee);
        if invitee == inviter {
            return notice("You can't invite yourself.".to_string());
        }
        if party.members.contains(&invitee) {
            return notice(format!("{who} is already in your party."));
        }
        if self.party_of(invitee).is_some() {
            return notice(format!("{who} is already in a party."));
        }
        if self.invitations.contains_key(&invitee) {
            return notice(format!("{who} already has a party invitation active."));
        }
        if party.members.len() >= rules.max_members as usize {
            return notice(format!("Your party is full ({} players).", rules.max_members));
        }
        let party_name = party.name.clone();
        self.invitations.insert(invitee, Invitation { party: id, inviter, expires: now + rules.invite_seconds });
        let offer = PartyInvite { from: name(inviter), party: party_name.clone(), seconds: rules.invite_seconds };
        vec![
            Tell::Notice(inviter, format!("Inviting {who} to party {party_name}...")),
            Tell::Notice(invitee, format!("You have been invited to party {party_name}.")),
            Tell::Invitation(invitee, Some(offer)),
        ]
    }

    /// `player` accepts their invitation.
    pub fn accept(&mut self, player: Entity, rules: &PartyConfig, name: impl Fn(Entity) -> String) -> Vec<Tell> {
        let Some(invitation) = self.invitations.remove(&player) else {
            return vec![Tell::Notice(player, "You don't have a party invitation.".to_string())];
        };
        let mut tells = vec![Tell::Invitation(player, None)];
        let Some(party) = self.parties.get_mut(&invitation.party) else {
            tells.push(Tell::Notice(player, "That party no longer exists.".to_string()));
            return tells;
        };
        if party.members.len() >= rules.max_members as usize {
            tells.push(Tell::Notice(player, format!("{} is full now.", party.name)));
            return tells;
        }
        let who = name(player);
        for &member in &party.members {
            let text = if member == invitation.inviter {
                format!("{who} accepted the party invite.")
            } else {
                format!("{who} has joined the party.")
            };
            tells.push(Tell::Notice(member, text));
        }
        party.members.push(player);
        tells.push(Tell::Notice(player, format!("You have joined {}.", party.name)));
        tells
    }

    /// `player` declines their invitation.
    pub fn decline(&mut self, player: Entity, name: impl Fn(Entity) -> String) -> Vec<Tell> {
        let Some(invitation) = self.invitations.remove(&player) else {
            return vec![Tell::Notice(player, "You don't have a party invitation.".to_string())];
        };
        let party = self.party_name(invitation.party);
        vec![
            Tell::Invitation(player, None),
            Tell::Notice(player, format!("You have declined to join {party}.")),
            Tell::Notice(invitation.inviter, format!("{} declined the party invite.", name(player))),
        ]
    }

    /// `player` leaves their party.
    pub fn leave(&mut self, player: Entity, name: impl Fn(Entity) -> String) -> Vec<Tell> {
        let Some(id) = self.party_of(player) else {
            return vec![Tell::Notice(player, "You aren't in a party.".to_string())];
        };
        let mut tells =
            vec![Tell::Notice(player, format!("You have left {}.", self.party_name(id))), Tell::NoParty(player)];
        tells.extend(self.remove_member(id, player, &name));
        tells
    }

    /// `player` has left the game: out of their party, and their invitation
    /// (sent or received) withdrawn.
    pub fn forget(&mut self, player: Entity, name: impl Fn(Entity) -> String) -> Vec<Tell> {
        let mut tells = Vec::new();
        if let Some(invitation) = self.invitations.remove(&player) {
            let text = format!("{} left before answering your party invite.", name(player));
            tells.push(Tell::Notice(invitation.inviter, text));
        }
        if let Some(id) = self.party_of(player) {
            tells.extend(self.remove_member(id, player, &name));
        }
        tells
    }

    /// Invitations that have run out by `now`.
    pub fn expire(&mut self, now: f32, name: impl Fn(Entity) -> String) -> Vec<Tell> {
        let expired: Vec<Entity> =
            self.invitations.iter().filter(|(_, invitation)| invitation.expires <= now).map(|(&who, _)| who).collect();
        let mut tells = Vec::new();
        for invitee in expired {
            let invitation = self.invitations.remove(&invitee).expect("just found");
            let party = self.party_name(invitation.party);
            tells.push(Tell::Invitation(invitee, None));
            tells.push(Tell::Notice(invitee, format!("Your invitation to join {party} has expired.")));
            tells
                .push(Tell::Notice(invitation.inviter, format!("Your party invite to {} has expired.", name(invitee))));
        }
        tells
    }

    /// Takes a player out of a party, telling the others; a party left
    /// empty is gone, with any invitations to it.
    fn remove_member(&mut self, id: u32, player: Entity, name: &impl Fn(Entity) -> String) -> Vec<Tell> {
        let party = self.parties.get_mut(&id).expect("the player's party exists");
        party.members.retain(|&member| member != player);
        let who = name(player);
        let mut tells: Vec<Tell> =
            party.members.iter().map(|&member| Tell::Notice(member, format!("{who} has left the party."))).collect();
        if party.members.is_empty() {
            let party = self.parties.remove(&id).expect("it's there");
            let withdrawn: Vec<Entity> =
                self.invitations.iter().filter(|(_, invitation)| invitation.party == id).map(|(&who, _)| who).collect();
            for invitee in withdrawn {
                self.invitations.remove(&invitee);
                tells.push(Tell::Invitation(invitee, None));
                tells.push(Tell::Notice(invitee, format!("{} has disbanded, so your invitation is void.", party.name)));
            }
        }
        tells
    }

    fn party_name(&self, id: u32) -> String {
        self.parties.get(&id).map_or_else(|| "the party".to_string(), |party| party.name.clone())
    }
}

/// Everything about a player that parties need.
type Member =
    (&'static Player, &'static NetworkId, &'static DisplayName, &'static Character, &'static Health, &'static Mana);

/// Takes players who've left the game out of parties and invitations.
/// (Players' names are remembered, as they're gone with the player.)
fn forget_leavers(
    mut removed: RemovedComponents<Player>,
    mut parties: ResMut<Parties>,
    mut known: Local<HashMap<Entity, String>>,
    named: Query<(Entity, &DisplayName), (With<Player>, Changed<DisplayName>)>,
    players: Query<&Player>,
    mut link: ResMut<Link>,
) {
    for player in removed.read() {
        let tells =
            parties.forget(player, |entity| known.get(&entity).cloned().unwrap_or_else(|| "Someone".to_string()));
        send(tells, &players, &mut link);
        known.remove(&player);
    }
    for (player, name) in &named {
        known.insert(player, name.0.clone());
    }
}

fn handle_party_requests(
    mut requests: MessageReader<PartyRequest>,
    mut parties: ResMut<Parties>,
    rules: Res<PartyRules>,
    time: Res<Time>,
    names: Query<&DisplayName>,
    players: Query<&Player>,
    ids: Query<(Entity, &NetworkId, &DisplayName), With<Player>>,
    mut link: ResMut<Link>,
) {
    let now = time.elapsed_secs();
    for request in requests.read() {
        let player = request.player;
        if !players.contains(player) {
            continue;
        }
        let name = |entity| name_of(&names, entity);
        let tells = match &request.action {
            PartyAction::Create(party) => parties.create(player, party),
            PartyAction::Invite(target) => match ids.iter().find(|(_, id, _)| id.0 == *target) {
                Some((invitee, ..)) => parties.invite(player, invitee, &rules.0, now, name),
                None => vec![Tell::Notice(player, "You can only invite players to a party.".to_string())],
            },
            PartyAction::InviteNamed(wanted) => {
                match ids.iter().find(|(_, _, name)| name.0.eq_ignore_ascii_case(wanted)) {
                    Some((invitee, ..)) => parties.invite(player, invitee, &rules.0, now, name),
                    None => vec![Tell::Notice(player, format!("There's no player called {wanted} in the game."))],
                }
            }
            PartyAction::Accept => parties.accept(player, &rules.0, name),
            PartyAction::Decline => parties.decline(player, name),
            PartyAction::Leave => parties.leave(player, name),
        };
        send(tells, &players, &mut link);
    }
}

fn expire_invitations(
    time: Res<Time>,
    mut parties: ResMut<Parties>,
    names: Query<&DisplayName>,
    players: Query<&Player>,
    mut link: ResMut<Link>,
) {
    let tells = parties.expire(time.elapsed_secs(), |entity| name_of(&names, entity));
    send(tells, &players, &mut link);
}

/// Sends each party to its members, when anything shown about it has changed.
fn send_party_status(
    mut parties: ResMut<Parties>,
    content: Res<LoadedContent>,
    members: Query<Member>,
    mut link: ResMut<Link>,
) {
    let class_name = |id: &str| {
        content
            .pack
            .classes
            .iter()
            .find(|class| class.id == id)
            .map_or_else(|| id.to_string(), |class| class.name.clone())
    };
    for party in parties.parties.values_mut() {
        let info = PartyInfo {
            name: party.name.clone(),
            members: members
                .iter_many(&party.members)
                .map(|(_, &NetworkId(id), name, character, health, mana)| PartyMember {
                    id,
                    name: name.0.clone(),
                    class: class_name(&character.class),
                    level: character.level(),
                    health: health.current(),
                    max_health: health.max(),
                    mana: mana.current(),
                    max_mana: mana.max(),
                })
                .collect(),
        };
        if party.shown.as_ref() != Some(&info) {
            let message = ServerMessage::Party(Some(info.clone()));
            link.broadcast(members.iter_many(&party.members).map(|(player, ..)| player.connection), &message);
            party.shown = Some(info);
        }
    }
}

/// A player's name (or "Someone", for one who's gone).
fn name_of(names: &Query<&DisplayName>, entity: Entity) -> String {
    names.get(entity).map_or_else(|_| "Someone".to_string(), |name| name.0.clone())
}

/// Sends what [`Parties`] said to tell whom (to those still in the game).
fn send(tells: Vec<Tell>, players: &Query<&Player>, link: &mut Link) {
    for tell in tells {
        let (player, message) = match tell {
            Tell::Notice(player, text) => (player, ServerMessage::Notice { text }),
            Tell::Invitation(player, invitation) => (player, ServerMessage::PartyInvite(invitation)),
            Tell::NoParty(player) => (player, ServerMessage::Party(None)),
        };
        if let Ok(player) = players.get(player) {
            link.send(player.connection, &message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three players: Ada, Bram and Cole.
    fn players() -> (World, [Entity; 3]) {
        let mut world = World::new();
        let entities = [world.spawn_empty().id(), world.spawn_empty().id(), world.spawn_empty().id()];
        (world, entities)
    }

    fn namer(entities: [Entity; 3]) -> impl Fn(Entity) -> String {
        move |entity| ["Ada", "Bram", "Cole"][entities.iter().position(|&e| e == entity).unwrap()].to_string()
    }

    fn notices(tells: &[Tell], to: Entity) -> Vec<&str> {
        tells
            .iter()
            .filter_map(|tell| match tell {
                Tell::Notice(who, text) if *who == to => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_party_is_made_joined_and_left() {
        let (_world, [ada, bram, cole]) = players();
        let name = namer([ada, bram, cole]);
        let rules = PartyConfig::default();
        let mut parties = Parties::default();
        assert_eq!(notices(&parties.create(ada, "Wanderers"), ada), ["You have created the party Wanderers."]);
        assert_eq!(notices(&parties.create(ada, "Again"), ada), ["You are already in a party."]);
        assert_eq!(notices(&parties.create(bram, "wanderers"), bram), ["There's already a party called Wanderers."]);

        let tells = parties.invite(ada, bram, &rules, 0.0, &name);
        assert_eq!(notices(&tells, ada), ["Inviting Bram to party Wanderers..."]);
        assert_eq!(notices(&tells, bram), ["You have been invited to party Wanderers."]);
        assert!(tells.iter().any(|tell| matches!(tell, Tell::Invitation(who, Some(_)) if *who == bram)));
        // One invitation at a time.
        assert_eq!(
            notices(&parties.invite(cole, bram, &rules, 0.0, &name), cole),
            ["You aren't in a party. Start one with /party NAME."]
        );
        parties.create(cole, "Others");
        assert_eq!(
            notices(&parties.invite(cole, bram, &rules, 0.0, &name), cole),
            ["Bram already has a party invitation active."]
        );

        let tells = parties.accept(bram, &rules, &name);
        assert_eq!(notices(&tells, bram), ["You have joined Wanderers."]);
        assert_eq!(parties.members_of(bram), [ada, bram]);
        assert_eq!(notices(&tells, ada), ["Bram accepted the party invite."]);
        assert_eq!(notices(&parties.accept(bram, &rules, &name), bram), ["You don't have a party invitation."]);
        assert_eq!(notices(&parties.invite(ada, bram, &rules, 0.0, &name), ada), ["Bram is already in your party."]);
        assert_eq!(notices(&parties.invite(ada, cole, &rules, 0.0, &name), ada), ["Cole is already in a party."]);

        let tells = parties.leave(bram, &name);
        assert_eq!(notices(&tells, bram), ["You have left Wanderers."]);
        assert_eq!(notices(&tells, ada), ["Bram has left the party."]);
        assert!(tells.contains(&Tell::NoParty(bram)));
        assert_eq!(parties.members_of(ada), [ada]);
        assert_eq!(parties.members_of(bram), [bram], "someone in no party is on their own");
        assert_eq!(notices(&parties.leave(bram, &name), bram), ["You aren't in a party."]);
    }

    #[test]
    fn invitations_are_declined_or_run_out() {
        let (_world, [ada, bram, cole]) = players();
        let name = namer([ada, bram, cole]);
        let rules = PartyConfig::default();
        let mut parties = Parties::default();
        parties.create(ada, "Wanderers");

        parties.invite(ada, bram, &rules, 0.0, &name);
        let tells = parties.decline(bram, &name);
        assert_eq!(notices(&tells, bram), ["You have declined to join Wanderers."]);
        assert_eq!(notices(&tells, ada), ["Bram declined the party invite."]);

        parties.invite(ada, cole, &rules, 10.0, &name);
        assert!(parties.expire(10.0 + rules.invite_seconds - 0.1, &name).is_empty());
        let tells = parties.expire(10.0 + rules.invite_seconds, &name);
        assert_eq!(notices(&tells, cole), ["Your invitation to join Wanderers has expired."]);
        assert_eq!(notices(&tells, ada), ["Your party invite to Cole has expired."]);
        assert_eq!(notices(&parties.accept(cole, &rules, &name), cole), ["You don't have a party invitation."]);
    }

    #[test]
    fn empty_parties_are_gone_with_their_invitations() {
        let (_world, [ada, bram, cole]) = players();
        let name = namer([ada, bram, cole]);
        let rules = PartyConfig { max_members: 2, ..PartyConfig::default() };
        let mut parties = Parties::default();
        parties.create(ada, "Wanderers");
        parties.invite(ada, bram, &rules, 0.0, &name);
        parties.accept(bram, &rules, &name);
        assert_eq!(notices(&parties.invite(ada, cole, &rules, 0.0, &name), ada), ["Your party is full (2 players)."]);

        parties.leave(bram, &name);
        parties.invite(ada, cole, &rules, 0.0, &name);
        // Ada leaves the game: the party is empty, so it's gone, and Cole's
        // invitation with it.
        let tells = parties.forget(ada, &name);
        assert_eq!(notices(&tells, cole), ["Wanderers has disbanded, so your invitation is void."]);
        assert!(tells.contains(&Tell::Invitation(cole, None)));
        assert_eq!(notices(&parties.create(cole, "Wanderers"), cole), ["You have created the party Wanderers."]);
    }
}
