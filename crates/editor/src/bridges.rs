//! Drawing bridges (see `docs/plan-bridges.md`): add one where the camera
//! looks, choose one (in the list, or by clicking it), and edit its path:
//! drag its points, click the ground to add one at the nearer end, and
//! Delete to remove the chosen one. Its kind, width, curve and seed are its
//! properties. It's built again when its path is left.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::egui;
use bevy_egui::input::EguiWantsInput;
use venture_client::common::coords::to_bevy;
use venture_client::world::bridges::ShownBridges;
use venture_client::world::query::{ground_height, raycast_ground};
use venture_client::world::source::WorldInfo;
use venture_client::world::streaming::ChunkStore;
use venture_structures::bridges::bake_bridge;
use venture_world::{BridgeKind, BridgePlan};

use crate::document::{Document, Entry};
use crate::places::Selection;
use crate::shell::ViewRect;
use crate::walls::{WallGenerator, WallTool, new_seed, pointer_ray, ray_miss, tidy};

/// Bridge editing.
pub struct BridgeToolsPlugin;

impl Plugin for BridgeToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BridgeTool>().add_systems(
            Update,
            (
                show_bridges,
                bridge_clicks.before(crate::places::PickPlaces).before(crate::walls::PickWalls),
                pick_bridge.after(crate::walls::PickWalls),
                (bridge_keys, draw_bridge_guides),
            ),
        );
    }
}

/// What's being done to bridges.
#[derive(Resource, Default)]
pub struct BridgeTool {
    /// The chosen bridge, by id.
    pub chosen: Option<String>,
    /// Whether its path is being edited.
    pub editing: bool,
    /// The chosen point of its path.
    pub point: Option<usize>,
    /// A point being dragged.
    dragging: Option<usize>,
}

impl BridgeTool {
    /// Chooses a bridge (or none), leaving its path.
    pub fn choose(&mut self, id: Option<String>) {
        if self.chosen != id {
            self.chosen = id;
            self.editing = false;
            self.point = None;
            self.dragging = None;
        }
    }
}

/// The bridges in the document.
pub fn bridge_plans(document: &Document) -> Vec<BridgePlan> {
    document
        .find::<BridgePlan>()
        .map(|(_, plans)| plans.files().iter().flat_map(|file| file.entries.iter().cloned()).collect())
        .unwrap_or_default()
}

fn change_bridge(document: &mut Document, id: &str, now: f64, change: impl FnOnce(&mut BridgePlan)) -> bool {
    let Some((collection, plans)) = document.find_mut::<BridgePlan>() else { return false };
    for (f, file) in plans.files_mut().iter_mut().enumerate() {
        if let Some(plan) = file.entries.iter_mut().find(|plan| plan.id == id) {
            change(plan);
            document.changed(collection, f, now);
            return true;
        }
    }
    false
}

/// Deletes a bridge.
pub fn delete_bridge(document: &mut Document, id: &str, now: f64) {
    let Some((collection, plans)) = document.find_mut::<BridgePlan>() else { return };
    for (f, file) in plans.files_mut().iter_mut().enumerate() {
        if let Some(index) = file.entries.iter().position(|plan| plan.id == id) {
            file.entries.remove(index);
            document.changed(collection, f, now);
            return;
        }
    }
}

/// Adds a stone bridge 12 m long, across where the camera looks (east to
/// west), and returns its id.
pub fn add_bridge_at(document: &mut Document, at: Vec2, now: f64) -> Option<String> {
    let (collection, plans) = document.find_mut::<BridgePlan>()?;
    let taken: Vec<String> = plans.files().iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect();
    let id = (1..).map(|n| format!("bridge_{n}")).find(|id| !taken.contains(id))?;
    let kind = BridgeKind::Stone;
    let plan = BridgePlan {
        id: id.clone(),
        kind,
        points: vec![tidy(Vec3::new(at.x - 6.0, 0.0, at.y)), tidy(Vec3::new(at.x + 6.0, 0.0, at.y))],
        width: kind.usual_width(),
        curve: kind.usual_curve(),
        seed: new_seed(now),
    };
    plans.files_mut().first_mut()?.entries.push(plan);
    document.changed(collection, 0, now);
    Some(id)
}

