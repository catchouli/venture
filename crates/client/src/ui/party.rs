//! The player's party, as the server last said ([`ServerMessage::Party`]),
//! and their party invitation ([`ServerMessage::PartyInvite`]), shown in two
//! layouts under the player frame:
//!
//! - `party_frame`: a box for each other member, with their level, name,
//!   class, health and mana (a copy of its `member` template each);
//! - `party_invite`: who's inviting them to which party, the time left to
//!   answer, and Accept and Decline buttons.
//!
//! The buttons are just `/accept` and `/decline` (see
//! [`crate::interaction::actions`]); the server decides everything. The
//! party tab of the character window shows the party too (see
//! [`crate::ui::character_window::party`]).

use bevy::prelude::*;
use venture_protocol::{PartyInfo, PartyInvite, ServerMessage};

use crate::characters::player::LocalPlayer;
use crate::interaction::actions::{Action, PerformAction};
use crate::network::{ClientState, FromServer};
use crate::ui::layout::{LayoutBuilt, LayoutSystems, Ui, UiData, UiId, UiRoot, scoped_name};

/// The party members' layout.
const FRAME: &str = "party_frame";
/// The invitation's layout.
const INVITATION: &str = "party_invite";

/// Keeps track of the party, and shows it.
pub struct PartyPlugin;

impl Plugin for PartyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Party>()
            .add_systems(
                Update,
                (record_party, publish_data, show_members, show_windows, answer_invitation)
                    .chain()
                    .after(LayoutSystems),
            )
            .add_systems(OnExit(ClientState::Playing), |mut party: ResMut<Party>| *party = Party::default());
    }
}

/// The player's party and invitation.
#[derive(Resource, Default)]
pub struct Party {
    /// Their party, if they're in one.
    pub info: Option<PartyInfo>,
    /// Their invitation, if they have one, and when it arrived (in the
    /// client's seconds).
    pub invitation: Option<(PartyInvite, f32)>,
}

impl Party {
    /// Seconds left to answer the invitation (0 when it's run out), if there is one.
    fn seconds_left(&self, now: f32) -> Option<f32> {
        self.invitation.as_ref().map(|(invite, arrived)| (invite.seconds - (now - arrived)).max(0.0))
    }
}

/// A copy of the party frame's `member` template, for one member.
#[derive(Component)]
struct MemberFrame;

fn record_party(time: Res<Time>, mut events: MessageReader<FromServer>, mut party: ResMut<Party>) {
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::Party(info) => party.info = info.clone(),
            ServerMessage::PartyInvite(invite) => {
                party.invitation = invite.clone().map(|invite| (invite, time.elapsed_secs()));
            }
            _ => {}
        }
    }
}

/// Gives the layouts the party's and invitation's data (each member's in
/// their own scope: their entity id).
fn publish_data(time: Res<Time>, party: Res<Party>, mut data: ResMut<UiData>) {
    if let Some(info) = &party.info {
        for member in &info.members {
            let scope = member.id.0.to_string();
            let mut set = |name: &str, value: String| UiData::set(&mut data, &scoped_name(name, &scope), value);
            set("member_name", member.name.clone());
            set("member_class", member.class.clone());
            set("member_level", member.level.to_string());
            set("member_health", member.health.to_string());
            set("member_health_max", member.max_health.to_string());
            set("member_mana", member.mana.to_string());
            set("member_mana_max", member.max_mana.to_string());
        }
    }
    if let (Some((invite, _)), Some(left)) = (&party.invitation, party.seconds_left(time.elapsed_secs())) {
        UiData::set(&mut data, "invite_from", &invite.from);
        UiData::set(&mut data, "invite_party", &invite.party);
        UiData::set(&mut data, "invite_seconds", left.ceil());
        // (Rounded, so the bar isn't marked changed every frame for nothing.)
        UiData::set(&mut data, "invite_time", format!("{:.2}", left));
        UiData::set(&mut data, "invite_time_max", invite.seconds);
    }
}

/// Makes a box for each other member whenever who's in the party changes
/// (or the layout is rebuilt).
fn show_members(
    mut built: MessageReader<LayoutBuilt>,
    party: Res<Party>,
    me: Option<Single<&LocalPlayer>>,
    frames: Query<Entity, With<MemberFrame>>,
    mut shown: Local<Vec<String>>,
    mut ui: Ui,
) {
    let rebuilt = built.read().any(|built| built.layout == FRAME);
    let me = me.map(|me| me.id);
    let others: Vec<String> = party
        .info
        .iter()
        .flat_map(|info| &info.members)
        .filter(|member| Some(member.id) != me)
        .map(|member| member.id.0.to_string())
        .collect();
    if !rebuilt && *shown == others {
        return;
    }
    for frame in &frames {
        ui.commands().entity(frame).despawn();
    }
    for copy in ui.copy_scoped(FRAME, "member", &others) {
        ui.commands().entity(copy.root).insert(MemberFrame);
    }
    *shown = others;
}

/// Shows the party frame while in a party with anyone else, and the
/// invitation while there's time to answer it.
fn show_windows(
    time: Res<Time>,
    state: Res<State<ClientState>>,
    party: Res<Party>,
    members: Query<(), With<MemberFrame>>,
    mut roots: Query<(&UiRoot, &mut Visibility)>,
) {
    let playing = *state.get() == ClientState::Playing;
    let invited = party.seconds_left(time.elapsed_secs()).is_some_and(|left| left > 0.0);
    for (root, mut visibility) in &mut roots {
        let show = match root.0.as_str() {
            FRAME => playing && !members.is_empty(),
            INVITATION => playing && invited,
            _ => continue,
        };
        let wanted = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// The invitation's buttons: `/accept` and `/decline`.
fn answer_invitation(
    buttons: Query<(&UiId, &Interaction), Changed<Interaction>>,
    mut actions: MessageWriter<PerformAction>,
) {
    for (id, interaction) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match id.0.as_str() {
            "invite_accept" => {
                actions.write(PerformAction::new(Action::Accept));
            }
            "invite_decline" => {
                actions.write(PerformAction::new(Action::Decline));
            }
            _ => {}
        }
    }
}
