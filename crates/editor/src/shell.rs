//! The editor's layout: tabs across the top (the world, then each kind of
//! content), the entries on the left, the chosen entry's properties in the
//! middle (or the 3D world, on the World tab), and problems at the bottom.
//!
//! Shortcuts: Ctrl+S (Cmd+S) saves, Ctrl+Z undoes, Ctrl+Shift+Z (or Ctrl+Y) redoes.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Viewport};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy_egui::{EguiContext, EguiGlobalSettings, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use venture_content::ContentError;

use crate::Project;
use crate::camera::EditorCamera;
use crate::document::Document;

/// How long after the last change the content is checked, in seconds.
const CHECK_DELAY: f64 = 0.4;

/// Lays out the editor.
pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Shell { check_due: Some(0.0), ..default() })
            .init_resource::<ViewRect>()
            .add_systems(Startup, spawn_interface_camera)
            .add_systems(EguiPrimaryContextPass, shell_ui)
            .add_systems(Update, check_when_due);
    }
}

/// Where the 3D view is in the window (logical pixels), while it's showing.
#[derive(Resource, Default)]
pub struct ViewRect(pub Option<Rect>);

/// Which tab is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// The world, in 3D.
    #[default]
    World,
    /// A kind of content (by its place in [`Document::collections`]).
    Content(usize),
}

/// What the editor's layout remembers.
#[derive(Resource, Default)]
pub struct Shell {
    /// The tab showing.
    pub tab: Tab,
    /// The chosen entry on each content tab: (file, entry).
    chosen: std::collections::HashMap<usize, (usize, usize)>,
    /// What the last check found.
    problems: Vec<ContentError>,
    /// When to check the content next (seconds of app time), if a check is due.
    check_due: Option<f64>,
    /// The last thing that happened, for the status line.
    status: String,
}

impl Shell {
    /// Chooses an entry on a content tab.
    pub fn choose(&mut self, collection: usize, file: usize, entry: usize) {
        self.chosen.insert(collection, (file, entry));
    }
}

/// The interface has a camera of its own, covering the whole window, drawn
/// over the 3D view (whose camera only draws in the middle).
fn spawn_interface_camera(mut commands: Commands, mut settings: ResMut<EguiGlobalSettings>) {
    settings.auto_create_primary_context = false;
    commands.spawn((
        Name::new("Interface camera"),
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

/// Checks the content a moment after it changes.
fn check_when_due(time: Res<Time>, mut shell: ResMut<Shell>, document: Res<Document>, project: Res<Project>) {
    if shell.check_due.is_some_and(|due| time.elapsed_secs_f64() >= due) {
        shell.check_due = None;
        shell.problems = document.check(&project.mirror);
    }
}

fn shell_ui(world: &mut World) {
    let Ok(context) = world.query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>().single_mut(world) else {
        return;
    };
    let mut context = context.clone();
    let ctx = context.get_mut().clone();
    // Everything is laid out inside one background area; what's left in the
    // middle on the World tab is the 3D view.
    let mut root = egui::Ui::new(
        ctx.clone(),
        "editor".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );
    let ui = &mut root;
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_scope(|world, mut shell: Mut<Shell>| {
        world.resource_scope(|world, mut document: Mut<Document>| {
            shortcuts(&ctx, &mut shell, &mut document, now);
            top_bar(ui, &mut shell, &mut document, now);
            problems_panel(ui, &mut shell, &document);
            match shell.tab {
                Tab::World => world_tab(ui, world),
                Tab::Content(collection) => {
                    entries_panel(ui, &mut shell, &mut document, collection, now);
                    properties_panel(ui, world, &mut shell, &mut document, collection, now);
                }
            }
        });
    });
    // The 3D view only draws on the World tab.
    let showing_world = world.resource::<Shell>().tab == Tab::World;
    if !showing_world {
        world.resource_mut::<ViewRect>().0 = None;
    }
    let mut cameras = world.query_filtered::<&mut Camera, With<EditorCamera>>();
    if let Ok(mut camera) = cameras.single_mut(world)
        && camera.is_active != showing_world
    {
        camera.is_active = showing_world;
    }
}

fn shortcuts(ctx: &egui::Context, shell: &mut Shell, document: &mut Document, now: f64) {
    let (save, undo, redo) = ctx.input(|input| {
        let command = input.modifiers.command;
        let shift = input.modifiers.shift;
        (
            command && input.key_pressed(egui::Key::S),
            command && !shift && input.key_pressed(egui::Key::Z),
            command && ((shift && input.key_pressed(egui::Key::Z)) || input.key_pressed(egui::Key::Y)),
        )
    });
    if save {
        save_all(shell, document);
    }
    if undo && let Some(place) = document.undo() {
        after_undo(shell, place, "Undone.", now);
    }
    if redo && let Some(place) = document.redo() {
        after_undo(shell, place, "Redone.", now);
    }
}

fn after_undo(shell: &mut Shell, (collection, file): (usize, usize), status: &str, now: f64) {
    shell.tab = Tab::Content(collection);
    let chosen = shell.chosen.entry(collection).or_insert((file, 0));
    chosen.0 = file;
    shell.status = status.to_string();
    shell.check_due = Some(now + CHECK_DELAY);
}

fn save_all(shell: &mut Shell, document: &mut Document) {
    shell.status = match document.save() {
        Ok(0) => "Nothing to save.".to_string(),
        Ok(1) => "Saved 1 file.".to_string(),
        Ok(count) => format!("Saved {count} files."),
        Err(error) => format!("Couldn't save: {error}"),
    };
}

fn top_bar(root: &mut egui::Ui, shell: &mut Shell, document: &mut Document, now: f64) {
    egui::Panel::top("tabs").show_inside(root, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut shell.tab, Tab::World, "World");
            ui.separator();
            for (index, collection) in document.collections.iter().enumerate() {
                ui.selectable_value(&mut shell.tab, Tab::Content(index), collection.title());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let unsaved = document.unsaved();
                let label = if unsaved.is_empty() { "Save".to_string() } else { format!("Save ({})", unsaved.len()) };
                let save = ui.add_enabled(!unsaved.is_empty(), egui::Button::new(label));
                let save = save.on_hover_text(if unsaved.is_empty() {
                    "Everything is saved.".to_string()
                } else {
                    unsaved.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join("\n")
                });
                if save.clicked() {
                    save_all(shell, document);
                }
                if ui.add_enabled(document.can_redo(), egui::Button::new("Redo")).clicked()
                    && let Some(place) = document.redo()
                {
                    after_undo(shell, place, "Redone.", now);
                }
                if ui.add_enabled(document.can_undo(), egui::Button::new("Undo")).clicked()
                    && let Some(place) = document.undo()
                {
                    after_undo(shell, place, "Undone.", now);
                }
            });
        });
    });
}

