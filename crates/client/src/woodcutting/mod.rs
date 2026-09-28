//! Woodcutting, as the player does it (the server decides everything: see
//! its `woodcutting.rs`, and `docs/plan-trees.md`).
//!
//! - Clicking a selected tree (standing or fallen) with an axe in hand
//!   ([`ChopClicked`], from [`crate::interaction::clicks`]) runs the player
//!   beside its trunk, then asks the server to chop it: to fell a standing
//!   tree, or cut a fallen one up for logs.
//! - Every swing of an axe nearby ([`ServerMessage::Chopped`]) swings the
//!   chopper's arms, turned to face what they chop.
//!
//! Trees themselves (standing, felled, fallen and tipping over) are drawn by
//! [`crate::world::trees`].

use bevy::prelude::*;
use venture_content::WoodworkingPart;
use venture_protocol::{ClientMessage, EquipSlot, ServerMessage, WoodcuttingCommand};

use crate::characters::model::Gait;
use crate::characters::player::LocalPlayer;
use crate::common::coords::from_bevy;
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::interaction::orders::Orders;
use crate::interaction::targeting::{Level, Target, Targetable, nearest};
use crate::network::ToServer;
use crate::network::{ClientState, FromServer};
use crate::ui::character_window::Sheet;
use crate::ui::character_window::inventory::PlayerInventory;
use crate::ui::chat::SystemMessage;
use crate::world::streaming::StreamingProgress;
use crate::world::trees::{Dressed, Fallen, StandingTree};

/// How near a trunk the player goes to chop it, in metres (a little less
/// than the server's reach, in `server.ron`).
const CHOP_REACH: f32 = 2.2;
/// How much nearer than that the player aims, so they stop within reach.
const STOP_SHORT: f32 = 0.6;

/// Woodcutting's client side.
pub struct WoodcuttingPlugin;

impl Plugin for WoodcuttingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ChopClicked>().init_resource::<PendingChop>().add_systems(
            Update,
            (chop_on_startup_if_requested, start_chopping, chop_once_there, swing_axes)
                .chain()
                .run_if(in_state(ClientState::Playing)),
        );
    }
}

/// The player clicked a selected tree, standing or fallen, to chop it.
#[derive(Message)]
pub struct ChopClicked(pub Entity);

/// A chop waiting for the player to reach the trunk: what to ask the
/// server, and the trunk (a segment on the ground: base and top, in metres).
#[derive(Resource, Default)]
struct PendingChop(Option<(WoodcuttingCommand, (f32, f32), (f32, f32))>);

/// The power and level of the axe in the player's hand, if they hold one.
pub fn axe_in_hand(sheet: &Sheet, content: &ContentStore) -> Option<(f32, u32)> {
    let sheet = sheet.0.as_ref()?;
    let (_, item) = sheet.equipment.iter().find(|(slot, _)| *slot == EquipSlot::DominantHand)?;
    match content.items.get(item)?.woodworking? {
        WoodworkingPart::Axe { power, level } => Some((power, level)),
    }
}

/// For development (`VENTURE_CHOP`): once loaded, takes an axe in hand,
/// selects the nearest tree the player's level can fell, and fells it.
fn chop_on_startup_if_requested(
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    inventory: Res<PlayerInventory>,
    player: Option<Single<&GlobalTransform, With<LocalPlayer>>>,
    trees: Query<(Entity, &GlobalTransform, &Level), With<StandingTree>>,
    mut target: ResMut<Target>,
    mut to_server: MessageWriter<ToServer>,
    mut chops: MessageWriter<ChopClicked>,
    mut stage: Local<u8>,
) {
    if !dev.chop || *stage >= 2 || !progress.measured || progress.waiting > 0 {
        return;
    }
    let Some(player) = player else { return };
    if axe_in_hand(&sheet, &content).is_none() {
        if *stage == 0 {
            let is_axe = |item: &str| content.items.get(item).is_some_and(|def| def.woodworking.is_some());
            let Some(slot) = inventory.slots.iter().position(|slot| slot.as_ref().is_some_and(|s| is_axe(&s.item)))
            else {
                return;
            };
            to_server.write(ToServer(ClientMessage::Equip { slot: slot as u32 }));
            *stage = 1;
        }
        return;
    }
    // The nearest the player's woodworking level can fell.
    let skill = sheet.0.as_ref().and_then(|sheet| sheet.skills.iter().find(|progress| progress.skill == "woodworking"));
    let level = skill.map_or(1, |progress| progress.level);
    let fellable = trees.iter().filter(|(.., tree_level)| tree_level.0 <= level);
    let Some((tree, _)) =
        nearest(player.translation(), fellable.map(|(entity, place, _)| (entity, place.translation())))
    else {
        return;
    };
    target.0 = Some(tree);
    chops.write(ChopClicked(tree));
    *stage = 2;
}

