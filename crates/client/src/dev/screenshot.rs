//! A development tool for checking how the world looks without a person at
//! the keyboard: when the `VENTURE_SCREENSHOT` environment variable is set to
//! a file path, the client waits for the visible terrain to finish loading,
//! saves a screenshot there, and exits.
//!
//! The camera renders into an offscreen image rather than the window, so this
//! works even when the window can't be seen (such as on a locked screen).
//!
//! ```text
//! VENTURE_SCREENSHOT=shot.png cargo run -p venture_client -- --local
//! ```
//!
//! Combine with `VENTURE_CAMERA` (see [`crate::dev::settings`]) to choose the
//! view, and `VENTURE_SCREENSHOT_DELAY=seconds` to wait longer after loading
//! (for example, to capture something that happens a few seconds in).

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::network::ClientState;
use crate::world::streaming::StreamingProgress;

/// Size of the screenshot, in pixels.
const SIZE: (u32, u32) = (1600, 900);

/// Frames to wait after loading finishes, so shadows and exposure settle.
const SETTLE_FRAMES: u32 = 30;

/// Frames to wait after requesting the screenshot before exiting, so it has
/// time to be written.
const SAVE_FRAMES: u32 = 30;

/// Takes a screenshot and exits, if `VENTURE_SCREENSHOT` is set.
pub struct ScreenshotPlugin;

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var("VENTURE_SCREENSHOT") else { return };
        let delay = std::env::var("VENTURE_SCREENSHOT_DELAY").ok().and_then(|value| value.parse().ok()).unwrap_or(0.0);
        app.insert_resource(PendingScreenshot {
            path,
            target: Handle::default(),
            frames: 0,
            taken: false,
            delay: Timer::from_seconds(delay, TimerMode::Once),
        })
        .add_systems(Startup, create_target)
        .add_systems(Update, (render_offscreen, take_screenshot_when_loaded));
    }
}

#[derive(Resource)]
struct PendingScreenshot {
    path: String,
    /// The offscreen image the camera renders into.
    target: Handle<Image>,
    /// Frames counted since loading finished (or since the screenshot was taken).
    frames: u32,
    /// Extra time to wait once loaded, before settling and capturing.
    delay: Timer,
    taken: bool,
}

/// Creates the offscreen image the camera renders into.
fn create_target(mut pending: ResMut<PendingScreenshot>, mut images: ResMut<Assets<Image>>) {
    let image =
        Image::new_target_texture(SIZE.0, SIZE.1, TextureFormat::Rgba8Unorm, Some(TextureFormat::Rgba8UnormSrgb));
    pending.target = images.add(image);
}

/// Points each new camera at the offscreen image, drawing the interface there too.
fn render_offscreen(mut commands: Commands, pending: Res<PendingScreenshot>, cameras: Query<Entity, Added<Camera3d>>) {
    for camera in &cameras {
        commands.entity(camera).insert((RenderTarget::Image(pending.target.clone().into()), IsDefaultUiCamera));
    }
}

fn take_screenshot_when_loaded(
    mut commands: Commands,
    time: Res<Time>,
    mut pending: ResMut<PendingScreenshot>,
    progress: Res<StreamingProgress>,
    state: Res<State<ClientState>>,
    mut exit: MessageWriter<AppExit>,
) {
    let loaded = *state.get() == ClientState::Playing && progress.measured && progress.waiting == 0;
    if !loaded && !pending.taken {
        pending.frames = 0;
        return;
    }
    if !pending.delay.tick(time.delta()).is_finished() {
        return;
    }
    pending.frames += 1;
    if !pending.taken && pending.frames >= SETTLE_FRAMES {
        commands.spawn(Screenshot::image(pending.target.clone())).observe(save_to_disk(pending.path.clone()));
        pending.taken = true;
        pending.frames = 0;
    } else if pending.taken && pending.frames >= SAVE_FRAMES {
        exit.write(AppExit::Success);
    }
}
