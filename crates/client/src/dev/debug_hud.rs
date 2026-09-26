//! On-screen debug text (where the player is and how the game is running),
//! shown and hidden with **F3**, and a reminder of the controls. Developer
//! tools such as the free camera (**F1**) only work while the debug text is
//! showing.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use venture_world::CellPos;

use crate::camera::CameraMode;
use crate::characters::player::LocalPlayer;
use crate::common::coords::from_bevy;
use crate::network::ClientState;
use crate::ui::text_input::KeyboardFocus;
use crate::world::streaming::{ChunkStore, ShownChunks};

const CONTROLS_HELP: &str = "Left-click or WASD: run\nClick someone: select; again: follow/talk\n\
                             Right-drag or arrow keys: rotate and tilt\nWheel: zoom\nEnter: chat (/help: commands)   Esc: deselect";
/// The developer controls, listed in the debug text.
const DEBUG_HELP: &str = "F1: free camera (WASD to fly, Shift: fast)   F3: hide this";

/// Shows the debug overlay.
pub struct DebugHudPlugin;

impl Plugin for DebugHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<DebugOverlay>()
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, (toggle_overlay, update_hud).chain());
    }
}

/// Whether the debug text (and the developer controls) are on.
#[derive(Resource, Default)]
pub struct DebugOverlay(pub bool);

/// F3 shows or hides the debug text.
fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    mut overlay: ResMut<DebugOverlay>,
    mut text: Single<&mut Visibility, With<HudText>>,
) {
    if keys.just_pressed(KeyCode::F3) && !focus.is_typing() {
        overlay.0 = !overlay.0;
    }
    let wanted = if overlay.0 { Visibility::Inherited } else { Visibility::Hidden };
    if **text != wanted {
        **text = wanted;
    }
}

/// The text block that [`update_hud`] rewrites every frame.
#[derive(Component)]
struct HudText;

fn spawn_hud(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(15.0), ..default() };
    let shadow = TextShadow { offset: Vec2::splat(1.5), ..default() };
    // (Below the player frame.)
    let debug = Node { position_type: PositionType::Absolute, left: px(10), top: px(78), ..default() };
    // Bottom right, clear of the chat box in the bottom left.
    let help = Node { position_type: PositionType::Absolute, right: px(10), bottom: px(10), ..default() };
    commands.spawn((HudText, Text::default(), font.clone(), shadow, debug, Visibility::Hidden));
    commands.spawn((Text::new(CONTROLS_HELP), font, shadow, help, TextLayout::justify(Justify::Right)));
}

fn update_hud(
    overlay: Res<DebugOverlay>,
    mut text: Single<&mut Text, With<HudText>>,
    player: Option<Single<&Transform, With<LocalPlayer>>>,
    state: Res<State<ClientState>>,
    mode: Res<CameraMode>,
    store: Res<ChunkStore>,
    shown: Res<ShownChunks>,
    diagnostics: Res<DiagnosticsStore>,
) {
    if !overlay.0 {
        return;
    }
    let Some(player) = player else {
        text.0 = format!("{:?}", state.get());
        return;
    };
    let (x, y) = from_bevy(player.translation);
    let cell_pos = CellPos::containing(x, y);
    let cell_info = store
        .0
        .get(&cell_pos.chunk())
        .map(|chunk| {
            let (lx, ly) = cell_pos.local();
            let cell = chunk.cell(lx, ly);
            let fluid = match cell.fluid {
                Some(fluid) => format!("{:?} at {:.1} m", fluid.kind, fluid.level),
                None => "none".to_string(),
            };
            format!("Biome: {:?}\nGround: {:?}\nLiquid: {fluid}", cell.biome, cell.ground)
        })
        .unwrap_or_else(|| "(chunk loading)".to_string());

    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|fps| fps.smoothed()).unwrap_or(0.0);

    text.0 = format!(
        "{:?}\nPlayer at ({x:.1}, {y:.1}) m, height {:.1} m\n{cell_info}\nCamera: {:?}\nChunks: {} shown, {} loaded\nFPS: {fps:.0}\n{DEBUG_HELP}",
        state.get(),
        player.translation.y,
        *mode,
        shown.0.len(),
        store.0.len(),
    );
}
