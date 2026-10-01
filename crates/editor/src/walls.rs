//! Walls and fences on the World tab (see `docs/plan-walls.md`): drawn as
//! paths of points on the ground, shown as they'll be baked, and baked into
//! the world from here.
//!
//! The walls are the document's (`world_authoring/walls.ron`, a collection
//! like any other, so they undo and save with the rest). The editor bakes
//! them itself, against the same world generator the bake uses (built in
//! the background from the world's seed), and hands them to the game's own
//! wall drawing ([`ShownWalls`]), so what's shown is what the bake makes.
//!
//! - **Draw:** each click on the ground adds a point; double-click, Enter or
//!   Escape finishes, and clicking the first point again closes it into a ring.
//! - **Choose:** click a wall. Its points show as handles: drag one to move
//!   it, click the wall between two to add one there (and drag it), and
//!   Delete removes the chosen one.
//! - **Bake:** saves, then runs the bake (in the background, only the
//!   chunks walls touch), which also blocks the cells the walls stand on and
//!   clears trees from their way, then shows the world as baked.

use std::process::Child;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use bevy::window::PrimaryWindow;
use bevy_egui::egui;
use bevy_egui::input::EguiWantsInput;
use venture_client::common::coords::{from_bevy, to_bevy};
use venture_client::world::query::{ground_height, raycast_ground};
use venture_client::world::source::{ReloadWorld, WorldInfo};
use venture_client::world::streaming::ChunkStore;
use venture_client::world::walls::ShownWalls;
use venture_structures::{BakedWall, WallDef, WallKind};
use venture_worldgen::structures::bake_authored_wall;
use venture_worldgen::{WorldGenConfig, WorldGenerator};

use crate::Project;
use crate::camera::EditorCamera;
use crate::document::{Document, Entry};
use crate::places::Selection;
use crate::shell::ViewRect;

/// How close (metres) the pointer's ray must pass to a point's handle to pick it...
const HANDLE_RADIUS: f32 = 0.7;
/// ...or to a wall to pick the wall.
const WALL_RADIUS: f32 = 0.8;
/// How far the pointer must move (pixels) before a press becomes a drag.
const DRAG_START: f32 = 4.0;
/// How far past a wall's end a point added there starts, in metres.
const PAST_END: f32 = 1.0;
/// Two clicks this close together (seconds) are a double-click.
const DOUBLE_CLICK: f64 = 0.35;

/// Draws, edits and bakes walls.
pub struct WallToolsPlugin;

impl Plugin for WallToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WallTool>().init_resource::<WallGenerator>().add_systems(
            Update,
            (
                (make_generator, show_walls, watch_bake),
                // Handles and drawing come before placed things' markers; picking a wall after.
                wall_clicks.before(crate::places::PickPlaces),
                pick_wall.after(crate::places::PickPlaces).in_set(PickWalls),
                (wall_keys, draw_wall_guides),
            ),
        );
    }
}

/// Where clicks pick walls (buildings are picked after).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PickWalls;

/// The world generator, for baking walls (and buildings) as the bake does.
#[derive(Resource, Default)]
pub struct WallGenerator {
    /// The generator, once it's built.
    pub world: Option<Arc<WorldGenerator>>,
    building: Option<Task<WorldGenerator>>,
}

/// What's being done to walls.
#[derive(Resource, Default)]
pub struct WallTool {
    /// The chosen wall, by id.
    pub chosen: Option<String>,
    /// Its chosen point.
    pub point: Option<usize>,
    /// Whether points being clicked make the chosen wall (it's being drawn).
    pub drawing: bool,
    /// What kind of wall the next one drawn is.
    pub new_kind: WallKind,
    /// A press on a point's handle (its index, and where the pointer was
    /// pressed), which becomes a drag once the pointer moves.
    pressed: Option<(usize, Vec2)>,
    /// Where the point being dragged is now (it's written into the wall when let go).
    dragged_to: Option<(f32, f32)>,
    /// When and where the last click while drawing was (for double-clicks).
    last_click: Option<(f64, Vec2)>,
    /// The walls as last baked, with what they were baked from.
    baked: Vec<(WallDef, BakedWall)>,
    /// The bake, while it runs.
    bake: Option<Child>,
    /// What happened last, for the panel.
    pub status: String,
    /// A split asked for, at this point, waiting for "are you sure?".
    confirm_split: Option<usize>,
}

impl WallTool {
    /// Chooses a wall (or none).
    pub fn choose(&mut self, id: Option<String>) {
        if self.chosen != id {
            self.chosen = id;
            self.point = None;
            self.drawing = false;
            self.confirm_split = None;
        }
    }
}

