//! Fishing, as the player sees and plays it (the server decides everything:
//! see `docs/plan-fishing.md` and the server's `fishing.rs`).
//!
//! With a fishing rod in hand, clicking water casts there (with the bait or
//! lure last used, or the first in the bag that suits the rod). Then:
//! - a float sits where the line went in, bobbing, and dipping at a nibble;
//! - when a fish bites (ledger and float), a click or Space strikes;
//! - once one's hooked, holding the mouse button or Space reels, and
//!   letting go lets it run; the float comes in as the line does.
//!
//! [`shadows`] shows the fish in the water, [`traps`] handles traps, and
//! [`panel`] shows how it's going: what's happening, and in a fight the
//! line's tension and how much is out.

pub mod others;
pub mod panel;
pub mod ripples;
pub mod shadows;
pub mod traps;

use bevy::prelude::*;
use venture_content::FishingPart;
use venture_navigation::Point;
use venture_protocol::{ClientMessage, EquipSlot, FishingCommand, FishingState, ServerMessage};
use venture_rpg::fishing::Method;
use venture_simulation::TICK_RATE;
use venture_simulation::fishing::{Fight, reeling_at};

use crate::characters::model::Gait;
use crate::characters::model::{ROD_ANGLE, RodGrip, RodTip};
use crate::characters::player::LocalPlayer;
use crate::common::coords::{from_bevy, to_bevy};
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::interaction::orders::Orders;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::character_window::Sheet;
use crate::ui::character_window::inventory::PlayerInventory;
use crate::ui::chat::SystemMessage;
use crate::ui::text_input::KeyboardFocus;
use crate::world::query::water_level;
use crate::world::streaming::{ChunkStore, StreamingProgress};

/// Radius of the float, in metres.
const FLOAT_RADIUS: f32 = 0.12;
/// How far a float bobs, in metres.
const BOB: f32 = 0.03;
/// How far it dips at a nibble, and at a bite, in metres.
const NIBBLE_DIP: f32 = 0.08;
const BITE_DIP: f32 = 0.25;
/// How long a nibble's dip lasts, in seconds.
const NIBBLE_SECONDS: f32 = 0.5;
/// About where a held rod's tip is, from the player's feet (how high, and
/// how far in front, in metres), for before the rod is drawn.
const ROD_TIP_HEIGHT: f32 = 2.4;
const ROD_TIP_REACH: f32 = 1.0;
/// How quickly the player turns to face their line (per second).
const TURN_RATE: f32 = 8.0;
/// The line's thickness, in metres.
const LINE_RADIUS: f32 = 0.006;

/// Fishing.
pub struct FishingPlugin;

impl Plugin for FishingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Angling>()
            .add_message::<CastAt>()
            .add_plugins((
                panel::PanelPlugin,
                ripples::RipplesPlugin,
                shadows::ShadowsPlugin,
                traps::TrapsPlugin,
                others::OthersPlugin,
            ))
            .add_systems(Startup, make_float)
            .add_systems(
                Update,
                (
                    follow_server,
                    fish_on_startup_if_requested,
                    cast_where_clicked,
                    strike_and_reel,
                    predict_fight,
                    face_the_line,
                    show_float_and_line,
                    bend_the_rod,
                )
                    .chain()
                    .run_if(in_state(ClientState::Playing)),
            );
    }
}

/// The player's fishing, as the server last said.
#[derive(Resource, Default)]
pub struct Angling {
    /// Their line, if it's in the water.
    pub line: Option<LineView>,
    /// The bait or lure they last cast with.
    last_tackle: Option<String>,
    /// Whether they're reeling (as last told to the server).
    reeling: bool,
    /// The fight, run ahead of the server (see [`predict_fight`]).
    prediction: Option<Prediction>,
}

/// A fight run ahead of the server, from the player's own reeling, so
/// reeling shows at once. Whenever the server says how the fight stands,
/// the player's changes of reeling are replayed on top of that.
struct Prediction {
    /// The fight as predicted.
    fight: Fight,
    /// The player's changes of reeling this fight: (the tick from, reeling).
    changes: Vec<(u32, bool)>,
    /// Ticks of time not yet stepped.
    clock: f32,
    /// How the fight stood when the server last said (to replay from).
    server: Option<Fight>,
    /// Whether the prediction has ended the fight (it waits for the server).
    ended: bool,
}

