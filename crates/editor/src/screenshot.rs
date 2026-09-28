//! For checking the editor without a person at the keyboard: with
//! `VENTURE_SCREENSHOT=path`, the editor saves a picture of its window once
//! the world has loaded, and exits. (The window has to be on screen.)
//!
//! - `VENTURE_EDITOR_TAB=Classes`: the tab to show (by its title).
//! - `VENTURE_EDITOR_ENTRY=mage`: the entry to choose on it.
//! - `VENTURE_EDITOR_TABLE=1`: show content tabs as tables.
//! - `VENTURE_EDITOR_FOCUS=x,y`: where the camera looks (metres).
//! - `VENTURE_EDITOR_PLACE=town_guard`: choose this placed thing on the World
//!   tab (by id, or name for respawn points), and look at it.
//! - `VENTURE_EDITOR_WALL=start_village_wall`: choose this wall on the World
//!   tab, and look at its first point.
//! - `VENTURE_EDITOR_DISTANCE=metres`: how far the camera is from where it looks.
//! - `VENTURE_SCREENSHOT_DELAY=seconds`: wait longer before the picture.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use venture_client::world::streaming::StreamingProgress;

use crate::camera::EditorCamera;
use crate::document::Document;
use crate::places::{Places, Selection};
use crate::shell::{Shell, Tab};
use crate::walls::{WallTool, wall_defs};

/// Frames to wait before and after taking it.
const SETTLE_FRAMES: u32 = 30;

/// Takes a picture of the editor and exits, if `VENTURE_SCREENSHOT` is set.
pub struct ScreenshotPlugin;

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var("VENTURE_SCREENSHOT") else { return };
        let delay = std::env::var("VENTURE_SCREENSHOT_DELAY").ok().and_then(|value| value.parse().ok()).unwrap_or(1.0);
        app.insert_resource(Pending { path, frames: 0, taken: false, delay })
            .add_systems(PostStartup, choose_view)
            .add_systems(Update, (choose_place, choose_wall, take_when_loaded));
    }
}

#[derive(Resource)]
struct Pending {
    path: String,
    frames: u32,
    taken: bool,
    delay: f32,
}

fn choose_view(mut shell: ResMut<Shell>, document: Res<Document>, mut camera: Single<&mut EditorCamera>) {
    if let Ok(title) = std::env::var("VENTURE_EDITOR_TAB")
        && let Some(index) = document.collections.iter().position(|kind| kind.title().eq_ignore_ascii_case(&title))
    {
        shell.tab = Tab::Content(index);
        shell.show_tables(std::env::var_os("VENTURE_EDITOR_TABLE").is_some());
        if let Ok(key) = std::env::var("VENTURE_EDITOR_ENTRY") {
            let kind = &document.collections[index];
            for file in 0..kind.paths().len() {
                if let Some(entry) = kind.keys(file).iter().position(|each| *each == key) {
                    shell.choose(index, file, entry);
                }
            }
        }
    }
    let focus = std::env::var("VENTURE_EDITOR_FOCUS").ok().and_then(|text| {
        let numbers: Vec<f32> = text.split(',').filter_map(|part| part.trim().parse().ok()).collect();
        (numbers.len() == 2).then(|| Vec3::new(numbers[0], 0.0, numbers[1]))
    });
    if let Some(focus) = focus {
        camera.go_to(focus);
    }
    if let Some(distance) = std::env::var("VENTURE_EDITOR_DISTANCE").ok().and_then(|text| text.parse().ok()) {
        camera.distance = distance;
    }
}

fn choose_place(
    places: Res<Places>,
    mut selection: ResMut<Selection>,
    mut camera: Single<&mut EditorCamera>,
    mut done: Local<bool>,
) {
    let Ok(key) = std::env::var("VENTURE_EDITOR_PLACE") else { return };
    if *done {
        return;
    }
    if let Some(place) = places.list.iter().find(|place| place.key == key) {
        let (x, y) = place.position;
        selection.0 = Some((place.kind, place.key.clone()));
        camera.go_to(Vec3::new(x, 0.0, y));
        *done = true;
    }
}

fn choose_wall(
    document: Res<Document>,
    mut tool: ResMut<WallTool>,
    mut camera: Single<&mut EditorCamera>,
    mut done: Local<bool>,
) {
    let Ok(id) = std::env::var("VENTURE_EDITOR_WALL") else { return };
    if *done {
        return;
    }
    *done = true;
    if let Some(&(x, y)) = wall_defs(&document).iter().find(|def| def.id == id).and_then(|def| def.points.first()) {
        tool.choose(Some(id));
        camera.go_to(Vec3::new(x, 0.0, y));
    }
}

fn take_when_loaded(
    mut commands: Commands,
    time: Res<Time>,
    mut pending: ResMut<Pending>,
    progress: Res<StreamingProgress>,
    shell: Res<Shell>,
    mut exit: MessageWriter<AppExit>,
) {
    let loaded = shell.tab != Tab::World || (progress.measured && progress.waiting == 0);
    if !loaded || time.elapsed_secs() < pending.delay {
        return;
    }
    pending.frames += 1;
    if !pending.taken && pending.frames >= SETTLE_FRAMES {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(pending.path.clone()));
        pending.taken = true;
        pending.frames = 0;
    } else if pending.taken && pending.frames >= SETTLE_FRAMES {
        exit.write(AppExit::Success);
    }
}
