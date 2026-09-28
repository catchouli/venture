//! Buildings on the World tab (see `docs/plan-buildings.md`): drawn as an
//! outline of right-angled corners, shown as they'll be baked, and their
//! floor plans edited in a mode of their own.
//!
//! The buildings are the document's (`world_authoring/buildings.ron`), and
//! their styles too (`content/buildings/styles.ron`, a tab of their own).
//! The editor bakes them itself, against the same world generator the bake
//! uses, and hands them to the game's own drawing ([`ShownBuildings`]).
//!
//! - **Add:** click its corners on the ground; each is squared up to the
//!   last wall (so every corner is a right angle). Click the first corner
//!   again (or press Enter) to close it: the last corner is found for you.
//! - **Choose:** click a building. Drag the handle in its middle to move
//!   it; the buttons on the right turn it.
//! - **Edit plan:** its meshes hide and the view looks down on it:
//!   - *Corners:* drag them (the walls either side follow, square);
//!   - *Inner walls:* click where one starts and where it ends;
//!   - *Doors:* click a wall (outer or inner);
//!   - *Choose:* click a door or inner wall, and Delete removes it.
//!
//!   **Done** (or Escape) leaves the plan.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::egui;
use bevy_egui::input::EguiWantsInput;
use venture_client::common::coords::to_bevy;
use venture_client::world::buildings::ShownBuildings;
use venture_client::world::query::{ground_height, raycast_ground};
use venture_client::world::streaming::ChunkStore;
use venture_structures::buildings::plan::contains;
use venture_structures::buildings::{BuildingStyle, bake_building};
use venture_world::{BuildingPlan, DoorPlan, RoofShape, WallRef};

use crate::camera::EditorCamera;
use crate::document::{Document, Entry};
use crate::places::Selection;
use crate::shell::ViewRect;
use crate::walls::{WallGenerator, WallTool, new_seed, pointer_ray, ray_miss, tidy};

/// How close (metres) the pointer's ray must pass to a handle to pick it.
const HANDLE_RADIUS: f32 = 0.7;
/// How close (metres) a click must be to a wall to put a door in it, or to
/// a door or inner wall to choose it.
const WALL_REACH: f32 = 1.2;
/// How far the pointer must move (pixels) before a press becomes a drag.
const DRAG_START: f32 = 4.0;
/// A new building's walls and inner walls are rounded to this, in metres.
const SNAP: f32 = 0.05;

/// Draws, edits and shows buildings.
pub struct BuildingToolsPlugin;

impl Plugin for BuildingToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildingTool>().add_systems(
            Update,
            (
                show_buildings,
                building_clicks.before(crate::places::PickPlaces).before(crate::walls::PickWalls),
                pick_building.after(crate::walls::PickWalls),
                (building_keys, draw_building_guides),
            ),
        );
    }
}

/// What the plan editor's clicks do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlanTool {
    /// Drag corners.
    #[default]
    Corners,
    /// Draw inner walls.
    InnerWalls,
    /// Put doors in walls.
    Doors,
    /// Choose doors and inner walls (to delete).
    Choose,
}

/// Something chosen in a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanPiece {
    /// A door, by its place in the list.
    Door(usize),
    /// An inner wall, by its place in the list.
    InnerWall(usize),
}

/// What's being done to buildings.
#[derive(Resource, Default)]
pub struct BuildingTool {
    /// The chosen building, by id.
    pub chosen: Option<String>,
    /// The corners of a building being drawn (not in the document yet), if one is.
    pub drawing: Option<Vec<Vec2>>,
    /// What style the next building drawn is.
    pub new_style: String,
    /// Whether the chosen building's plan is being edited, and with what.
    pub editing: Option<PlanTool>,
    /// What's chosen in the plan.
    pub piece: Option<PlanPiece>,
    /// Where an inner wall being drawn starts.
    wall_start: Option<Vec2>,
    /// A press on a handle (a corner's index, or `None` for the building's
    /// middle), and where the pointer was pressed; a drag once it moves.
    pressed: Option<(Option<usize>, Vec2)>,
    /// Where the handle being dragged is now.
    dragged_to: Option<Vec2>,
    /// Where the camera was before the plan was edited (to go back to).
    camera_before: Option<(Vec3, f32, f32, f32)>,
    /// What happened last, for the panel.
    pub status: String,
}

impl BuildingTool {
    /// Chooses a building (or none), leaving its plan.
    pub fn choose(&mut self, id: Option<String>) {
        if self.chosen != id {
            self.chosen = id;
            self.editing = None;
            self.piece = None;
            self.wall_start = None;
        }
    }
}