/// How many ticks ahead of the server's last word the fight is run when it
/// starts (about the time a message takes to arrive).
const LEAD_TICKS: u32 = 2;

/// A line in the water.
pub struct LineView {
    /// Where it went in.
    pub at: Point,
    /// How they're fishing.
    pub method: Method,
    /// What's happening.
    pub stage: Stage,
    /// When the last nibble came (seconds of game time), if any.
    pub nibbled: Option<f32>,
}

/// What's happening on a line.
#[derive(Clone, Copy, PartialEq)]
pub enum Stage {
    /// Waiting for a bite.
    Waiting,
    /// A fish is biting: strike!
    Biting,
    /// A fish is hooked.
    Fighting(Fight),
}

impl Angling {
    /// Starts or stops reeling, from the next tick of the predicted fight,
    /// and tells the server which tick that is.
    fn set_reeling(&mut self, reeling: bool, to_server: &mut MessageWriter<ToServer>) {
        // (Until the fight's first word from the server, there's no tick to
        // count from: this is tried again next frame.)
        let Some(prediction) = &mut self.prediction else { return };
        self.reeling = reeling;
        let tick = prediction.fight.ticks + 1;
        prediction.changes.retain(|&(from, _)| from < tick);
        prediction.changes.push((tick, reeling));
        to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::Reel { reeling, tick })));
    }

    /// Whether a fish is biting or hooked, so clicks strike and reel rather
    /// than doing their usual things.
    pub fn busy(&self) -> bool {
        self.line.as_ref().is_some_and(|line| line.stage != Stage::Waiting)
    }
}

/// A click on water, where the player might cast (sent by the clicks module).
#[derive(Message)]
pub struct CastAt(pub Point);

/// The method of the fishing rod in the player's hand, if they hold one.
pub fn rod_in_hand(sheet: &Sheet, content: &ContentStore) -> Option<Method> {
    let sheet = sheet.0.as_ref()?;
    let (_, item) = sheet.equipment.iter().find(|(slot, _)| *slot == EquipSlot::DominantHand)?;
    match content.items.get(item)?.fishing.as_ref()? {
        FishingPart::Rod { method, .. } => Some(*method),
        _ => None,
    }
}

fn follow_server(
    mut events: MessageReader<FromServer>,
    time: Res<Time>,
    mut angling: ResMut<Angling>,
    mut gait: Option<Single<&mut Gait, With<LocalPlayer>>>,
) {
    for FromServer(message) in events.read() {
        let ServerMessage::Fishing(state) = message else { continue };
        match state {
            FishingState::Waiting { at, method } => {
                angling.line = Some(LineView { at: *at, method: *method, stage: Stage::Waiting, nibbled: None });
                angling.reeling = false;
                angling.prediction = None;
                // The cast: a swing of the rod.
                if let Some(gait) = &mut gait {
                    gait.swing();
                }
            }
            FishingState::Nibble => {
                if let Some(line) = &mut angling.line {
                    line.nibbled = Some(time.elapsed_secs());
                }
            }
            FishingState::Bite => {
                if let Some(line) = &mut angling.line {
                    line.stage = Stage::Biting;
                }
            }
            FishingState::Fight(fight) => {
                match &mut angling.prediction {
                    Some(prediction) => prediction.server = Some(*fight),
                    None => {
                        angling.prediction = Some(Prediction {
                            fight: *fight,
                            changes: Vec::new(),
                            clock: LEAD_TICKS as f32,
                            server: None,
                            ended: false,
                        });
                    }
                }
                if let Some(line) = &mut angling.line {
                    line.stage = Stage::Fighting(*fight);
                }
            }
            FishingState::Caught { .. } => {}
            FishingState::Ended => {
                angling.line = None;
                angling.reeling = false;
                angling.prediction = None;
            }
        }
    }
}