/// How far a point is from a trunk lying from `base` to `top`, and the
/// nearest point on it.
fn from_trunk(point: (f32, f32), base: (f32, f32), top: (f32, f32)) -> (f32, (f32, f32)) {
    let (ax, ay) = (top.0 - base.0, top.1 - base.1);
    let length_squared = (ax * ax + ay * ay).max(1e-6);
    let along = (((point.0 - base.0) * ax + (point.1 - base.1) * ay) / length_squared).clamp(0.0, 1.0);
    let nearest = (base.0 + ax * along, base.1 + ay * along);
    ((point.0 - nearest.0).hypot(point.1 - nearest.1), nearest)
}

fn start_chopping(
    mut commands: Commands,
    mut clicks: MessageReader<ChopClicked>,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    standing: Query<(&StandingTree, &GlobalTransform)>,
    fallen: Query<(&Fallen, &Targetable, &GlobalTransform, Option<&Dressed>)>,
    mut orders: Orders,
    mut pending: ResMut<PendingChop>,
    mut system: MessageWriter<SystemMessage>,
) {
    let Some(ChopClicked(clicked)) = clicks.read().last() else { return };
    if axe_in_hand(&sheet, &content).is_none() {
        system.write(SystemMessage("Take an axe in your hand to chop wood.".to_string()));
        return;
    }
    let (command, base, top) = if let Ok((&StandingTree(tree), place)) = standing.get(*clicked) {
        let at = from_bevy(place.translation());
        (WoodcuttingCommand::Fell { tree }, at, at)
    } else if let Ok((fallen_tree, targetable, place, dressed)) = fallen.get(*clicked) {
        let base = from_bevy(place.translation());
        let length = dressed.map_or(venture_rpg::woodcutting::FALLEN_LENGTHS[1], |dressed| dressed.0);
        let top = (base.0 + fallen_tree.toward.sin() * length, base.1 + fallen_tree.toward.cos() * length);
        (WoodcuttingCommand::Cut { fallen: targetable.id }, base, top)
    } else {
        return;
    };
    let Some(player) = orders.position().map(from_bevy) else { return };
    let (distance, nearest) = from_trunk(player, base, top);
    if distance <= CHOP_REACH {
        orders.ask(ClientMessage::Stop);
        orders.ask(ClientMessage::Woodcutting(command));
        pending.0 = None;
        return;
    }
    // Run to beside the trunk (the side the player's on), then chop.
    let (dx, dy) = ((player.0 - nearest.0) / distance, (player.1 - nearest.1) / distance);
    let beside = (nearest.0 + dx * (CHOP_REACH - STOP_SHORT), nearest.1 + dy * (CHOP_REACH - STOP_SHORT));
    orders.move_to(&mut commands, beside);
    pending.0 = Some((command, base, top));
}

/// Chops once the player has run to the trunk; gives up if they stop elsewhere.
fn chop_once_there(mut pending: ResMut<PendingChop>, mut orders: Orders) {
    let Some((command, base, top)) = pending.0 else { return };
    let Some(player) = orders.position() else { return };
    let (distance, _) = from_trunk(from_bevy(player), base, top);
    if distance <= CHOP_REACH {
        orders.ask(ClientMessage::Stop);
        orders.ask(ClientMessage::Woodcutting(command));
        pending.0 = None;
    } else if !orders.is_moving() {
        pending.0 = None;
    }
}

/// Swings the arms of whoever chopped, turned to face what they chop.
fn swing_axes(
    mut events: MessageReader<FromServer>,
    mut me: Option<Single<(&LocalPlayer, &mut Gait, &mut Transform)>>,
    mut others: Query<(&Targetable, &mut Gait, &mut Transform), Without<LocalPlayer>>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Chopped { id, at, .. } = message else { continue };
        if let Some(me) = me.as_mut()
            && me.0.id == *id
        {
            let (_, gait, transform) = &mut **me;
            swing_at(gait, transform, *at);
        } else if let Some((_, mut gait, mut transform)) =
            others.iter_mut().find(|(targetable, ..)| targetable.id == *id)
        {
            swing_at(&mut gait, &mut transform, *at);
        }
    }
}

/// Swings a character's arms, turned to face `at`.
fn swing_at(gait: &mut Gait, transform: &mut Transform, at: (f32, f32)) {
    gait.swing();
    // (Characters face +Z in their own space, which is the world's +y.)
    let (x, y) = from_bevy(transform.translation);
    let (dx, dy) = (at.0 - x, at.1 - y);
    if dx.hypot(dy) > 0.05 {
        transform.rotation = Quat::from_rotation_y(dx.atan2(dy));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_to_a_lying_trunk_is_to_its_nearest_point() {
        let (distance, nearest) = from_trunk((5.0, 2.0), (0.0, 0.0), (10.0, 0.0));
        assert!((distance - 2.0).abs() < 1e-5 && (nearest.0 - 5.0).abs() < 1e-5);
        // Past its top, from the top.
        let (distance, _) = from_trunk((13.0, 4.0), (0.0, 0.0), (10.0, 0.0));
        assert!((distance - 5.0).abs() < 1e-5);
        // A standing trunk is a point.
        let (distance, _) = from_trunk((3.0, 4.0), (0.0, 0.0), (0.0, 0.0));
        assert!((distance - 5.0).abs() < 1e-5);
    }
}
