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

pub mod panel;
pub mod shadows;
pub mod traps;

use bevy::prelude::*;
use venture_content::FishingPart;
use venture_navigation::Point;
use venture_protocol::{ClientMessage, EquipSlot, FishingCommand, FishingState, ServerMessage};
use venture_rpg::fishing::Method;
use venture_simulation::fishing::Fight;

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
/// How high above the ground the line leaves the rod, in metres.
const ROD_TIP_HEIGHT: f32 = 1.9;
/// The line's thickness, in metres.
const LINE_RADIUS: f32 = 0.006;

/// Fishing.
pub struct FishingPlugin;

impl Plugin for FishingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Angling>()
            .add_message::<CastAt>()
            .add_plugins((panel::PanelPlugin, shadows::ShadowsPlugin, traps::TrapsPlugin))
            .add_systems(Startup, make_float)
            .add_systems(
                Update,
                (follow_server, fish_on_startup_if_requested, cast_where_clicked, strike_and_reel, show_float_and_line)
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
}

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

fn follow_server(mut events: MessageReader<FromServer>, time: Res<Time>, mut angling: ResMut<Angling>) {
    for FromServer(message) in events.read() {
        let ServerMessage::Fishing(state) = message else { continue };
        match state {
            FishingState::Waiting { at, method } => {
                angling.line = Some(LineView { at: *at, method: *method, stage: Stage::Waiting, nibbled: None });
                angling.reeling = false;
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
                if let Some(line) = &mut angling.line {
                    line.stage = Stage::Fighting(*fight);
                }
            }
            FishingState::Caught { .. } => {}
            FishingState::Ended => {
                angling.line = None;
                angling.reeling = false;
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
            angling.reeling = !fight.running;
            to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::Reel(angling.reeling))));
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
            angling.reeling = held;
            to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::Reel(held))));
        }
        _ => {}
    }
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
    commands.spawn((
        Float,
        Mesh3d(meshes.add(Sphere::new(FLOAT_RADIUS).mesh().uv(16, 8))),
        MeshMaterial3d(float),
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        FishingLine,
        // A unit-long cylinder, stretched between the rod tip and the float.
        Mesh3d(meshes.add(Cylinder::new(LINE_RADIUS, 1.0).mesh().resolution(4))),
        MeshMaterial3d(line),
        // (A shadow of a line this thin would only be a dotted streak.)
        bevy::light::NotShadowCaster,
        Transform::default(),
        Visibility::Hidden,
    ));
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
    *float.1 = Visibility::Inherited;

    // The line: from above the player (the rod's tip) to the float.
    let tip = standing + Vec3::Y * ROD_TIP_HEIGHT;
    let span = at - tip;
    *line_mesh.0 = Transform {
        translation: tip + span / 2.0,
        rotation: Quat::from_rotation_arc(Vec3::Y, span.normalize_or(Vec3::Y)),
        scale: Vec3::new(1.0, span.length(), 1.0),
    };
    *line_mesh.1 = Visibility::Inherited;
}