/// Casts where the player clicked water, with bait or a lure that suits the rod.
fn cast_where_clicked(
    mut clicks: MessageReader<CastAt>,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    inventory: Res<PlayerInventory>,
    mut angling: ResMut<Angling>,
    mut orders: Orders,
    mut system: MessageWriter<SystemMessage>,
) {
    let Some(&CastAt(at)) = clicks.read().last() else { return };
    let Some(method) = rod_in_hand(&sheet, &content) else { return };
    let suits = |item: &str| match content.items.get(item).and_then(|def| def.fishing.as_ref()) {
        Some(FishingPart::Lure(_)) => method == Method::Lure,
        Some(FishingPart::Bait(_)) => method != Method::Lure,
        _ => false,
    };
    let carried: Vec<&str> = inventory.slots.iter().flatten().map(|stack| stack.item.as_str()).collect();
    let tackle = angling
        .last_tackle
        .as_deref()
        .filter(|item| carried.contains(item) && suits(item))
        .or_else(|| carried.iter().copied().find(|item| suits(item)));
    let Some(tackle) = tackle.map(str::to_string) else {
        let needed = if method == Method::Lure { "a lure" } else { "bait" };
        system.write(SystemMessage(format!("You need {needed} to fish with this rod.")));
        return;
    };
    orders.cast(at, &tackle);
    angling.last_tackle = Some(tackle);
}

/// For development (`VENTURE_FISH`): once the world has loaded, takes a rod
/// in hand and casts; with `VENTURE_FISH_AUTO`, strikes and reels too.
fn fish_on_startup_if_requested(
    time: Res<Time>,
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    inventory: Res<PlayerInventory>,
    mut angling: ResMut<Angling>,
    mut casts: MessageWriter<CastAt>,
    mut to_server: MessageWriter<ToServer>,
    mut asked: Local<Option<f32>>,
) {
    let Some(at) = dev.fish_at else { return };
    if !progress.measured || progress.waiting > 0 {
        return;
    }
    let now = time.elapsed_secs();
    if asked.is_none() {
        if rod_in_hand(&sheet, &content).is_none() {
            let is_rod = |item: &str| {
                matches!(content.items.get(item).and_then(|def| def.fishing.as_ref()), Some(FishingPart::Rod { .. }))
            };
            let Some(slot) = inventory.slots.iter().position(|slot| slot.as_ref().is_some_and(|s| is_rod(&s.item)))
            else {
                return;
            };
            to_server.write(ToServer(ClientMessage::Equip { slot: slot as u32 }));
        }
        *asked = Some(now);
        return;
    }
    // Cast once the rod's in hand (a moment after asking), and then only once.
    if asked.is_some_and(|since| since >= 0.0) && rod_in_hand(&sheet, &content).is_some() {
        casts.write(CastAt(at));
        *asked = Some(-1.0);
    }
    if !dev.fish_auto {
        return;
    }
    let Some(line) = &angling.line else { return };
    match line.stage {
        Stage::Biting => {
            to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::Strike)));
        }
        Stage::Fighting(fight) if angling.reeling == fight.running => {
            let reeling = !fight.running;
            angling.set_reeling(reeling, &mut to_server);
        }
        _ => {}
    }
}

/// Strikes at a bite, and reels while the mouse button or Space is held in a fight.
fn strike_and_reel(
    dev: Res<DevSettings>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    ui: Query<&Interaction>,
    mut angling: ResMut<Angling>,
    mut to_server: MessageWriter<ToServer>,
) {
    let Some(line) = &angling.line else { return };
    if dev.fish_auto {
        return;
    }
    let over_ui = ui.iter().any(|interaction| *interaction != Interaction::None);
    let space = !focus.is_typing();
    let pressed = (buttons.just_pressed(MouseButton::Left) && !over_ui) || (space && keys.just_pressed(KeyCode::Space));
    let held = (buttons.pressed(MouseButton::Left) && !over_ui) || (space && keys.pressed(KeyCode::Space));
    match line.stage {
        Stage::Biting if pressed => {
            to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::Strike)));
        }
        Stage::Fighting(_) if held != angling.reeling => {
            angling.set_reeling(held, &mut to_server);
        }
        _ => {}
    }
}

