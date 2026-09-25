//! On-screen debug text (where the player is and how the game is running),
//! and a reminder of the controls.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use venture_world::CellPos;

use crate::camera::CameraMode;
use crate::chunk_streaming::{ChunkStore, ShownChunks};
use crate::coords::from_bevy;
use crate::network::ClientState;
use crate::player::LocalPlayer;

const CONTROLS_HELP: &str = "Left-click or WASD: run\nRight-drag or arrow keys: rotate and tilt\nWheel: zoom\n\
                             Enter: chat\nF1: free camera (WASD to fly, Shift: fast)";

/// Shows the debug overlay.
pub struct DebugHudPlugin;

impl Plugin for DebugHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud);
    }
}

/// The text block that [`update_hud`] rewrites every frame.
#[derive(Component)]
struct HudText;

fn spawn_hud(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(15.0), ..default() };
    let shadow = TextShadow { offset: Vec2::splat(1.5), ..default() };
    let debug = Node { position_type: PositionType::Absolute, left: px(10), top: px(10), ..default() };
    // Bottom right, clear of the chat box in the bottom left.
    let help = Node { position_type: PositionType::Absolute, right: px(10), bottom: px(10), ..default() };
    commands.spawn((HudText, Text::default(), font.clone(), shadow, debug));
    commands.spawn((Text::new(CONTROLS_HELP), font, shadow, help, TextLayout::justify(Justify::Right)));
}

fn update_hud(
    mut text: Single<&mut Text, With<HudText>>,
    player: Option<Single<&Transform, With<LocalPlayer>>>,
    state: Res<State<ClientState>>,
    mode: Res<CameraMode>,
    store: Res<ChunkStore>,
    shown: Res<ShownChunks>,
    diagnostics: Res<DiagnosticsStore>,
) {
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
        "{:?}\nPlayer at ({x:.1}, {y:.1}) m, height {:.1} m\n{cell_info}\nCamera: {:?}\nChunks: {} shown, {} loaded\nFPS: {fps:.0}",
        state.get(),
        player.translation.y,
        *mode,
        shown.0.len(),
        store.0.len(),
    );
}