/// Builds the world generator in the background once the world's seed is known.
fn make_generator(info: Option<Res<WorldInfo>>, mut generator: ResMut<WallGenerator>) {
    if generator.world.is_none()
        && generator.building.is_none()
        && let Some(info) = info
    {
        let seed = info.0.seed;
        generator.building = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { WorldGenerator::new(WorldGenConfig { seed, ..WorldGenConfig::default() }) }),
        );
    }
    if let Some(task) = generator.building.as_mut()
        && let Some(world) = check_ready(task)
    {
        generator.world = Some(Arc::new(world));
        generator.building = None;
    }
}

/// The walls in the document.
pub fn wall_defs(document: &Document) -> Vec<WallDef> {
    document
        .find::<WallDef>()
        .map(|(_, walls)| walls.files().iter().flat_map(|f| f.entries.clone()).collect())
        .unwrap_or_default()
}

/// Bakes the document's walls (with a point being dragged where it is now)
/// and shows them, whenever they change. Only walls that changed are baked again.
fn show_walls(
    document: Res<Document>,
    generator: Res<WallGenerator>,
    mut tool: ResMut<WallTool>,
    mut shown: ResMut<ShownWalls>,
) {
    let Some(world) = generator.world.as_ref() else { return };
    if !document.is_changed() && !tool.is_changed() && !generator.is_changed() {
        return;
    }
    let mut defs = wall_defs(&document);
    if let (Some(id), Some(index), Some(at)) = (&tool.chosen, tool.point, tool.dragged_to)
        && let Some(point) = defs.iter_mut().find(|def| &def.id == id).and_then(|def| def.points.get_mut(index))
    {
        *point = at;
    }
    if !shown.authored
        || tool.baked.len() != defs.len()
        || tool.baked.iter().zip(&defs).any(|((was, _), def)| was != def)
    {
        let baked: Vec<(WallDef, BakedWall)> = defs
            .into_iter()
            .map(|def| match tool.baked.iter().find(|(was, _)| *was == def) {
                Some((_, wall)) => (def, wall.clone()),
                None => {
                    let wall = bake_authored_wall(world, &def);
                    (def, wall)
                }
            })
            .collect();
        shown.walls = baked.iter().map(|(_, wall)| wall.clone()).collect();
        shown.authored = true;
        tool.bypass_change_detection().baked = baked;
    }
}

/// The pointer's ray into the world, if it's over the 3D view.
pub fn pointer_ray(window: &Window, view: &ViewRect, camera: (&Camera, &GlobalTransform)) -> Option<(Ray3d, Vec2)> {
    let cursor = window.cursor_position()?;
    if !view.0?.contains(cursor) {
        return None;
    }
    Some((camera.0.viewport_to_world(camera.1, cursor).ok()?, cursor))
}

/// How near a ray passes a point in front of it, and how far along it that is.
pub fn ray_miss(ray: Ray3d, point: Vec3) -> Option<(f32, f32)> {
    let along = (point - ray.origin).dot(*ray.direction);
    (along > 0.0).then(|| ((ray.origin + *ray.direction * along).distance(point), along))
}

/// Where a wall's points' handles are.
fn handles(def: &WallDef, store: &ChunkStore) -> Vec<Vec3> {
    def.points.iter().map(|&(x, y)| to_bevy(x, y, ground_height(store, x, y).unwrap_or(0.0) + 0.3)).collect()
}