/// The buildings in the document.
pub fn building_plans(document: &Document) -> Vec<BuildingPlan> {
    document
        .find::<BuildingPlan>()
        .map(|(_, plans)| plans.files().iter().flat_map(|f| f.entries.clone()).collect())
        .unwrap_or_default()
}

/// The building styles in the document.
pub fn styles(document: &Document) -> Vec<BuildingStyle> {
    document
        .find::<BuildingStyle>()
        .map(|(_, styles)| styles.files().iter().flat_map(|f| f.entries.clone()).collect())
        .unwrap_or_default()
}

/// Changes a building in the document, by id. Returns whether it was there.
fn change_building(document: &mut Document, id: &str, now: f64, change: impl FnOnce(&mut BuildingPlan)) -> bool {
    let Some((collection, plans)) = document.find_mut::<BuildingPlan>() else { return false };
    for (f, file) in plans.files_mut().iter_mut().enumerate() {
        if let Some(plan) = file.entries.iter_mut().find(|plan| plan.id == id) {
            change(plan);
            document.changed(collection, f, now);
            return true;
        }
    }
    false
}

/// Removes a building.
pub fn delete_building(document: &mut Document, id: &str, now: f64) {
    let Some((collection, plans)) = document.find_mut::<BuildingPlan>() else { return };
    for (f, file) in plans.files_mut().iter_mut().enumerate() {
        if let Some(index) = file.entries.iter().position(|plan| plan.id == id) {
            file.entries.remove(index);
            document.changed(collection, f, now);
            return;
        }
    }
}

/// Rounds to the nearest [`SNAP`].
fn snap(value: f32) -> f32 {
    (value / SNAP).round() * SNAP
}

/// A corner being drawn after `corners`, where the pointer is: squared up
/// to the last wall (it lies straight on from the last corner, across the
/// last wall), so every corner is a right angle.
pub fn square_up(corners: &[Vec2], pointer: Vec2) -> Vec2 {
    match corners {
        [] => pointer,
        [only] => *only + (pointer - *only),
        [.., before, last] => {
            let across = (*last - *before).normalize_or(Vec2::X).perp();
            *last + across * (pointer - *last).dot(across)
        }
    }
}

/// Closes an outline: the corner it needs to come back square to its first
/// (if it isn't already there). `None` if it can't close (too few corners).
pub fn close(corners: &[Vec2]) -> Option<Vec<Vec2>> {
    if corners.len() < 3 {
        return None;
    }
    let (first, second) = (corners[0], corners[1]);
    let (before, last) = (corners[corners.len() - 2], corners[corners.len() - 1]);
    // The last corner's next wall runs across the last wall; the first wall's
    // start is reached across the first wall. Where they meet is the corner wanted.
    let out_of_last = (last - before).normalize_or(Vec2::X).perp();
    let into_first = (second - first).normalize_or(Vec2::X).perp();
    let denominator = out_of_last.perp_dot(into_first);
    let mut outline = corners.to_vec();
    if denominator.abs() > 1e-4 {
        let t = (first - last).perp_dot(into_first) / denominator;
        let corner = last + out_of_last * t;
        if corner.distance(last) > 0.05 && corner.distance(first) > 0.05 {
            outline.push(corner);
        }
    }
    // Every wall must still be square (an odd number of corners can't be).
    (outline.len() >= 4 && outline.len().is_multiple_of(2)).then_some(outline)
}

/// Moves corner `index` of an outline to `to`, with the walls either side
/// following (so it stays square): in the outline's own frame, its
/// neighbours keep lining up with it.
pub fn move_corner(outline: &[(f32, f32)], index: usize, to: Vec2) -> Vec<(f32, f32)> {
    let points: Vec<Vec2> = outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let count = points.len();
    if count < 4 || index >= count {
        return outline.to_vec();
    }
    let x = (points[1] - points[0]).normalize_or(Vec2::X);
    let y = x.perp();
    let local = |p: Vec2| Vec2::new((p - points[0]).dot(x), (p - points[0]).dot(y));
    let world = |p: Vec2| points[0] + x * p.x + y * p.y;
    let mut locals: Vec<Vec2> = points.iter().map(|&p| local(p)).collect();
    let target = local(to);
    let (before, after) = ((index + count - 1) % count, (index + 1) % count);
    // A wall along x keeps its y (so its other end takes the new y), and one along y its x.
    let along_x = |a: Vec2, b: Vec2| (b - a).x.abs() >= (b - a).y.abs();
    let (was_before, was_after) = (along_x(locals[before], locals[index]), along_x(locals[index], locals[after]));
    locals[index] = target;
    if was_before {
        locals[before].y = target.y;
    } else {
        locals[before].x = target.x;
    }
    if was_after {
        locals[after].y = target.y;
    } else {
        locals[after].x = target.x;
    }
    // (Rounded to the centimetre, so it saves as tidy numbers.)
    let tidy = |value: f32| (value * 100.0).round() / 100.0;
    locals.into_iter().map(world).map(|p| (tidy(p.x), tidy(p.y))).collect()
}