/// Runs the fight ahead of the server at the server's pace, from the
/// player's reeling, replaying their changes on top of what the server last
/// said; what's shown is the prediction.
fn predict_fight(time: Res<Time>, mut angling: ResMut<Angling>) {
    let Angling { line, prediction, .. } = &mut *angling;
    let (Some(line), Some(prediction)) = (line, prediction) else { return };
    if let Some(server) = prediction.server.take() {
        // Replay from the server's word up to where the prediction had got.
        let target = prediction.fight.ticks.max(server.ticks);
        let mut fight = server;
        prediction.ended = false;
        while fight.ticks < target {
            if fight.step(reeling_at(&prediction.changes, false, fight.ticks + 1)).is_some() {
                prediction.ended = true;
                break;
            }
        }
        prediction.fight = fight;
    }
    prediction.clock += time.delta_secs() * TICK_RATE;
    while prediction.clock >= 1.0 {
        prediction.clock -= 1.0;
        if prediction.ended {
            continue;
        }
        let reeling = reeling_at(&prediction.changes, false, prediction.fight.ticks + 1);
        prediction.ended = prediction.fight.step(reeling).is_some();
    }
    line.stage = Stage::Fighting(prediction.fight);
}

/// Turns the player to face their line (while standing: moving ends fishing).
fn face_the_line(time: Res<Time>, angling: Res<Angling>, player: Option<Single<&mut Transform, With<LocalPlayer>>>) {
    let (Some(line), Some(mut player)) = (&angling.line, player) else { return };
    let (x, y) = from_bevy(player.translation);
    let (dx, dy) = (line.at.0 - x, line.at.1 - y);
    if dx.hypot(dy) < 0.1 {
        return;
    }
    // (Characters face +Z in their own space, which is world +y.)
    let facing = Quat::from_rotation_y(dx.atan2(dy));
    player.rotation = player.rotation.slerp(facing, (TURN_RATE * time.delta_secs()).min(1.0));
}

/// Bends the player's rod: a twitch at a nibble, a pull down at a bite, and
/// in a fight as much as the line's tension (so a ledger's bites, which
/// have no float, show on the rod's tip).
fn bend_the_rod(
    time: Res<Time>,
    angling: Res<Angling>,
    player: Option<Single<&GlobalTransform, With<LocalPlayer>>>,
    mut grips: Query<(&GlobalTransform, &mut Transform), With<RodGrip>>,
) {
    let Some(player) = player else { return };
    let now = time.elapsed_secs();
    let bend = match angling.line.as_ref().map(|line| (line.stage, line.nibbled)) {
        Some((Stage::Biting, _)) => 0.35 + (now * 30.0).sin() * 0.05,
        Some((Stage::Fighting(fight), _)) => {
            let strain = (fight.tension / fight.setup.strength.max(0.01)).min(1.2);
            0.45 * strain + (now * 23.0).sin() * 0.02 * strain
        }
        Some((Stage::Waiting, Some(at))) if now - at < 0.4 => (now * 40.0).sin().abs() * 0.08,
        _ => 0.0,
    };
    // The player's own rod: the one held nearest them.
    let here = player.translation();
    let Some((_, mut grip)) = grips
        .iter_mut()
        .filter(|(place, _)| place.translation().distance(here) < 2.5)
        .min_by(|a, b| a.0.translation().distance(here).total_cmp(&b.0.translation().distance(here)))
    else {
        return;
    };
    grip.rotation = Quat::from_rotation_x(ROD_ANGLE + bend);
}

/// The float, and the line from the rod to it.
#[derive(Component)]
struct Float;

#[derive(Component)]
struct FishingLine;

fn make_float(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let float = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.12, 0.08),
        perceptual_roughness: 0.5,
        ..default()
    });
    let line = materials.add(StandardMaterial { base_color: Color::srgb(0.9, 0.9, 0.88), unlit: true, ..default() });
    let assets = LineAssets {
        float_mesh: meshes.add(Sphere::new(FLOAT_RADIUS).mesh().uv(16, 8)),
        float,
        // A unit-long cylinder, stretched between the rod tip and the float.
        line_mesh: meshes.add(Cylinder::new(LINE_RADIUS, 1.0).mesh().resolution(4)),
        line,
    };
    let (float, line) = assets.spawn(&mut commands);
    commands.entity(float).insert(Float);
    commands.entity(line).insert(FishingLine);
    commands.insert_resource(assets);
}