fn problems_panel(root: &mut egui::Ui, shell: &mut Shell, document: &Document) {
    egui::Panel::bottom("problems").resizable(true).default_size(90.0).show_inside(root, |ui| {
        ui.horizontal(|ui| {
            if shell.check_due.is_some() {
                ui.label("Checking…");
            } else if shell.problems.is_empty() {
                ui.label("No problems.");
            } else {
                ui.colored_label(egui::Color32::from_rgb(240, 120, 100), format!("Problems: {}", shell.problems.len()));
            }
            ui.separator();
            ui.label(&shell.status);
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut go_to = None;
            for problem in &shell.problems {
                if ui.link(problem.to_string()).clicked() {
                    go_to = find_problem(document, problem);
                }
            }
            if let Some((collection, file, entry)) = go_to {
                shell.tab = Tab::Content(collection);
                shell.chosen.insert(collection, (file, entry));
            }
        });
    });
}

/// Where a problem is: its file's (collection, file), and the entry it
/// names in quotes, if any.
fn find_problem(document: &Document, problem: &ContentError) -> Option<(usize, usize, usize)> {
    for (c, collection) in document.collections.iter().enumerate() {
        if let Some(f) = collection.paths().iter().position(|path| path == &problem.file) {
            let keys = collection.keys(f);
            let entry = keys.iter().position(|key| problem.message.contains(&format!("\"{key}\""))).unwrap_or(0);
            return Some((c, f, entry));
        }
    }
    None
}