/// Shows the world's bridges placed over rivers (by the bake), and the
/// document's own, baked, except the one whose path is being edited
/// (built again when it's left).
fn show_bridges(
    document: Res<Document>,
    generator: Res<WallGenerator>,
    world: Option<Res<WorldInfo>>,
    tool: Res<BridgeTool>,
    mut shown: ResMut<ShownBridges>,
    mut last: Local<Option<Vec<BridgePlan>>>,
) {
    let Some(generator) = generator.world.as_ref() else { return };
    let world_changed = world.as_ref().is_some_and(|world| world.is_changed());
    if !document.is_changed() && !tool.is_changed() && !world_changed {
        return;
    }
    let mut plans = bridge_plans(&document);
    if tool.editing
        && let Some(id) = &tool.chosen
    {
        plans.retain(|plan| &plan.id != id);
    }
    if last.as_ref() == Some(&plans) && shown.authored && !world_changed {
        return;
    }
    let ids: Vec<&str> = plans.iter().map(|plan| plan.id.as_str()).collect();
    let placed = world
        .iter()
        .flat_map(|world| world.0.bridges.iter())
        .filter(|bridge| bridge.plan.id.starts_with("river_bridge_") && !ids.contains(&bridge.plan.id.as_str()))
        .cloned();
    let authored = plans.iter().map(|plan| bake_bridge(plan, |x, y| generator.ground_height(x, y)));
    shown.bridges = placed.chain(authored).collect();
    shown.authored = true;
    *last = Some(plans);
}

fn chosen_plan(document: &Document, tool: &BridgeTool) -> Option<BridgePlan> {
    let id = tool.chosen.as_ref()?;
    bridge_plans(document).into_iter().find(|plan| &plan.id == id)
}

/// While a bridge's path is edited: pressing a point chooses it and drags
/// it; clicking the ground adds a point at the nearer end.
#[allow(clippy::too_many_arguments)]
fn bridge_clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::EditorCamera>>,
    store: Res<ChunkStore>,
    time: Res<Time>,
    mut document: ResMut<Document>,
    mut tool: ResMut<BridgeTool>,
) {
    if !tool.editing {
        return;
    }
    let Some(plan) = chosen_plan(&document, &tool) else { return };
    let Some((ray, _)) = pointer_ray(&window, &view, *camera) else { return };
    let now = time.elapsed_secs_f64();
    if let Some(index) = tool.dragging {
        if !buttons.pressed(MouseButton::Left) {
            tool.dragging = None;
        } else if let Some(ground) = raycast_ground(&store, ray) {
            let to = tidy(ground);
            if plan.points.get(index) != Some(&to) {
                change_bridge(&mut document, &plan.id, now, |plan| plan.points[index] = to);
            }
        }
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) || egui.wants_pointer_input() {
        return;
    }
    let handle = plan.points.iter().position(|&(x, y)| {
        let at = to_bevy(x, y, ground_height(&store, x, y).unwrap_or(0.0) + 0.3);
        ray_miss(ray, at).is_some_and(|(miss, _)| miss < 0.6)
    });
    if let Some(index) = handle {
        tool.point = Some(index);
        tool.dragging = Some(index);
        return;
    }
    let Some(ground) = raycast_ground(&store, ray) else { return };
    let at = tidy(ground);
    let near_start = plan.points.first().zip(plan.points.last()).is_some_and(|(&first, &last)| {
        let distance = |p: (f32, f32)| (p.0 - at.0).hypot(p.1 - at.1);
        distance(first) < distance(last)
    });
    let mut added = None;
    change_bridge(&mut document, &plan.id, now, |plan| {
        if near_start {
            plan.points.insert(0, at);
            added = Some(0);
        } else {
            plan.points.push(at);
            added = Some(plan.points.len() - 1);
        }
    });
    tool.point = added;
}

