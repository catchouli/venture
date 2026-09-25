//! On-screen debug text: where the camera target is and how the game is running.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use venture_world::CellPos;

use crate::camera::CameraMode;
use crate::chunk_streaming::{ChunkStore, ShownChunks};
use crate::coords::from_bevy;
use crate::player::Player;

const CONTROLS_HELP: &str = "Left-click or WASD: run   Right-drag or arrow keys: rotate and tilt   Wheel: zoom   \
                             F1: free camera (WASD to fly, Shift: fast)";

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
    let panel = |top: bool| Node {
        position_type: PositionType::Absolute,
        left: px(10),
        top: if top { px(10) } else { Val::Auto },
        bottom: if top { Val::Auto } else { px(10) },
        ..default()
    };
    commands.spawn((HudText, Text::default(), font.clone(), shadow, panel(true)));
    commands.spawn((Text::new(CONTROLS_HELP), font, shadow, panel(false)));
}

fn update_hud(
    mut text: Single<&mut Text, With<HudText>>,
    player: Single<&Transform, With<Player>>,
    mode: Res<CameraMode>,
    store: Res<ChunkStore>,
    shown: Res<ShownChunks>,
    diagnostics: Res<DiagnosticsStore>,
) {
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
        "Player at ({x:.1}, {y:.1}) m, height {:.1} m\n{cell_info}\nCamera: {:?}\nChunks: {} shown, {} loaded\nFPS: {fps:.0}",
        player.translation.y,
        *mode,
        shown.0.len(),
        store.0.len(),
    );
}