/// A building's middle (its outline's corners' average).
pub fn middle(plan: &BuildingPlan) -> Vec2 {
    let sum: Vec2 = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).sum();
    sum / plan.outline.len().max(1) as f32
}

/// Moves a building (and everything in it) by `by`.
pub fn shift(plan: &mut BuildingPlan, by: Vec2) {
    let moved = |(x, y): (f32, f32)| (x + by.x, y + by.y);
    plan.outline = plan.outline.iter().map(|&p| moved(p)).collect();
    plan.inner_walls = plan.inner_walls.iter().map(|&(a, b)| (moved(a), moved(b))).collect();
}

/// Turns a building (and everything in it) by `angle` (radians) about its middle.
pub fn turn(plan: &mut BuildingPlan, angle: f32) {
    let centre = middle(plan);
    let (sin, cos) = angle.sin_cos();
    let turned = |(x, y): (f32, f32)| {
        let d = Vec2::new(x, y) - centre;
        let p = centre + Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
        ((p.x * 100.0).round() / 100.0, (p.y * 100.0).round() / 100.0)
    };
    plan.outline = plan.outline.iter().map(|&p| turned(p)).collect();
    plan.inner_walls = plan.inner_walls.iter().map(|&(a, b)| (turned(a), turned(b))).collect();
}