/// Clicking a bridge (when nothing else is chosen) chooses it.
fn pick_bridge(
    buttons: Res<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::EditorCamera>>,
    selection: Res<Selection>,
    walls: Res<WallTool>,
    buildings: Res<crate::buildings::BuildingTool>,
    document: Res<Document>,
    shown: Res<ShownBridges>,
    mut tool: ResMut<BridgeTool>,
) {
    if tool.editing || !buttons.just_pressed(MouseButton::Left) || egui.wants_pointer_input() {
        return;
    }
    let Some((ray, _)) = pointer_ray(&window, &view, *camera) else { return };
    if selection.0.is_some() || walls.chosen.is_some() || buildings.chosen.is_some() {
        tool.choose(None);
        return;
    }
    let ids: Vec<String> = bridge_plans(&document).into_iter().map(|plan| plan.id).collect();
    let picked = shown
        .bridges
        .iter()
        .filter(|bridge| ids.contains(&bridge.plan.id))
        .filter_map(|bridge| {
            let reach = bridge.plan.width / 2.0 + 0.5;
            bridge
                .deck
                .iter()
                .filter_map(|&[x, y, height]| ray_miss(ray, to_bevy(x, y, height)).filter(|(miss, _)| *miss < reach))
                .map(|(_, along)| along)
                .min_by(|a, b| a.total_cmp(b))
                .map(|along| (along, bridge.plan.id.clone()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id);
    tool.choose(picked);
}

/// Delete (or Backspace) removes the chosen point of a path being edited
/// (keeping at least two); Escape leaves the path.
fn bridge_keys(
    keys: Res<ButtonInput<KeyCode>>,
    egui: Res<EguiWantsInput>,
    time: Res<Time>,
    mut document: ResMut<Document>,
    mut tool: ResMut<BridgeTool>,
) {
    if !tool.editing || egui.wants_keyboard_input() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        tool.editing = false;
        tool.point = None;
        return;
    }
    if !keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        return;
    }
    let (Some(id), Some(index)) = (tool.chosen.clone(), tool.point) else { return };
    change_bridge(&mut document, &id, time.elapsed_secs_f64(), |plan| {
        if plan.points.len() > 2 && index < plan.points.len() {
            plan.points.remove(index);
        }
    });
    tool.point = None;
}

/// The chosen bridge's path (and, while it's edited, its points and deck).
fn draw_bridge_guides(
    mut gizmos: Gizmos,
    tool: Res<BridgeTool>,
    document: Res<Document>,
    generator: Res<WallGenerator>,
    store: Res<ChunkStore>,
) {
    let Some(plan) = chosen_plan(&document, &tool) else { return };
    let at = |(x, y): (f32, f32), lift: f32| to_bevy(x, y, ground_height(&store, x, y).unwrap_or(0.0) + lift);
    let gold = Color::srgb(1.0, 0.85, 0.3);
    gizmos.linestrip(plan.points.iter().map(|&p| at(p, 0.2)), if tool.editing { gold } else { Color::WHITE });
    if !tool.editing {
        return;
    }
    for (i, &point) in plan.points.iter().enumerate() {
        let colour = if tool.point == Some(i) { Color::WHITE } else { gold };
        gizmos.sphere(Isometry3d::from_translation(at(point, 0.3)), 0.35, colour);
    }
    // Where its deck will be.
    if let Some(world) = generator.world.as_ref() {
        let baked = bake_bridge(&plan, |x, y| world.ground_height(x, y));
        gizmos.linestrip(baked.deck.iter().map(|&[x, y, height]| to_bevy(x, y, height)), Color::srgb(0.5, 0.8, 1.0));
    }
}

/// The bridges, in the World tab's list (those within `nearby` metres of
/// `focus`, or all). Returns where to take the camera, if one was chosen.
pub fn bridges_list(
    ui: &mut egui::Ui,
    tool: &mut BridgeTool,
    document: &Document,
    nearby: Option<f32>,
    focus: (f32, f32),
) -> Option<Vec2> {
    let plans = bridge_plans(document);
    let middle = |plan: &BridgePlan| {
        let count = plan.points.len().max(1) as f32;
        let (x, y) = plan.points.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
        Vec2::new(x / count, y / count)
    };
    let shown: Vec<&BridgePlan> = plans
        .iter()
        .filter(|plan| {
            nearby.is_none_or(|metres| middle(plan).distance(Vec2::from(focus)) <= metres)
                || tool.chosen.as_ref() == Some(&plan.id)
        })
        .collect();
    let mut go_to = None;
    egui::CollapsingHeader::new(format!("Bridges ({})", shown.len())).id_salt("Bridges").default_open(true).show(
        ui,
        |ui| {
            for plan in shown {
                let chosen = tool.chosen.as_ref() == Some(&plan.id);
                if ui.selectable_label(chosen, format!("{} ({})", plan.id, plan.kind.name())).clicked() {
                    tool.choose(Some(plan.id.clone()));
                    go_to = Some(middle(plan));
                }
            }
        },
    );
    go_to
}

/// The chosen bridge's buttons: edit its path (or finish), and delete it.
pub fn bridge_buttons(ui: &mut egui::Ui, tool: &mut BridgeTool, document: &mut Document, now: f64) {
    let Some(id) = tool.chosen.clone() else { return };
    ui.heading(format!("Bridge: {id}"));
    ui.horizontal_wrapped(|ui| {
        let label = if tool.editing { "Done" } else { "Edit path" };
        if ui.button(label).on_hover_text("Drag its points; click the ground to add one at the nearer end").clicked() {
            tool.editing = !tool.editing;
            tool.point = None;
        }
        if ui.button("Delete").clicked() {
            delete_bridge(document, &id, now);
            tool.choose(None);
        }
    });
    if tool.editing {
        ui.weak("Drag a point to move it. Click the ground to add one at the nearer end.");
        ui.weak("Delete removes the chosen point. The bridge is built again when you press Done.");
    }
}