/// The float's and line's meshes and materials.
#[derive(Resource)]
pub struct LineAssets {
    float_mesh: Handle<Mesh>,
    float: Handle<StandardMaterial>,
    line_mesh: Handle<Mesh>,
    line: Handle<StandardMaterial>,
}

impl LineAssets {
    /// A float and a line (hidden until placed).
    pub fn spawn(&self, commands: &mut Commands) -> (Entity, Entity) {
        let float = commands
            .spawn((
                Mesh3d(self.float_mesh.clone()),
                MeshMaterial3d(self.float.clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        let line = commands
            .spawn((
                Mesh3d(self.line_mesh.clone()),
                MeshMaterial3d(self.line.clone()),
                // (A shadow of a line this thin would only be a dotted streak.)
                bevy::light::NotShadowCaster,
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        (float, line)
    }
}

/// Where the tip of the rod held by a character standing at `feet` (facing
/// `rotation`) is: the nearest rod tip drawn, or if there's none (the rod
/// isn't drawn yet), about where it would be.
pub fn rod_tip(tips: &Query<&GlobalTransform, With<RodTip>>, feet: Vec3, rotation: Quat) -> Vec3 {
    let guess = feet + Vec3::Y * ROD_TIP_HEIGHT + rotation * Vec3::Z * ROD_TIP_REACH;
    tips.iter()
        .map(GlobalTransform::translation)
        .filter(|tip| tip.distance(guess) < 3.0)
        .min_by(|a, b| a.distance(guess).total_cmp(&b.distance(guess)))
        .unwrap_or(guess)
}

/// Where a line from a rod's tip to a float runs: the line's transform (a
/// unit cylinder stretched from one to the other).
pub fn line_between(tip: Vec3, float: Vec3) -> Transform {
    let span = float - tip;
    Transform {
        translation: tip + span / 2.0,
        rotation: Quat::from_rotation_arc(Vec3::Y, span.normalize_or(Vec3::Y)),
        scale: Vec3::new(1.0, span.length(), 1.0),
    }
}

/// Places the float where the line is (coming in as a fish is reeled), and
/// the line to it.
fn show_float_and_line(
    time: Res<Time>,
    angling: Res<Angling>,
    store: Res<ChunkStore>,
    player: Option<Single<&Transform, (With<LocalPlayer>, Without<Float>, Without<FishingLine>)>>,
    mut float: Single<(&mut Transform, &mut Visibility), (With<Float>, Without<FishingLine>)>,
    mut line_mesh: Single<(&mut Transform, &mut Visibility), (With<FishingLine>, Without<Float>)>,
    tips: Query<&GlobalTransform, With<RodTip>>,
) {
    let (Some(line), Some(player)) = (&angling.line, player) else {
        *float.1 = Visibility::Hidden;
        *line_mesh.1 = Visibility::Hidden;
        return;
    };
    let now = time.elapsed_secs();
    let standing = player.translation;
    let (px, py) = from_bevy(standing);
    let (ax, ay) = line.at;
    // Where the float is: where it went in, or as far out as the line is in a fight.
    let (x, y) = match line.stage {
        Stage::Fighting(fight) => {
            let (dx, dy) = (ax - px, ay - py);
            let length = dx.hypot(dy).max(0.01);
            let out = fight.distance;
            (px + dx / length * out, py + dy / length * out)
        }
        _ => (ax, ay),
    };
    let surface = water_level(&store, x, y).unwrap_or(standing.y);
    let dip = match line.stage {
        Stage::Biting => BITE_DIP,
        Stage::Fighting(_) => BITE_DIP * 0.5,
        Stage::Waiting => line.nibbled.filter(|&at| now - at < NIBBLE_SECONDS).map_or(0.0, |_| NIBBLE_DIP),
    };
    let bob = (now * 2.4).sin() * BOB;
    let at = to_bevy(x, y, surface + bob - dip);
    *float.0 = Transform::from_translation(at);
    // A ledger's weight holds the bait on the bottom: no float (bites show
    // on the rod's tip).
    *float.1 = if line.method == Method::Ledger { Visibility::Hidden } else { Visibility::Inherited };

    // The line: from the rod's tip, up and in front of the player, to the float.
    *line_mesh.0 = line_between(rod_tip(&tips, standing, player.rotation), at);
    *line_mesh.1 = Visibility::Inherited;
}