/// The wall of a building nearest a point (within `reach`): which, and how
/// far along it (0 to 1).
pub fn nearest_wall(plan: &BuildingPlan, point: Vec2, reach: f32) -> Option<(WallRef, f32)> {
    let points: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let mut walls: Vec<(WallRef, Vec2, Vec2)> =
        (0..points.len()).map(|i| (WallRef::Outer(i as u32), points[i], points[(i + 1) % points.len()])).collect();
    walls.extend(
        plan.inner_walls
            .iter()
            .enumerate()
            .map(|(i, &(a, b))| (WallRef::Inner(i as u32), Vec2::from(a), Vec2::from(b))),
    );
    walls
        .into_iter()
        .map(|(wall, a, b)| {
            let run = b - a;
            let t = ((point - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (wall, t, (a + run * t).distance(point))
        })
        .filter(|&(_, _, distance)| distance <= reach)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(wall, t, _)| (wall, (t * 100.0).round() / 100.0))
}

/// An inner wall from `from` towards `to`, squared up to the building (it
/// runs along whichever of its walls' directions is nearer).
pub fn square_inner_wall(plan: &BuildingPlan, from: Vec2, to: Vec2) -> (Vec2, Vec2) {
    let points: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let x = (points[1] - points[0]).normalize_or(Vec2::X);
    let y = x.perp();
    let run = to - from;
    let end = if run.dot(x).abs() >= run.dot(y).abs() { from + x * run.dot(x) } else { from + y * run.dot(y) };
    (from, end)
}

/// Adds a new building with this outline. Returns its id.
fn add_building(document: &mut Document, style: &str, outline: &[Vec2], now: f64) -> Option<String> {
    let (collection, plans) = document.find_mut::<BuildingPlan>()?;
    let taken: Vec<String> = plans.files().iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect();
    let id = (1..).map(|n| format!("building_{n}")).find(|id| !taken.contains(id))?;
    let outline: Vec<(f32, f32)> = outline.iter().map(|p| (snap(p.x), snap(p.y))).collect();
    // A door in the middle of its longest wall.
    let longest = (0..outline.len())
        .max_by(|&a, &b| {
            let length = |i: usize| Vec2::from(outline[i]).distance(Vec2::from(outline[(i + 1) % outline.len()]));
            length(a).total_cmp(&length(b))
        })
        .unwrap_or(0);
    let plan = BuildingPlan {
        id: id.clone(),
        style: style.to_string(),
        floors: 1,
        seed: new_seed(now),
        roof: RoofShape::Gable,
        chimney: None,
        outline,
        inner_walls: Vec::new(),
        doors: vec![DoorPlan { wall: WallRef::Outer(longest as u32), along: 0.5 }],
    };
    plans.files_mut().first_mut()?.entries.push(plan);
    document.changed(collection, 0, now);
    Some(id)
}

/// Bakes the document's buildings (with a handle being dragged where it is
/// now, and leaving out the one whose plan's being edited) and shows them,
/// whenever they change.
fn show_buildings(
    document: Res<Document>,
    generator: Res<WallGenerator>,
    tool: Res<BuildingTool>,
    mut shown: ResMut<ShownBuildings>,
    mut last: Local<Option<Vec<BuildingPlan>>>,
) {
    let Some(world) = generator.world.as_ref() else { return };
    if !document.is_changed() && !tool.is_changed() && !generator.is_changed() {
        return;
    }
    let mut plans = building_plans(&document);
    if let (Some(id), Some(to)) = (&tool.chosen, tool.dragged_to)
        && tool.editing.is_none()
        && let Some(plan) = plans.iter_mut().find(|plan| &plan.id == id)
    {
        shift(plan, to - middle(plan));
    }
    if let Some(id) = &tool.chosen
        && tool.editing.is_some()
    {
        plans.retain(|plan| &plan.id != id);
    }
    if last.as_ref() == Some(&plans) && shown.authored {
        return;
    }
    shown.buildings = plans.iter().map(|plan| bake_building(plan, |x, y| world.ground_height(x, y))).collect();
    shown.authored = true;
    *last = Some(plans);
}

/// The chosen building (with a corner being dragged where it is now).
fn chosen_plan(document: &Document, tool: &BuildingTool) -> Option<BuildingPlan> {
    let id = tool.chosen.as_ref()?;
    let mut plan = building_plans(document).into_iter().find(|plan| &plan.id == id)?;
    if let (Some((Some(index), _)), Some(to)) = (tool.pressed, tool.dragged_to) {
        plan.outline = move_corner(&plan.outline, index, to);
    }
    Some(plan)
}

/// Clicks for buildings that come before others': drawing, handles, and
/// the plan editor's tools. They take the click.
fn building_clicks(
    time: Res<Time>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
    store: Res<ChunkStore>,
    mut tool: ResMut<BuildingTool>,
    mut selection: ResMut<Selection>,
    mut walls: ResMut<WallTool>,
    mut document: ResMut<Document>,
) {
    let now = time.elapsed_secs_f64();
    let pointer = pointer_ray(&window, &view, *camera);
    let ground = |ray: Ray3d| raycast_ground(&store, ray).map(|at| Vec2::from(tidy(at)));

    if buttons.just_pressed(MouseButton::Left) && !egui.wants_pointer_input() {
        let Some((ray, cursor)) = pointer else { return };
        // Drawing a new one: each click a corner, squared up; the first again closes it.
        if let Some(corners) = tool.drawing.clone() {
            buttons.clear_just_pressed(MouseButton::Left);
            let Some(at) = ground(ray) else { return };
            let closing = corners.len() >= 3 && corners[0].distance(at) < 1.0;
            if closing {
                finish_drawing(&mut tool, &mut document, now);
            } else {
                let corner = square_up(&corners, at);
                if let Some(drawing) = tool.drawing.as_mut() {
                    drawing.push(Vec2::new(snap(corner.x), snap(corner.y)));
                }
            }
            selection.0 = None;
            walls.choose(None);
            return;
        }
        let Some(plan) = chosen_plan(&document, &tool) else { return };
        let height = ground_height(&store, middle(&plan).x, middle(&plan).y).unwrap_or(0.0);
        let near = |point: Vec2| {
            ray_miss(ray, to_bevy(point.x, point.y, height + 0.3))
                .is_some_and(|(miss, along)| miss < HANDLE_RADIUS + along * 0.004)
        };
        match tool.editing {
            None => {
                // The handle in its middle: move it.
                if near(middle(&plan)) {
                    buttons.clear_just_pressed(MouseButton::Left);
                    tool.pressed = Some((None, cursor));
                }
            }
            Some(PlanTool::Corners) => {
                let corner = plan.outline.iter().position(|&(x, y)| near(Vec2::new(x, y)));
                buttons.clear_just_pressed(MouseButton::Left);
                if let Some(index) = corner {
                    tool.pressed = Some((Some(index), cursor));
                }
            }
            Some(PlanTool::InnerWalls) => {
                buttons.clear_just_pressed(MouseButton::Left);
                let Some(at) = ground(ray) else { return };
                let at = Vec2::new(snap(at.x), snap(at.y));
                match tool.wall_start.take() {
                    None => tool.wall_start = Some(at),
                    Some(start) => {
                        let (from, to) = square_inner_wall(&plan, start, at);
                        if from.distance(to) > 0.5 {
                            let id = plan.id.clone();
                            change_building(&mut document, &id, now, |plan| {
                                plan.inner_walls.push(((snap(from.x), snap(from.y)), (snap(to.x), snap(to.y))));
                            });
                            tool.piece = Some(PlanPiece::InnerWall(plan.inner_walls.len()));
                        }
                    }
                }
            }
            Some(PlanTool::Doors) => {
                buttons.clear_just_pressed(MouseButton::Left);
                let Some(at) = ground(ray) else { return };
                if let Some((wall, along)) = nearest_wall(&plan, at, WALL_REACH) {
                    let id = plan.id.clone();
                    change_building(&mut document, &id, now, |plan| plan.doors.push(DoorPlan { wall, along }));
                    tool.piece = Some(PlanPiece::Door(plan.doors.len()));
                }
            }
            Some(PlanTool::Choose) => {
                buttons.clear_just_pressed(MouseButton::Left);
                let Some(at) = ground(ray) else { return };
                tool.piece = piece_at(&plan, at);
            }
        }
        return;
    }

    // Dragging a handle, and letting it go.
    let Some((handle, pressed_at)) = tool.pressed else { return };
    if buttons.pressed(MouseButton::Left) {
        let Some((ray, cursor)) = pointer else { return };
        if tool.dragged_to.is_none() && cursor.distance(pressed_at) < DRAG_START {
            return;
        }
        if let Some(at) = ground(ray) {
            tool.dragged_to = Some(Vec2::new(snap(at.x), snap(at.y)));
        }
        return;
    }
    tool.pressed = None;
    let (Some(to), Some(id)) = (tool.dragged_to.take(), tool.chosen.clone()) else { return };
    change_building(&mut document, &id, now, |plan| match handle {
        None => shift(plan, to - middle(plan)),
        Some(index) => plan.outline = move_corner(&plan.outline, index, to),
    });
}

/// The door or inner wall of a plan nearest a point (within reach).
fn piece_at(plan: &BuildingPlan, at: Vec2) -> Option<PlanPiece> {
    let door = plan.doors.iter().enumerate().filter_map(|(i, door)| {
        let (a, b) = match door.wall {
            WallRef::Outer(n) => {
                let n = n as usize;
                (plan.outline.get(n)?, plan.outline.get((n + 1) % plan.outline.len())?)
            }
            WallRef::Inner(n) => {
                let wall = plan.inner_walls.get(n as usize)?;
                (&wall.0, &wall.1)
            }
        };
        let place = Vec2::from(*a).lerp(Vec2::from(*b), door.along);
        Some((PlanPiece::Door(i), place.distance(at)))
    });
    let walls = plan.inner_walls.iter().enumerate().map(|(i, &(a, b))| {
        let (a, b) = (Vec2::from(a), Vec2::from(b));
        let run = b - a;
        let t = ((at - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
        (PlanPiece::InnerWall(i), (a + run * t).distance(at))
    });
    door.chain(walls)
        .filter(|&(_, distance)| distance <= WALL_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(piece, _)| piece)
}

/// Finishes drawing: a closed outline becomes a building (chosen); one
/// that can't close is let go.
pub fn finish_drawing(tool: &mut BuildingTool, document: &mut Document, now: f64) {
    let Some(corners) = tool.drawing.take() else { return };
    match close(&corners) {
        Some(outline) => {
            let style = if tool.new_style.is_empty() {
                styles(document).first().map(|style| style.id.clone()).unwrap_or_default()
            } else {
                tool.new_style.clone()
            };
            let id = add_building(document, &style, &outline, now);
            tool.status = String::new();
            tool.choose(id);
        }
        None => tool.status = "A building needs at least three corners (the last is found for you).".to_string(),
    }
}

/// A click that chose nothing else (no placed thing, no wall) chooses the
/// building under the pointer, or no building.
fn pick_building(
    buttons: Res<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
    selection: Res<Selection>,
    walls: Res<WallTool>,
    shown: Res<ShownBuildings>,
    mut tool: ResMut<BuildingTool>,
) {
    if !buttons.just_pressed(MouseButton::Left) || egui.wants_pointer_input() {
        return;
    }
    let Some((ray, _)) = pointer_ray(&window, &view, *camera) else { return };
    if selection.0.is_some() || walls.chosen.is_some() {
        tool.choose(None);
        return;
    }
    // Along the ray, the first point inside a building (up to its roof, roughly).
    let picked = (0..800).find_map(|step| {
        let point = ray.origin + *ray.direction * (step as f32 * 0.25);
        shown.buildings.iter().find_map(|building| {
            let outline: Vec<Vec2> = building.plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
            let height = building.plan.floors as f32 * 3.0 + 3.0;
            let inside = contains(&outline, Vec2::new(point.x, point.z));
            (inside && point.y >= building.floor - 0.5 && point.y <= building.floor + height)
                .then(|| building.plan.id.clone())
        })
    });
    tool.choose(picked);
}

/// Enter closes a building being drawn, Escape stops drawing (or leaves a
/// plan), and Delete removes what's chosen in a plan.
fn building_keys(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    mut tool: ResMut<BuildingTool>,
    mut document: ResMut<Document>,
    mut camera: Single<&mut EditorCamera>,
) {
    if egui.wants_keyboard_input() || view.0.is_none() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if tool.drawing.is_some() {
        if keys.just_pressed(KeyCode::Enter) {
            finish_drawing(&mut tool, &mut document, now);
        } else if keys.just_pressed(KeyCode::Escape) {
            tool.drawing = None;
        }
        return;
    }
    if tool.editing.is_some() && keys.just_pressed(KeyCode::Escape) {
        leave_plan(&mut tool, &mut camera);
        return;
    }
    if !keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        return;
    }
    let (Some(id), Some(piece)) = (tool.chosen.clone(), tool.piece.take()) else { return };
    change_building(&mut document, &id, now, |plan| match piece {
        PlanPiece::Door(i) if i < plan.doors.len() => {
            plan.doors.remove(i);
        }
        PlanPiece::InnerWall(i) if i < plan.inner_walls.len() => {
            plan.inner_walls.remove(i);
            // Its doors go with it, and later walls' doors move up.
            plan.doors.retain(|door| door.wall != WallRef::Inner(i as u32));
            for door in &mut plan.doors {
                if let WallRef::Inner(n) = &mut door.wall
                    && *n as usize > i
                {
                    *n -= 1;
                }
            }
        }
        _ => {}
    });
}

/// Starts editing the chosen building's plan: looking straight down on it.
pub fn enter_plan(tool: &mut BuildingTool, document: &Document, camera: &mut EditorCamera) {
    let Some(plan) = chosen_plan(document, tool) else { return };
    tool.camera_before = Some((camera.focus, camera.yaw, camera.pitch, camera.distance));
    tool.editing = Some(PlanTool::Corners);
    tool.piece = None;
    let centre = middle(&plan);
    let size = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y).distance(centre)).fold(0.0, f32::max);
    camera.go_to(to_bevy(centre.x, centre.y, camera.focus.y));
    camera.pitch = 1.45;
    camera.distance = (size * 3.2).max(12.0);
}