fn entries_panel(root: &mut egui::Ui, shell: &mut Shell, document: &mut Document, collection: usize, now: f64) {
    let (mut file, mut entry) = shell.chosen.get(&collection).copied().unwrap_or((0, 0));
    let mut changed = None;
    let unsaved: Vec<bool> =
        (0..document.collections[collection].paths().len()).map(|f| document.is_unsaved(collection, f)).collect();
    egui::Panel::left("entries").resizable(true).default_size(230.0).show_inside(root, |ui| {
        let kind = &mut document.collections[collection];
        if !kind.is_single() {
            ui.horizontal(|ui| {
                if ui.button("Copy").on_hover_text("Add a copy of this, just after it").clicked()
                    && let Some(copy) = kind.duplicate(file, entry)
                {
                    entry = copy;
                    changed = Some(file);
                }
                if ui.button("Delete").clicked() && entry < kind.keys(file).len() {
                    kind.remove(file, entry);
                    entry = entry.saturating_sub(1);
                    changed = Some(file);
                }
                if ui.button("⏶").on_hover_text("Move up").clicked()
                    && let Some(to) = kind.shift(file, entry, -1)
                {
                    entry = to;
                    changed = Some(file);
                }
                if ui.button("⏷").on_hover_text("Move down").clicked()
                    && let Some(to) = kind.shift(file, entry, 1)
                {
                    entry = to;
                    changed = Some(file);
                }
            });
            ui.separator();
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (f, path) in kind.paths().iter().enumerate() {
                let name = path.display().to_string();
                let name = if unsaved[f] { format!("{name} •") } else { name };
                egui::CollapsingHeader::new(name).id_salt(f).default_open(true).show(ui, |ui| {
                    for (index, key) in kind.keys(f).iter().enumerate() {
                        if ui.selectable_label((file, entry) == (f, index), key).clicked() {
                            (file, entry) = (f, index);
                        }
                    }
                });
            }
        });
    });
    if let Some(f) = changed {
        document.changed(collection, f, now);
        shell.check_due = Some(now + CHECK_DELAY);
    }
    shell.chosen.insert(collection, (file, entry));
}

fn properties_panel(
    root: &mut egui::Ui,
    world: &mut World,
    shell: &mut Shell,
    document: &mut Document,
    collection: usize,
    now: f64,
) {
    let (file, entry) = shell.chosen.get(&collection).copied().unwrap_or((0, 0));
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let mut changed = false;
    egui::CentralPanel::default().show_inside(root, |ui| {
        let unsaved = document.is_unsaved(collection, file);
        let kind = &mut document.collections[collection];
        let Some(path) = kind.paths().get(file).cloned() else {
            ui.label("Nothing here yet.");
            return;
        };
        let key = kind.keys(file).get(entry).cloned().unwrap_or_default();
        ui.heading(&key);
        ui.label(format!("{}{}", path.display(), if unsaved { " (unsaved)" } else { "" }));
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            // (Wide enough for ids and names inside lists.)
            ui.spacing_mut().text_edit_width = 320.0;
            if let Some(value) = kind.entry_mut(file, entry) {
                changed = bevy_inspector_egui::reflect_inspector::ui_for_value(value, ui, &registry);
            }
        });
    });
    if changed {
        document.changed(collection, file, now);
        shell.check_due = Some(now + CHECK_DELAY);
    }
}

/// The World tab: the 3D view fills the middle. (Placing things arrives in
/// milestone 1.)
fn world_tab(root: &mut egui::Ui, world: &mut World) {
    egui::Panel::left("places").resizable(true).default_size(230.0).show_inside(root, |ui| {
        ui.heading("Places");
        ui.label("Placing NPCs, spawners and respawn points comes next.");
        ui.separator();
        ui.label("Right drag: turn");
        ui.label("Middle drag (or Shift + right drag): slide");
        ui.label("Scroll: closer / further");
        ui.label("W A S D: move (Shift: faster)");
    });
    // The camera draws only in what's left between the panels (and only
    // takes the mouse there).
    let rect = root.available_rect_before_wrap();
    let scale = root.ctx().pixels_per_point();
    world.resource_mut::<ViewRect>().0 = Some(Rect::new(rect.min.x, rect.min.y, rect.max.x, rect.max.y));
    let mut cameras = world.query_filtered::<&mut Camera, With<EditorCamera>>();
    if let Ok(mut camera) = cameras.single_mut(world) {
        // (Kept inside what the camera draws to, which can briefly disagree
        // with the window, such as while it's resized.)
        let Some(target) = camera.physical_target_size() else { return };
        let position = UVec2::new((rect.min.x * scale) as u32, (rect.min.y * scale) as u32).min(target - UVec2::ONE);
        let size = UVec2::new((rect.width() * scale).max(1.0) as u32, (rect.height() * scale).max(1.0) as u32)
            .min(target - position)
            .max(UVec2::ONE);
        let viewport = Some(Viewport { physical_position: position, physical_size: size, ..default() });
        if camera.viewport.as_ref().map(|v| (v.physical_position, v.physical_size)) != Some((position, size)) {
            camera.viewport = viewport;
        }
    }
}