/// The point of a baked wall a ray passes nearest (within reach): its
/// place on the ground, and how far along the ray it is.
fn wall_hit(ray: Ray3d, wall: &BakedWall) -> Option<(Vec2, f32)> {
    wall.samples
        .iter()
        .filter_map(|sample| {
            let middle = to_bevy(sample.x, sample.y, sample.ground + wall.height * 0.5);
            let (miss, along) = ray_miss(ray, middle)?;
            (miss < WALL_RADIUS + along * 0.004).then_some((Vec2::new(sample.x, sample.y), along, miss))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(at, along, _)| (at, along))
}

/// Where a new point goes in a wall to fall at `at`: after the point
/// whose line to the next passes nearest.
fn insert_index(def: &WallDef, at: Vec2) -> usize {
    let points: Vec<Vec2> = def.points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let segments = if def.closed { points.len() } else { points.len().saturating_sub(1) };
    (0..segments)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let run = b - a;
            let t = ((at - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (i, at.distance(a + run * t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(points.len(), |(i, _)| i + 1)
}

/// Where a point added past one end of a wall goes: a little further on,
/// the way the wall runs at that end (east, for a wall of one point).
pub fn past_end(def: &WallDef, at_start: bool) -> (f32, f32) {
    let points: Vec<Vec2> = def.points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let (end, before) = match (at_start, points.len()) {
        (_, 0) => return (0.0, 0.0),
        (_, 1) => (points[0], points[0] - Vec2::X),
        (true, _) => (points[0], points[1]),
        (false, n) => (points[n - 1], points[n - 2]),
    };
    let way = (end - before).normalize_or(Vec2::X);
    let at = end + way * PAST_END;
    ((at.x * 100.0).round() / 100.0, (at.y * 100.0).round() / 100.0)
}

/// Adds a point past one end of a wall (not a ring). Returns its index.
pub fn add_past_end(document: &mut Document, id: &str, at_start: bool, now: f64) -> Option<usize> {
    let mut index = None;
    change_wall(document, id, now, |def| {
        if def.closed {
            return;
        }
        let at = past_end(def, at_start);
        if at_start {
            def.points.insert(0, at);
            index = Some(0);
        } else {
            def.points.push(at);
            index = Some(def.points.len() - 1);
        }
    });
    index
}

/// A point added to a wall's list in its fields starts as a copy of the
/// last; this moves it a little past the end instead, so it can be seen
/// (and grabbed). `before` is the wall as it was. (The caller tells the
/// document about the change.)
pub fn place_added_point(document: &mut Document, before: &WallDef) {
    let Some((_, walls)) = document.find_mut::<WallDef>() else { return };
    let Some(def) =
        walls.files_mut().iter_mut().flat_map(|file| file.entries.iter_mut()).find(|def| def.id == before.id)
    else {
        return;
    };
    if def.points.len() == before.points.len() + 1
        && !before.points.is_empty()
        && def.points.last() == before.points.last()
    {
        let at = past_end(before, false);
        if let Some(last) = def.points.last_mut() {
            *last = at;
        }
    }
}

/// Where the "add a point" handles past a wall's ends are (none for a ring).
fn end_handles(def: &WallDef, store: &ChunkStore) -> Vec<(bool, Vec3)> {
    if def.closed || def.points.is_empty() {
        return Vec::new();
    }
    [true, false]
        .into_iter()
        .map(|at_start| {
            let (x, y) = past_end(def, at_start);
            (at_start, to_bevy(x, y, ground_height(store, x, y).unwrap_or(0.0) + 0.3))
        })
        .collect()
}

/// Rounds a point to the centimetre (so it saves as tidy numbers).
pub fn tidy(at: Vec3) -> (f32, f32) {
    let (x, y) = from_bevy(at);
    ((x * 100.0).round() / 100.0, (y * 100.0).round() / 100.0)
}

/// Changes a wall in the document, by id. Returns whether it was there.
fn change_wall(document: &mut Document, id: &str, now: f64, change: impl FnOnce(&mut WallDef)) -> bool {
    let Some((collection, walls)) = document.find_mut::<WallDef>() else { return false };
    for (f, file) in walls.files_mut().iter_mut().enumerate() {
        if let Some(def) = file.entries.iter_mut().find(|def| def.id == id) {
            change(def);
            document.changed(collection, f, now);
            return true;
        }
    }
    false
}

/// A seed for a new wall (or a new look for one).
pub fn new_seed(now: f64) -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    (u64::from(nanos) ^ (now * 1000.0) as u64) % 10_000
}

/// Adds a new wall with its first point. Returns its id.
fn add_wall(document: &mut Document, kind: WallKind, first: (f32, f32), now: f64) -> Option<String> {
    let (collection, walls) = document.find_mut::<WallDef>()?;
    let taken: Vec<String> = walls.files().iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect();
    let prefix = match kind {
        WallKind::DryStone => "wall",
        WallKind::Fence => "fence",
        WallKind::Hedge => "hedge",
        WallKind::Curtain => "castle_wall",
    };
    let id = (1..).map(|n| format!("{prefix}_{n}")).find(|id| !taken.contains(id))?;
    let height = kind.usual_height();
    let def = WallDef { id: id.clone(), kind, points: vec![first], closed: false, height, seed: new_seed(now) };
    walls.files_mut().first_mut()?.entries.push(def);
    document.changed(collection, 0, now);
    Some(id)
}

/// What splitting a wall at a point makes: the walls it becomes (each of
/// at least two points). An open wall becomes the parts either side of the
/// point (a part of a single point is too short to be a wall, so it goes);
/// a ring opens there. The point itself goes. `None` if it can't be split
/// there (an open wall's end, or too few points).
fn split_parts(def: &WallDef, index: usize) -> Option<Vec<Vec<(f32, f32)>>> {
    let points = &def.points;
    if index >= points.len() {
        return None;
    }
    let parts = if def.closed {
        vec![points[index + 1..].iter().chain(&points[..index]).copied().collect::<Vec<_>>()]
    } else {
        if index == 0 || index + 1 == points.len() {
            return None;
        }
        vec![points[..index].to_vec(), points[index + 1..].to_vec()]
    };
    let parts: Vec<Vec<(f32, f32)>> = parts.into_iter().filter(|part| part.len() >= 2).collect();
    (!parts.is_empty()).then_some(parts)
}

/// Splits a wall at a point (see [`split_parts`]), as one change. Returns
/// the id of the new wall, if it made one.
pub fn split_wall(document: &mut Document, id: &str, index: usize, now: f64) -> Option<String> {
    let (collection, walls) = document.find_mut::<WallDef>()?;
    let taken: Vec<String> = walls.files().iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect();
    let (f, file) =
        walls.files_mut().iter_mut().enumerate().find(|(_, file)| file.entries.iter().any(|def| def.id == id))?;
    let at = file.entries.iter().position(|def| def.id == id)?;
    let mut parts = split_parts(&file.entries[at], index)?.into_iter();
    let first = parts.next()?;
    let def = &mut file.entries[at];
    def.points = first;
    def.closed = false;
    let mut made = None;
    if let Some(second) = parts.next() {
        let new_id = (2..).map(|n| format!("{id}_{n}")).find(|key| !taken.contains(key))?;
        let copy = WallDef { id: new_id.clone(), points: second, seed: def.seed + 1, ..def.clone() };
        file.entries.insert(at + 1, copy);
        made = Some(new_id);
    }
    document.changed(collection, f, now);
    made
}

/// Removes a wall.
pub fn delete_wall(document: &mut Document, id: &str, now: f64) {
    let Some((collection, walls)) = document.find_mut::<WallDef>() else { return };
    for (f, file) in walls.files_mut().iter_mut().enumerate() {
        if let Some(index) = file.entries.iter().position(|def| def.id == id) {
            file.entries.remove(index);
            document.changed(collection, f, now);
            return;
        }
    }
}

/// Finishes drawing: a wall of fewer than two points is taken away again.
pub fn finish_drawing(tool: &mut WallTool, document: &mut Document, now: f64) {
    tool.drawing = false;
    let Some(id) = tool.chosen.clone() else { return };
    if wall_defs(document).iter().any(|def| def.id == id && def.points.len() < 2) {
        delete_wall(document, &id, now);
        tool.choose(None);
    }
}

/// Clicks for walls that come before placed things': drawing, and the
/// chosen wall's handles and line. They take the click, so it doesn't also
/// choose a marker. Also drags points, and writes where they went when let go.
fn wall_clicks(
    time: Res<Time>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
    store: Res<ChunkStore>,
    mut tool: ResMut<WallTool>,
    mut selection: ResMut<Selection>,
    mut document: ResMut<Document>,
) {
    let now = time.elapsed_secs_f64();
    let pointer = pointer_ray(&window, &view, *camera);
    let chosen = tool.chosen.as_ref().and_then(|id| wall_defs(&document).into_iter().find(|def| &def.id == id));

    if buttons.just_pressed(MouseButton::Left) && !egui.wants_pointer_input() {
        let Some((ray, cursor)) = pointer else { return };
        // Drawing: every click adds a point (a double-click, or the first point again, finishes).
        if tool.drawing {
            buttons.clear_just_pressed(MouseButton::Left);
            let double =
                tool.last_click.is_some_and(|(at, place)| now - at < DOUBLE_CLICK && place.distance(cursor) < 6.0);
            tool.last_click = Some((now, cursor));
            if double {
                finish_drawing(&mut tool, &mut document, now);
                return;
            }
            if let Some(def) = &chosen
                && def.points.len() >= 3
                && handles(def, &store)
                    .first()
                    .and_then(|&first| ray_miss(ray, first))
                    .is_some_and(|(miss, along)| miss < HANDLE_RADIUS + along * 0.004)
            {
                change_wall(&mut document, &def.id, now, |def| def.closed = true);
                finish_drawing(&mut tool, &mut document, now);
                return;
            }
            let Some(ground) = raycast_ground(&store, ray) else { return };
            let at = tidy(ground);
            match &chosen {
                Some(def) => {
                    change_wall(&mut document, &def.id, now, |def| def.points.push(at));
                    tool.point = Some(def.points.len());
                }
                None => {
                    let kind = tool.new_kind;
                    let id = add_wall(&mut document, kind, at, now);
                    tool.chosen = id;
                    tool.point = Some(0);
                    selection.0 = None;
                }
            }
            return;
        }
        let Some(def) = chosen else { return };
        // A handle of the chosen wall: choose that point (and maybe drag it).
        let handle = handles(&def, &store)
            .into_iter()
            .enumerate()
            .filter_map(|(i, at)| {
                let (miss, along) = ray_miss(ray, at)?;
                (miss < HANDLE_RADIUS + along * 0.004).then_some((i, along))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, _)) = handle {
            buttons.clear_just_pressed(MouseButton::Left);
            tool.point = Some(index);
            tool.pressed = Some((index, cursor));
            return;
        }
        // A handle past an end: a new point there (to drag on further).
        let past = end_handles(&def, &store)
            .into_iter()
            .find(|&(_, at)| ray_miss(ray, at).is_some_and(|(miss, along)| miss < HANDLE_RADIUS + along * 0.004));
        if let Some((at_start, _)) = past {
            buttons.clear_just_pressed(MouseButton::Left);
            if let Some(index) = add_past_end(&mut document, &def.id, at_start, now) {
                tool.point = Some(index);
                tool.pressed = Some((index, cursor));
            }
            return;
        }
        // The chosen wall's line: a new point there, to drag.
        let baked = tool.baked.iter().find(|(was, _)| was.id == def.id).map(|(_, wall)| wall.clone());
        if let Some((at, _)) = baked.and_then(|wall| wall_hit(ray, &wall)) {
            buttons.clear_just_pressed(MouseButton::Left);
            let index = insert_index(&def, at);
            let point = ((at.x * 100.0).round() / 100.0, (at.y * 100.0).round() / 100.0);
            change_wall(&mut document, &def.id, now, |def| def.points.insert(index, point));
            tool.point = Some(index);
            tool.pressed = Some((index, cursor));
            tool.dragged_to = Some(point);
        }
        return;
    }

    // Dragging a point, and letting it go.
    let Some((index, pressed_at)) = tool.pressed else { return };
    if buttons.pressed(MouseButton::Left) {
        let Some((ray, cursor)) = pointer else { return };
        if tool.dragged_to.is_none() && cursor.distance(pressed_at) < DRAG_START {
            return;
        }
        if let Some(ground) = raycast_ground(&store, ray) {
            tool.dragged_to = Some(tidy(ground));
        }
        return;
    }
    tool.pressed = None;
    if let (Some(at), Some(def)) = (tool.dragged_to.take(), chosen) {
        change_wall(&mut document, &def.id, now, |def| {
            if let Some(point) = def.points.get_mut(index) {
                *point = at;
            }
        });
    }
}

/// A click that chose nothing else chooses the wall under the pointer (or
/// no wall); choosing a placed thing leaves walls unchosen.
fn pick_wall(
    buttons: Res<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
    selection: Res<Selection>,
    mut tool: ResMut<WallTool>,
) {
    if !buttons.just_pressed(MouseButton::Left) || egui.wants_pointer_input() {
        return;
    }
    let Some((ray, _)) = pointer_ray(&window, &view, *camera) else { return };
    if selection.0.is_some() {
        tool.choose(None);
        return;
    }
    let picked = tool
        .baked
        .iter()
        .filter_map(|(def, wall)| wall_hit(ray, wall).map(|(_, along)| (def.id.clone(), along)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id);
    tool.choose(picked);
}

/// Enter or Escape finishes drawing; Delete (or Backspace) removes the chosen point.
fn wall_keys(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    mut tool: ResMut<WallTool>,
    mut document: ResMut<Document>,
) {
    if egui.wants_keyboard_input() || view.0.is_none() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if tool.drawing && keys.any_just_pressed([KeyCode::Enter, KeyCode::Escape]) {
        finish_drawing(&mut tool, &mut document, now);
        return;
    }
    if !keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        return;
    }
    let (Some(id), Some(index)) = (tool.chosen.clone(), tool.point) else { return };
    let mut removed = false;
    change_wall(&mut document, &id, now, |def| {
        let least = if def.closed { 3 } else { 2 };
        if def.points.len() > least && index < def.points.len() {
            def.points.remove(index);
            removed = true;
        }
    });
    if removed {
        tool.point = index.checked_sub(1);
        tool.status = String::new();
    } else {
        tool.status = "A wall needs at least two points (three for a ring); delete the wall instead.".to_string();
    }
}

/// Draws the chosen wall's line and its points' handles, and while drawing,
/// the way from its last point to the pointer.
fn draw_wall_guides(
    mut gizmos: Gizmos,
    tool: Res<WallTool>,
    document: Res<Document>,
    store: Res<ChunkStore>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
) {
    if view.0.is_none() {
        return;
    }
    let Some(id) = &tool.chosen else { return };
    let Some(mut def) = wall_defs(&document).into_iter().find(|def| &def.id == id) else { return };
    if let (Some(index), Some(at)) = (tool.point, tool.dragged_to)
        && let Some(point) = def.points.get_mut(index)
    {
        *point = at;
    }
    // The line along the top (as baked).
    if let Some((_, wall)) = tool.baked.iter().find(|(was, _)| was.id == def.id) {
        let top = wall.samples.iter().map(|sample| to_bevy(sample.x, sample.y, sample.ground + wall.height + 0.15));
        gizmos.linestrip(top, Color::srgba(1.0, 1.0, 1.0, 0.8));
    }
    // The points: handles, joined by straight lines on the ground.
    let places = handles(&def, &store);
    let mut line = places.clone();
    if def.closed
        && let Some(&first) = places.first()
    {
        line.push(first);
    }
    gizmos.linestrip(line, Color::srgba(1.0, 0.85, 0.3, 0.6));
    for (i, &at) in places.iter().enumerate() {
        let colour = if tool.point == Some(i) { Color::srgb(1.0, 0.85, 0.2) } else { Color::WHITE };
        gizmos.sphere(Isometry3d::from_translation(at), 0.35, colour);
    }
    // Past each end (unless it's being drawn): a "+" to add a point there.
    if !tool.drawing {
        let colour = Color::srgba(0.5, 1.0, 0.6, 0.9);
        for ((_, at), end) in end_handles(&def, &store).into_iter().zip([places.first(), places.last()]) {
            if let Some(&end) = end {
                gizmos.line(end, at, Color::srgba(0.5, 1.0, 0.6, 0.4));
            }
            gizmos.sphere(Isometry3d::from_translation(at), 0.25, colour);
            gizmos.line(at - Vec3::X * 0.18, at + Vec3::X * 0.18, colour);
            gizmos.line(at - Vec3::Z * 0.18, at + Vec3::Z * 0.18, colour);
        }
    }
    if tool.drawing
        && let Some(&last) = places.last()
        && let Some((ray, _)) = pointer_ray(&window, &view, *camera)
        && let Some(ground) = raycast_ground(&store, ray)
    {
        gizmos.line(last, ground + Vec3::Y * 0.3, Color::srgba(1.0, 0.85, 0.3, 0.9));
    }
}

/// Saves everything, then bakes the world (in the background) with the walls as they are.
pub fn save_and_bake(tool: &mut WallTool, document: &mut Document, project: &Project, seed: Option<u32>) {
    if tool.bake.is_some() {
        return;
    }
    if let Err(error) = document.save() {
        tool.status = format!("Couldn't save: {error}");
        return;
    }
    let mut command = crate::program("bake", &["run", "--release", "-p", "venture_worldgen", "--bin", "bake"]);
    // (Only the chunks walls, buildings and bridges touch, unless the world there was baked differently.)
    command.arg("--structures");
    command.arg(&project.world).arg(seed.unwrap_or_default().to_string()).arg(&project.authoring);
    command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    match command.spawn() {
        Ok(child) => {
            tool.bake = Some(child);
            tool.status = "Baking the world…".to_string();
        }
        Err(error) => tool.status = format!("Couldn't start the bake: {error}"),
    }
}

/// Whether the bake is running.
pub fn baking(tool: &WallTool) -> bool {
    tool.bake.is_some()
}

/// Notices when the bake finishes, and shows the world as baked (the ground
/// walls cleared, and the map).
fn watch_bake(mut tool: ResMut<WallTool>, mut reload: MessageWriter<ReloadWorld>) {
    let Some(child) = tool.bake.as_mut() else { return };
    let done = match child.try_wait() {
        Ok(Some(status)) if status.success() => {
            reload.write(ReloadWorld { chunks: None });
            "Baked. A game or server that's running picks the new world up by itself.".to_string()
        }
        Ok(Some(status)) => format!("The bake failed ({status}); run it in a terminal to see why."),
        Ok(None) => return,
        Err(error) => format!("Couldn't follow the bake: {error}"),
    };
    tool.status = done;
    tool.bake = None;
}

/// The Walls part of the World tab's left panel: drawing, and the list of
/// walls (near where the camera looks, if `nearby` is set). Returns a wall
/// to look at, if one was clicked.
pub fn walls_list(
    ui: &mut egui::Ui,
    tool: &mut WallTool,
    document: &mut Document,
    nearby: Option<f32>,
    focus: (f32, f32),
    now: f64,
) -> Option<(f32, f32)> {
    let mut go_to = None;
    let defs = wall_defs(document);
    let near = |def: &WallDef| {
        nearby.is_none_or(|metres| def.points.iter().any(|&(x, y)| (x - focus.0).hypot(y - focus.1) <= metres))
            || tool.chosen.as_ref() == Some(&def.id)
    };
    let shown: Vec<&WallDef> = defs.iter().filter(|def| near(def)).collect();
    let count = if shown.len() == defs.len() {
        format!("{}", defs.len())
    } else {
        format!("{} of {}", shown.len(), defs.len())
    };
    egui::CollapsingHeader::new(format!("Walls and fences ({count})")).id_salt("walls").default_open(true).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                if tool.drawing {
                    if ui.button("Finish").on_hover_text("Or double-click, or press Enter").clicked() {
                        finish_drawing(tool, document, now);
                    }
                } else {
                    if ui.button("Draw").on_hover_text("Click points on the ground; double-click to finish").clicked() {
                        tool.choose(None);
                        tool.drawing = true;
                        tool.last_click = None;
                    }
                    egui::ComboBox::from_id_salt("new wall kind").selected_text(tool.new_kind.name()).show_ui(
                        ui,
                        |ui| {
                            for kind in WallKind::ALL {
                                ui.selectable_value(&mut tool.new_kind, kind, kind.name());
                            }
                        },
                    );
                }
            });
            if tool.drawing {
                ui.weak("Click the ground to add points. Click the first point to close a ring.");
            }
            if !tool.status.is_empty() {
                ui.label(&tool.status);
            }
            for def in shown {
                let label = format!("{} ({})", def.id, def.kind.name().to_lowercase());
                if ui.selectable_label(tool.chosen.as_ref() == Some(&def.id), label).clicked() {
                    tool.choose(Some(def.id.clone()));
                    go_to = def.points.first().copied();
                }
            }
        },
    );
    go_to
}