/// Stops editing a plan (the building's made again), and puts the camera back.
pub fn leave_plan(tool: &mut BuildingTool, camera: &mut EditorCamera) {
    tool.editing = None;
    tool.piece = None;
    tool.wall_start = None;
    if let Some((focus, yaw, pitch, distance)) = tool.camera_before.take() {
        camera.go_to(focus);
        camera.yaw = yaw;
        camera.pitch = pitch;
        camera.distance = distance;
    }
}

/// Draws a building being drawn, the chosen building's outline and handle,
/// and in its plan, its corners, inner walls and doors.
fn draw_building_guides(
    mut gizmos: Gizmos,
    tool: Res<BuildingTool>,
    document: Res<Document>,
    store: Res<ChunkStore>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
) {
    if view.0.is_none() {
        return;
    }
    let at = |p: Vec2, lift: f32| to_bevy(p.x, p.y, ground_height(&store, p.x, p.y).unwrap_or(0.0) + lift);
    let gold = Color::srgb(1.0, 0.85, 0.3);
    if let Some(corners) = &tool.drawing {
        let pointer = pointer_ray(&window, &view, *camera).and_then(|(ray, _)| raycast_ground(&store, ray));
        let mut line: Vec<Vec3> = corners.iter().map(|&p| at(p, 0.15)).collect();
        if let Some(pointer) = pointer {
            let next = square_up(corners, Vec2::new(pointer.x, pointer.z));
            line.push(at(next, 0.15));
        }
        gizmos.linestrip(line, gold);
        for (i, &corner) in corners.iter().enumerate() {
            gizmos.sphere(Isometry3d::from_translation(at(corner, 0.3)), if i == 0 { 0.45 } else { 0.3 }, gold);
        }
        return;
    }
    let Some(plan) = chosen_plan(&document, &tool) else { return };
    let outline: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    let mut line: Vec<Vec3> = outline.iter().map(|&p| at(p, 0.1)).collect();
    line.extend(line.first().copied());
    gizmos.linestrip(line, Color::WHITE);
    let Some(editing) = tool.editing else {
        gizmos.sphere(Isometry3d::from_translation(at(middle(&plan), 0.3)), 0.45, gold);
        return;
    };
    // The plan: corners, inner walls and doors.
    let chosen = |piece: PlanPiece| tool.piece == Some(piece);
    if editing == PlanTool::Corners {
        for &corner in &outline {
            gizmos.sphere(Isometry3d::from_translation(at(corner, 0.3)), 0.35, gold);
        }
    }
    for (i, &(a, b)) in plan.inner_walls.iter().enumerate() {
        let colour = if chosen(PlanPiece::InnerWall(i)) { gold } else { Color::srgb(0.6, 0.8, 1.0) };
        gizmos.line(at(Vec2::from(a), 0.15), at(Vec2::from(b), 0.15), colour);
    }
    if let Some(start) = tool.wall_start {
        gizmos.sphere(Isometry3d::from_translation(at(start, 0.2)), 0.25, Color::srgb(0.6, 0.8, 1.0));
    }
    for (i, door) in plan.doors.iter().enumerate() {
        let (a, b) = match door.wall {
            WallRef::Outer(n) => {
                let n = n as usize % outline.len().max(1);
                (outline[n], outline[(n + 1) % outline.len()])
            }
            WallRef::Inner(n) => match plan.inner_walls.get(n as usize) {
                Some(&(a, b)) => (Vec2::from(a), Vec2::from(b)),
                None => continue,
            },
        };
        let place = a.lerp(b, door.along);
        let way = (b - a).normalize_or(Vec2::X) * 0.55;
        let colour = if chosen(PlanPiece::Door(i)) { gold } else { Color::srgb(0.9, 0.5, 0.3) };
        gizmos.line(at(place - way, 0.2), at(place + way, 0.2), colour);
        gizmos.sphere(Isometry3d::from_translation(at(place, 0.2)), 0.2, colour);
    }
}

/// The Buildings part of the World tab's left panel: adding, and the list
/// (near where the camera looks, if `nearby` is set). Returns a building to
/// look at, if one was clicked.
pub fn buildings_list(
    ui: &mut egui::Ui,
    tool: &mut BuildingTool,
    document: &mut Document,
    nearby: Option<f32>,
    focus: (f32, f32),
    now: f64,
) -> Option<(f32, f32)> {
    let mut go_to = None;
    let plans = building_plans(document);
    let styles = styles(document);
    if tool.new_style.is_empty()
        && let Some(first) = styles.first()
    {
        tool.new_style = first.id.clone();
    }
    let near = |plan: &BuildingPlan| {
        let centre = middle(plan);
        nearby.is_none_or(|metres| (centre.x - focus.0).hypot(centre.y - focus.1) <= metres)
            || tool.chosen.as_ref() == Some(&plan.id)
    };
    let shown: Vec<&BuildingPlan> = plans.iter().filter(|plan| near(plan)).collect();
    let count = if shown.len() == plans.len() {
        format!("{}", plans.len())
    } else {
        format!("{} of {}", shown.len(), plans.len())
    };
    egui::CollapsingHeader::new(format!("Buildings ({count})")).id_salt("buildings").default_open(true).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                if tool.drawing.is_some() {
                    if ui.button("Close it").on_hover_text("Or click its first corner, or press Enter").clicked() {
                        finish_drawing(tool, document, now);
                    }
                    if ui.button("Cancel").clicked() {
                        tool.drawing = None;
                    }
                } else {
                    if ui.button("Add").on_hover_text("Click its corners on the ground").clicked() {
                        tool.choose(None);
                        tool.drawing = Some(Vec::new());
                    }
                    let name = |id: &str| {
                        styles.iter().find(|style| style.id == id).map_or(id.to_string(), |style| style.name.clone())
                    };
                    egui::ComboBox::from_id_salt("new building style").selected_text(name(&tool.new_style)).show_ui(
                        ui,
                        |ui| {
                            for style in &styles {
                                ui.selectable_value(&mut tool.new_style, style.id.clone(), &style.name);
                            }
                        },
                    );
                }
            });
            if tool.drawing.is_some() {
                ui.weak("Click its corners on the ground (each is squared up). Click the first again to close it.");
            }
            if !tool.status.is_empty() {
                ui.label(&tool.status);
            }
            for plan in shown {
                if ui.selectable_label(tool.chosen.as_ref() == Some(&plan.id), &plan.id).clicked() {
                    tool.choose(Some(plan.id.clone()));
                    let centre = middle(plan);
                    go_to = Some((centre.x, centre.y));
                }
            }
        },
    );
    go_to
}