/// The chosen wall's own buttons, above its fields: a new seed, and deleting it.
pub fn wall_buttons(ui: &mut egui::Ui, tool: &mut WallTool, document: &mut Document, now: f64) {
    let Some(id) = tool.chosen.clone() else { return };
    let length =
        tool.baked.iter().find(|(def, _)| def.id == id).map_or(0.0, |(_, wall)| {
            wall.samples.len().saturating_sub(1) as f32 * venture_structures::WALL_SAMPLE_SPACING
        });
    ui.horizontal(|ui| {
        ui.label(format!("{length:.0} m long"));
        if ui.button("New seed").on_hover_text("The same wall, with its stones laid differently").clicked() {
            let seed = new_seed(now);
            change_wall(document, &id, now, |def| def.seed = seed);
        }
        if ui.button("Delete wall").clicked() {
            delete_wall(document, &id, now);
            tool.choose(None);
        }
    });
    let ring = wall_defs(document).iter().any(|def| def.id == id && def.closed);
    ui.add_enabled_ui(!ring, |ui| {
        ui.horizontal(|ui| {
            ui.label("Add a point:");
            for (at_start, label) in [(true, "At the start"), (false, "At the end")] {
                if ui.button(label).clicked()
                    && let Some(index) = add_past_end(document, &id, at_start, now)
                {
                    tool.point = Some(index);
                }
            }
        });
    });
    // Splitting at the chosen point (after "are you sure?").
    let def = wall_defs(document).into_iter().find(|def| def.id == id);
    let can_split = def.as_ref().zip(tool.point).is_some_and(|(def, index)| split_parts(def, index).is_some());
    match (tool.confirm_split, tool.point) {
        (Some(asked), Some(point)) if asked == point && can_split => {
            ui.horizontal(|ui| {
                let ring = def.as_ref().is_some_and(|def| def.closed);
                ui.label(if ring {
                    format!("Open the ring at point {}, removing it?", point + 1)
                } else {
                    format!("Split in two at point {}, removing it?", point + 1)
                });
                if ui.button("Split").clicked() {
                    let made = split_wall(document, &id, point, now);
                    tool.status = match made {
                        Some(new) => format!("Split: the rest is {new}."),
                        None => "Split.".to_string(),
                    };
                    tool.point = None;
                    tool.confirm_split = None;
                }
                if ui.button("Cancel").clicked() {
                    tool.confirm_split = None;
                }
            });
        }
        _ => {
            tool.confirm_split = None;
            let button = ui.add_enabled(can_split, egui::Button::new("Split here…"));
            let button = button.on_hover_text(
                "Splits the wall in two at the chosen point, removing it (or opens a ring there). \
                 Choose a point that isn't an end first.",
            );
            if button.clicked() {
                tool.confirm_split = tool.point;
            }
        }
    }
    ui.weak(
        "Drag a point to move it. Click the wall between two points to add one, or a green + past an end. \
         Delete removes the chosen point.",
    );
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::document::copy_folder;

    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let folder = std::env::temp_dir().join(format!("venture-walls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        copy_folder(&root.join("content"), &folder.join("content")).unwrap();
        copy_folder(&root.join("world_authoring"), &folder.join("world_authoring")).unwrap();
        (folder.join("content"), folder.join("world_authoring"))
    }

    #[test]
    fn walls_are_drawn_saved_and_checked() {
        let (content, authoring) = scratch("draw");
        let mut document = Document::load_with_authoring(&content, Some(&authoring)).unwrap();
        let before = wall_defs(&document).len();
        let id = add_wall(&mut document, WallKind::Fence, (10.0, 10.0), 0.0).unwrap();
        assert_eq!(id, "fence_1");
        // One point isn't a wall yet.
        let mirror = std::env::temp_dir().join(format!("venture-walls-mirror-{}", std::process::id()));
        copy_folder(&content, &mirror).unwrap();
        assert!(document.check(&mirror).iter().any(|problem| problem.message.contains("fence_1")));
        change_wall(&mut document, &id, 5.0, |def| def.points.push((20.0, 10.0)));
        assert!(document.check(&mirror).is_empty(), "{:?}", document.check(&mirror));
        // Saved into the authoring folder (not the content folder), and read back the same.
        document.save().unwrap();
        assert!(!content.join("walls.ron").exists());
        let again = Document::load_with_authoring(&content, Some(&authoring)).unwrap();
        assert_eq!(wall_defs(&again).len(), before + 1);
        assert_eq!(wall_defs(&again).last().unwrap().points, vec![(10.0, 10.0), (20.0, 10.0)]);
        // A wall abandoned with one point goes away.
        let chosen = add_wall(&mut document, WallKind::DryStone, (0.0, 0.0), 9.0);
        let mut tool = WallTool { chosen, drawing: true, ..WallTool::default() };
        finish_drawing(&mut tool, &mut document, 9.5);
        assert_eq!(wall_defs(&document).len(), before + 1);
        assert!(tool.chosen.is_none() && !tool.drawing);
        let _ = std::fs::remove_dir_all(content.parent().unwrap());
        let _ = std::fs::remove_dir_all(&mirror);
    }

    #[test]
    fn points_added_at_the_ends_go_a_little_past_them() {
        let (content, authoring) = scratch("ends");
        let mut document = Document::load_with_authoring(&content, Some(&authoring)).unwrap();
        let id = add_wall(&mut document, WallKind::Fence, (10.0, 10.0), 0.0).unwrap();
        change_wall(&mut document, &id, 0.0, |def| def.points.push((20.0, 10.0)));
        assert_eq!(add_past_end(&mut document, &id, true, 1.0), Some(0));
        assert_eq!(add_past_end(&mut document, &id, false, 2.0), Some(3));
        let def = wall_defs(&document).into_iter().find(|def| def.id == id).unwrap();
        assert_eq!(def.points, vec![(9.0, 10.0), (10.0, 10.0), (20.0, 10.0), (21.0, 10.0)]);
        // A copy of the last point (as the fields' "+ Add" makes) moves past the end too.
        change_wall(&mut document, &id, 3.0, |def| def.points.push((21.0, 10.0)));
        place_added_point(&mut document, &def);
        let def = wall_defs(&document).into_iter().find(|def| def.id == id).unwrap();
        assert_eq!(def.points.last(), Some(&(22.0, 10.0)));
        let _ = std::fs::remove_dir_all(content.parent().unwrap());
    }

    #[test]
    fn walls_split_in_two_and_rings_open() {
        let def = WallDef {
            id: "w".into(),
            kind: WallKind::DryStone,
            points: vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)],
            closed: false,
            height: 1.3,
            seed: 0,
        };
        assert_eq!(split_parts(&def, 2), Some(vec![vec![(0.0, 0.0), (1.0, 0.0)], vec![(3.0, 0.0), (4.0, 0.0)]]));
        assert_eq!(split_parts(&def, 1), Some(vec![vec![(2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]]), "a one-point part goes");
        assert_eq!(split_parts(&def, 0), None, "not at an end");
        let ring = WallDef { closed: true, ..def.clone() };
        assert_eq!(split_parts(&ring, 1), Some(vec![vec![(2.0, 0.0), (3.0, 0.0), (4.0, 0.0), (0.0, 0.0)]]));

        let (content, authoring) = scratch("split");
        let mut document = Document::load_with_authoring(&content, Some(&authoring)).unwrap();
        let id = add_wall(&mut document, WallKind::Fence, (0.0, 0.0), 0.0).unwrap();
        change_wall(&mut document, &id, 0.0, |wall| wall.points = def.points.clone());
        let made = split_wall(&mut document, &id, 2, 5.0).unwrap();
        let walls = wall_defs(&document);
        let first = walls.iter().find(|wall| wall.id == id).unwrap();
        let second = walls.iter().find(|wall| wall.id == made).unwrap();
        assert_eq!((first.points.len(), second.points.len(), second.kind), (2, 2, WallKind::Fence));
        document.undo();
        assert!(!wall_defs(&document).iter().any(|wall| wall.id == made), "one undo puts it back");
        let _ = std::fs::remove_dir_all(content.parent().unwrap());
    }

    #[test]
    fn new_points_go_between_the_points_they_fall_between() {
        let def = WallDef {
            id: "w".into(),
            kind: WallKind::DryStone,
            points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)],
            closed: false,
            height: 1.3,
            seed: 0,
        };
        assert_eq!(insert_index(&def, Vec2::new(5.0, 0.3)), 1);
        assert_eq!(insert_index(&def, Vec2::new(9.8, 6.0)), 2);
        let ring = WallDef { closed: true, ..def };
        assert_eq!(insert_index(&ring, Vec2::new(4.0, 4.3)), 3, "on the way back round to the start");
    }
}