/// The chosen building's own controls, above its fields: its style, its
/// plan (and the plan's tools), a new seed, turning it, and deleting it.
pub fn building_buttons(
    ui: &mut egui::Ui,
    tool: &mut BuildingTool,
    document: &mut Document,
    camera: &mut EditorCamera,
    now: f64,
) {
    let Some(id) = tool.chosen.clone() else { return };
    let styles = styles(document);
    let Some(plan) = building_plans(document).into_iter().find(|plan| plan.id == id) else { return };
    ui.horizontal(|ui| {
        ui.label("Style");
        let name =
            styles.iter().find(|style| style.id == plan.style).map_or(plan.style.clone(), |style| style.name.clone());
        let mut style = plan.style.clone();
        egui::ComboBox::from_id_salt("building style").selected_text(name).show_ui(ui, |ui| {
            for each in &styles {
                ui.selectable_value(&mut style, each.id.clone(), &each.name);
            }
        });
        if style != plan.style {
            change_building(document, &id, now, |plan| plan.style = style);
        }
    });
    match tool.editing {
        None => {
            ui.horizontal(|ui| {
                if ui.button("Edit plan").on_hover_text("Its corners, inner walls and doors, from above").clicked() {
                    enter_plan(tool, document, camera);
                }
                if ui.button("New seed").on_hover_text("The same building, varied differently").clicked() {
                    let seed = new_seed(now);
                    change_building(document, &id, now, |plan| plan.seed = seed);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Turn");
                for (label, degrees) in [("⟲ 15°", -15.0f32), ("⟳ 15°", 15.0), ("90°", 90.0)] {
                    if ui.button(label).clicked() {
                        change_building(document, &id, now, |plan| turn(plan, degrees.to_radians()));
                    }
                }
                if ui.button("Delete building").clicked() {
                    delete_building(document, &id, now);
                    tool.choose(None);
                }
            });
            ui.weak("Drag the handle in its middle to move it.");
        }
        Some(mut editing) => {
            ui.horizontal(|ui| {
                for (mode, label) in [
                    (PlanTool::Corners, "Corners"),
                    (PlanTool::InnerWalls, "Inner walls"),
                    (PlanTool::Doors, "Doors"),
                    (PlanTool::Choose, "Choose"),
                ] {
                    ui.selectable_value(&mut editing, mode, label);
                }
            });
            if tool.editing != Some(editing) {
                tool.editing = Some(editing);
                tool.wall_start = None;
            }
            ui.weak(match editing {
                PlanTool::Corners => "Drag a corner: the walls either side follow, square.",
                PlanTool::InnerWalls => "Click where an inner wall starts, then where it ends.",
                PlanTool::Doors => "Click a wall (outer or inner) to put a door in it.",
                PlanTool::Choose => "Click a door or an inner wall, then press Delete to remove it.",
            });
            if ui.button("Done").on_hover_text("Or press Escape").clicked() {
                leave_plan(tool, camera);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_square_up_and_outlines_close() {
        let corners = vec![Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0)];
        // The third corner goes straight across from the second, however the pointer's off.
        assert_eq!(square_up(&corners, Vec2::new(8.7, 5.0)), Vec2::new(8.0, 5.0));
        let corners = vec![Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0), Vec2::new(8.0, 5.0)];
        let closed = close(&corners).unwrap();
        assert_eq!(closed.len(), 4);
        assert!(closed[3].distance(Vec2::new(0.0, 5.0)) < 1e-4, "{closed:?}");
        // An L, with its last corner found.
        let l = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 9.0)].map(|(x, y)| Vec2::new(x, y));
        let closed = close(&l).unwrap();
        assert_eq!(closed.last().copied(), Some(Vec2::new(0.0, 9.0)));
        assert!(close(&l[..2]).is_none());
    }

    #[test]
    fn corners_move_with_their_walls_square() {
        let outline = vec![(0.0, 0.0), (8.0, 0.0), (8.0, 5.0), (0.0, 5.0)];
        let moved = move_corner(&outline, 2, Vec2::new(10.0, 6.0));
        assert_eq!(moved, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)]);
    }

    #[test]
    fn buildings_move_turn_and_find_their_walls() {
        let mut plan = BuildingPlan {
            id: "b".into(),
            style: "s".into(),
            floors: 1,
            seed: 0,
            roof: RoofShape::Gable,
            chimney: None,
            outline: vec![(0.0, 0.0), (8.0, 0.0), (8.0, 4.0), (0.0, 4.0)],
            inner_walls: vec![((4.0, 0.0), (4.0, 4.0))],
            doors: Vec::new(),
        };
        assert_eq!(nearest_wall(&plan, Vec2::new(2.0, -0.3), 1.0), Some((WallRef::Outer(0), 0.25)));
        assert_eq!(nearest_wall(&plan, Vec2::new(4.2, 1.0), 1.0).map(|(wall, _)| wall), Some(WallRef::Inner(0)));
        shift(&mut plan, Vec2::new(10.0, 0.0));
        assert_eq!(plan.inner_walls[0].0, (14.0, 0.0));
        turn(&mut plan, std::f32::consts::FRAC_PI_2);
        assert!((middle(&plan) - Vec2::new(14.0, 2.0)).length() < 0.02, "it turns about its middle");
        let (from, to) = square_inner_wall(&plan, Vec2::new(14.0, 2.0), Vec2::new(14.3, 5.0));
        assert!((to - from).x.abs() < 1e-3 || (to - from).y.abs() < 1e-3, "square to the building");
    }
}
