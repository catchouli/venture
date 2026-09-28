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
use venture_client::common::coords::{from_bevy, to_bevy};

use crate::buildings::{BuildingTool, building_buttons, buildings_list};
use crate::camera::EditorCamera;
use crate::document::Document;
use crate::fields::{Choices, Fields, referred_as, rename_everywhere, users};
use crate::grips::{appearance_panel, grip_panel};
use crate::places::{
    EntryRef, Place, PlaceKind, PlaceMarker, Places, Selection, add_place, copy_place, delete_place, marker_heights,
};
use crate::previews::{class_preview, monster_preview};
use crate::walls::{WallTool, baking, place_added_point, save_and_bake, wall_buttons, wall_defs, walls_list};
use venture_client::world::source::WorldInfo;

/// How long after the last change the content is checked, in seconds.
const CHECK_DELAY: f64 = 0.4;

/// Lays out the editor.
pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Shell { check_due: Some(0.0), nearby: Some(DEFAULT_NEARBY), ..default() })
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
    /// Whether content tabs show every entry in a table, rather than one entry.
    table: bool,
    /// The World tab lists only things this near (metres) where the camera
    /// looks, if set.
    nearby: Option<f32>,
}

/// How near things are listed on the World tab, to begin with (metres).
const DEFAULT_NEARBY: f32 = 100.0;

impl Shell {
    /// Shows content tabs as tables (or one entry at a time).
    pub fn show_tables(&mut self, table: bool) {
        self.table = table;
    }

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
                Tab::World => world_tab(ui, world, &mut shell, &mut document, now),
                Tab::Content(collection) => {
                    entries_panel(ui, &mut shell, &mut document, collection, now);
                    if document.collections[collection].title() == "Items" && !shell.table {
                        let (file, entry) = shell.chosen.get(&collection).copied().unwrap_or((0, 0));
                        if let Some(key) = document.collections[collection].keys(file).get(entry).cloned() {
                            grip_panel(ui, world, &mut document, &key, now);
                        }
                    }
                    if document.collections[collection].title() == "Appearances" && !shell.table {
                        let (file, entry) = shell.chosen.get(&collection).copied().unwrap_or((0, 0));
                        if let Some(key) = document.collections[collection].keys(file).get(entry).cloned() {
                            appearance_panel(ui, world, &key);
                        }
                    }
                    if shell.table && !document.collections[collection].is_single() {
                        table_panel(ui, world, &mut shell, &mut document, collection, now);
                    } else {
                        properties_panel(ui, world, &mut shell, &mut document, collection, now);
                    }
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
        after_undo(shell, document, place, "Undone.", now);
    }
    if redo && let Some(place) = document.redo() {
        after_undo(shell, document, place, "Redone.", now);
    }
}

fn after_undo(shell: &mut Shell, document: &Document, (collection, file): (usize, usize), status: &str, now: f64) {
    // (Walls are edited on the World tab, so it stays there for them.)
    if !(shell.tab == Tab::World && document.collections[collection].is_authored()) {
        shell.tab = Tab::Content(collection);
    }
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
        // Saving and undoing first, then the tabs (which wrap onto more rows when there are many).
        ui.horizontal_wrapped(|ui| {
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
            if ui.add_enabled(document.can_undo(), egui::Button::new("Undo")).clicked()
                && let Some(place) = document.undo()
            {
                after_undo(shell, document, place, "Undone.", now);
            }
            if ui.add_enabled(document.can_redo(), egui::Button::new("Redo")).clicked()
                && let Some(place) = document.redo()
            {
                after_undo(shell, document, place, "Redone.", now);
            }
            ui.separator();
            ui.selectable_value(&mut shell.tab, Tab::World, "World");
            ui.separator();
            for (index, collection) in document.collections.iter().enumerate() {
                ui.selectable_value(&mut shell.tab, Tab::Content(index), collection.title());
            }
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
                ui.selectable_value(&mut shell.table, false, "One").on_hover_text("Every field of one entry");
                ui.selectable_value(&mut shell.table, true, "Table").on_hover_text("The simple fields of every entry");
            });
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
    let mut changed = false;
    egui::CentralPanel::default().show_inside(root, |ui| {
        changed = edit_entry(ui, world, document, (collection, file, entry));
    });
    if changed {
        document.changed(collection, file, now);
        shell.check_due = Some(now + CHECK_DELAY);
    }
}

/// Every entry of a kind in a table: a row each, and a column for each
/// simple field (text, numbers, ticks and plain choices). Clicking an
/// entry's key shows all of it.
fn table_panel(
    root: &mut egui::Ui,
    world: &mut World,
    shell: &mut Shell,
    document: &mut Document,
    collection: usize,
    now: f64,
) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let mut changed_files = Vec::new();
    let mut open = None;
    let fields = Fields { registry: &registry, choices: world.resource::<Choices>() };
    egui::CentralPanel::default().show_inside(root, |ui| {
        let kind = &mut document.collections[collection];
        let files = kind.paths().len();
        // The columns: the first entry's simple fields.
        let columns: Vec<String> = (0..files).find_map(|f| kind.entry_mut(f, 0).map(simple_fields)).unwrap_or_default();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.spacing_mut().text_edit_width = 160.0;
            egui::Grid::new("table").striped(true).num_columns(columns.len() + 1).show(ui, |ui| {
                ui.strong("");
                for column in &columns {
                    ui.strong(column);
                }
                ui.end_row();
                for f in 0..files {
                    for (i, key) in kind.keys(f).iter().enumerate() {
                        if ui.link(key).on_hover_text("Show all of it").clicked() {
                            open = Some((f, i));
                        }
                        let Some(entry) = kind.entry_mut(f, i) else { continue };
                        let bevy::reflect::ReflectMut::Struct(record) = entry.reflect_mut() else { continue };
                        for column in &columns {
                            match record.field_mut(column) {
                                Some(value) if is_simple(value) => {
                                    ui.push_id((f, i, column), |ui| {
                                        if fields.edit(ui, value) {
                                            changed_files.push(f);
                                        }
                                    });
                                }
                                _ => {
                                    ui.weak("…");
                                }
                            }
                        }
                        ui.end_row();
                    }
                }
            });
        });
    });
    changed_files.dedup();
    for file in changed_files {
        document.changed(collection, file, now);
        shell.check_due = Some(now + CHECK_DELAY);
    }
    if let Some((file, entry)) = open {
        shell.choose(collection, file, entry);
        shell.table = false;
    }
}

/// The names of a struct's simple fields (see [`is_simple`]).
fn simple_fields(value: &mut dyn bevy::reflect::PartialReflect) -> Vec<String> {
    let bevy::reflect::ReflectMut::Struct(fields) = value.reflect_mut() else { return Vec::new() };
    let mut names = Vec::new();
    for index in 0..fields.field_len() {
        let name = fields.name_at(index).unwrap_or_default().to_string();
        if fields.field_at(index).is_some_and(is_simple) {
            names.push(name);
        }
    }
    names
}

/// Whether a value fits in a table cell: text, a number, a tick, or a choice
/// (or an optional one) with nothing more inside.
fn is_simple(value: &dyn bevy::reflect::PartialReflect) -> bool {
    use bevy::reflect::ReflectRef;
    match value.reflect_ref() {
        ReflectRef::Opaque(_) => true,
        ReflectRef::Enum(choice) => (0..choice.field_len())
            .all(|i| choice.field_at(i).is_some_and(|f| matches!(f.reflect_ref(), ReflectRef::Opaque(_)))),
        _ => false,
    }
}

/// Shows an entry's every field, to edit, with what uses it, renaming, and
/// previews for classes and monsters. Returns whether it changed (the
/// caller then tells the document); renames tell the document themselves.
fn edit_entry(ui: &mut egui::Ui, world: &World, document: &mut Document, entry: EntryRef) -> bool {
    let (c, f, i) = entry;
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let now = world.resource::<Time>().elapsed_secs_f64();
    let unsaved = document.is_unsaved(c, f);
    let Some(kind) = document.collections.get(c) else { return false };
    let title = kind.title();
    let Some(path) = kind.paths().get(f).cloned() else {
        ui.label("Nothing here yet.");
        return false;
    };
    let key = kind.keys(f).get(i).cloned().unwrap_or_default();
    ui.heading(&key);
    ui.label(format!("{}{}", path.display(), if unsaved { " (unsaved)" } else { "" }));
    identity(ui, document, entry, &key, title, now);
    ui.separator();
    let mut changed = false;
    let fields = Fields { registry: &registry, choices: world.resource::<Choices>() };
    egui::ScrollArea::vertical().show(ui, |ui| {
        if let Some(value) = document.collections[c].entry_mut(f, i) {
            changed = fields.edit(ui, value);
        }
        ui.add_space(12.0);
        previews(ui, document, entry, title);
    });
    changed
}

/// What uses an entry, and renaming it (everywhere it's used).
fn identity(ui: &mut egui::Ui, document: &mut Document, entry: EntryRef, key: &str, title: &str, now: f64) {
    let referred = referred_as(title);
    if let Some(kind) = referred {
        let users = users(document, kind, key);
        egui::CollapsingHeader::new(format!("Used by {}", users.len())).id_salt("used by").show(ui, |ui| {
            if users.is_empty() {
                ui.weak("Nothing uses it.");
            }
            for (_, label) in &users {
                ui.label(label);
            }
        });
    }
    if document.collections[entry.0].is_single() {
        return;
    }
    let id = ui.id().with(("rename", entry));
    let mut new = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_else(|| key.to_string());
    ui.horizontal(|ui| {
        ui.label("Rename to");
        ui.add(egui::TextEdit::singleline(&mut new).desired_width(200.0));
        let taken = (0..document.collections[entry.0].paths().len())
            .any(|file| document.collections[entry.0].keys(file).contains(&new));
        let can = !new.trim().is_empty() && new != key && !taken;
        let label = if referred.is_some() { "Rename everywhere" } else { "Rename" };
        if ui.add_enabled(can, egui::Button::new(label)).clicked() {
            rename_everywhere(document, entry, new.trim(), now);
            new = new.trim().to_string();
        }
    });
    ui.data_mut(|data| data.insert_temp(id, new));
    if matches!(title, "Classes" | "Items" | "Abilities" | "Skills" | "Quests") {
        ui.weak("Saved characters keep the old id: renaming one they have loses it for them.");
    }
}

/// Previews for classes and monsters.
fn previews(ui: &mut egui::Ui, document: &Document, (c, f, i): EntryRef, title: &str) {
    match title {
        "Classes" => {
            let Some((_, classes)) = document.find::<venture_content::ClassDef>() else { return };
            let Some(class) = classes.files().get(f).and_then(|file| file.entries.get(i)) else { return };
            let items = document
                .find::<venture_content::ItemDef>()
                .map(|(_, items)| {
                    items
                        .files()
                        .iter()
                        .flat_map(|file| &file.entries)
                        .map(|item| (item.id.clone(), item.clone()))
                        .collect()
                })
                .unwrap_or_default();
            class_preview(ui, class, &items);
        }
        "Monsters" => {
            let Some((_, monsters)) = document.find::<venture_content::MonsterDef>() else { return };
            if let Some(monster) = monsters.files().get(f).and_then(|file| file.entries.get(i)) {
                monster_preview(ui, monster);
            }
        }
        _ => {}
    }
    let _ = c;
}

/// The World tab: placed things on the left, the chosen one's properties on
/// the right, and the 3D view in the middle, with names over the markers.
fn world_tab(root: &mut egui::Ui, world: &mut World, shell: &mut Shell, document: &mut Document, now: f64) {
    let mut go_to = None;
    let mut action = None;
    let mut cameras = world.query::<&EditorCamera>();
    let focus = cameras.single(world).map(|camera| from_bevy(camera.focus)).unwrap_or_default();
    let mut wall_go_to = None;
    world.resource_scope(|world, mut buildings: Mut<BuildingTool>| {
    world.resource_scope(|world, mut tool: Mut<WallTool>| {
        world.resource_scope(|world, mut selection: Mut<Selection>| {
            let places = world.resource::<Places>();
            let chosen_place = places.list.iter().find(|place| selection.is(place)).cloned();
            egui::Panel::left("places").resizable(true).default_size(240.0).show_inside(root, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Add:");
                    for (kind, label) in [
                        (PlaceKind::Npc, "NPC"),
                        (PlaceKind::Spawner, "Spawner"),
                        (PlaceKind::RespawnPoint, "Respawn point"),
                    ] {
                        if ui.button(label).on_hover_text("Where the camera is looking").clicked() {
                            action = Some(WorldAction::Add(kind));
                        }
                    }
                });
                if let Some(place) = &chosen_place {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Copy").clicked() {
                            action = Some(WorldAction::Copy(place.clone()));
                        }
                        if ui.button("Delete").clicked() {
                            action = Some(WorldAction::Delete(place.clone()));
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let mut limited = shell.nearby.is_some();
                    ui.checkbox(&mut limited, "Only within")
                        .on_hover_text("List only things this near where the camera is looking");
                    let mut metres = shell.nearby.unwrap_or(DEFAULT_NEARBY);
                    ui.add_enabled(
                        limited,
                        egui::DragValue::new(&mut metres).range(1.0..=10000.0).speed(5.0).suffix(" m"),
                    );
                    shell.nearby = limited.then_some(metres);
                });
                let nearby = shell.nearby;
                let chosen = selection.clone();
                let near = |place: &Place| {
                    let (x, y) = place.position;
                    nearby.is_none_or(|metres| (x - focus.0).hypot(y - focus.1) <= metres) || chosen.is(place)
                };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for kind in PlaceKind::ALL {
                        let of_kind: Vec<&Place> = places.list.iter().filter(|place| place.kind == kind).collect();
                        let shown: Vec<&Place> = of_kind.iter().copied().filter(|place| near(place)).collect();
                        let count = match shown.len() == of_kind.len() {
                            true => format!("{}", of_kind.len()),
                            false => format!("{} of {}", shown.len(), of_kind.len()),
                        };
                        egui::CollapsingHeader::new(format!("{} ({count})", kind.title()))
                            .id_salt(kind.title())
                            .default_open(true)
                            .show(ui, |ui| {
                                for place in shown {
                                    if ui.selectable_label(selection.is(place), &place.label).clicked() {
                                        selection.0 = Some((place.kind, place.key.clone()));
                                        tool.choose(None);
                                        buildings.choose(None);
                                        go_to = Some(place.position);
                                    }
                                }
                            });
                    }
                    if let Some(at) = walls_list(ui, &mut tool, document, nearby, focus, now) {
                        selection.0 = None;
                        buildings.choose(None);
                        buildings.drawing = None;
                        wall_go_to = Some(at);
                    }
                    if let Some(at) = buildings_list(ui, &mut buildings, document, nearby, focus, now) {
                        selection.0 = None;
                        tool.choose(None);
                        wall_go_to = Some(at);
                    }
                    if buildings.drawing.is_some() {
                        tool.choose(None);
                    }
                    if tool.drawing || tool.chosen.is_some() || buildings.drawing.is_some() || buildings.chosen.is_some() {
                        selection.0 = None;
                    }
                    ui.separator();
                    if ui.button("Save and play here").on_hover_text(PLAY_HERE_HELP).clicked() {
                        action = Some(WorldAction::PlayHere);
                    }
                    let label = if baking(&tool) { "Baking…" } else { "Save and bake" };
                    if ui.add_enabled(!baking(&tool), egui::Button::new(label)).on_hover_text(BAKE_HELP).clicked() {
                        action = Some(WorldAction::Bake);
                    }
                    ui.separator();
                    ui.weak("Click a marker to choose it, and drag it to move it.");
                    ui.weak("Right drag: turn. Middle drag (or Shift + right drag): slide.");
                    ui.weak("Scroll: closer or further. W A S D: move (Shift: faster).");
                });
            });
            // The properties panel is always there (and keeps its width), so the view doesn't
            // change size as things are chosen; what's in it scrolls sideways rather than widening it.
            let chosen = places.list.iter().find(|place| selection.is(place)).map(|place| place.entry);
            let wall = if chosen.is_none() { chosen_wall(document, &tool) } else { None };
            let building = if chosen.is_none() && wall.is_none() { chosen_building(document, &buildings) } else { None };
            let before = wall
                .and_then(|_| tool.chosen.clone())
                .and_then(|id| wall_defs(document).into_iter().find(|def| def.id == id));
            let mut changed = None;
            egui::Panel::right("properties").resizable(true).default_size(380.0).show_inside(root, |ui| {
                egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| match (chosen, wall, building) {
                    (Some(entry), _, _) => {
                        if edit_entry(ui, world, document, entry) {
                            changed = Some(entry);
                        }
                    }
                    (None, Some(entry), _) => {
                        wall_buttons(ui, &mut tool, document, now);
                        ui.separator();
                        if edit_entry(ui, world, document, entry) {
                            changed = Some(entry);
                        }
                    }
                    (None, None, Some(entry)) => {
                        {
                            let mut cameras = world.query::<&mut EditorCamera>();
                            if let Ok(mut camera) = cameras.single_mut(world) {
                                building_buttons(ui, &mut buildings, document, &mut camera, now);
                            }
                        }
                        ui.separator();
                        if edit_entry(ui, world, document, entry) {
                            changed = Some(entry);
                        }
                    }
                    (None, None, None) => {
                        ui.heading("Nothing chosen");
                        ui.weak("Click something in the view, or in the list on the left, to see and change it here.");
                    }
                });
            });
            if let Some(entry) = changed {
                // A point added to a wall's list starts a little past its end, not on it.
                if let Some(before) = &before {
                    place_added_point(document, before);
                }
                document.changed(entry.0, entry.1, now);
                shell.check_due = Some(now + CHECK_DELAY);
            }
            // (Renaming a wall or building, or undoing, can leave the choice naming nothing.)
            if tool.chosen.is_some() && !tool.drawing && chosen_wall(document, &tool).is_none() {
                tool.choose(None);
            }
            if buildings.chosen.is_some() && chosen_building(document, &buildings).is_none() {
                buildings.choose(None);
            }
        });
    });
    });
    if wall_go_to.is_some() {
        shell.check_due = Some(now + CHECK_DELAY);
        go_to = wall_go_to;
    }
    if let Some((x, y)) = go_to {
        let mut cameras = world.query::<&mut EditorCamera>();
        if let Ok(mut camera) = cameras.single_mut(world) {
            camera.go_to(to_bevy(x, y, 0.0));
        }
    }
    if let Some(action) = action {
        world_action(world, shell, document, action, now);
    }

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
    name_markers(root, world, rect);
}

/// Something done from the World tab's buttons.
enum WorldAction {
    Add(PlaceKind),
    Copy(Place),
    Delete(Place),
    PlayHere,
    Bake,
}

const BAKE_HELP: &str = "Saves, then bakes the world again (in the background, about a second: only the chunks \
walls touch), with the walls as they are: they block the ground they stand on and clear trees from their way. \
A game or server that's running loads the new world by itself.";

/// The chosen building's entry in the document, if there is one.
fn chosen_building(document: &Document, tool: &BuildingTool) -> Option<EntryRef> {
    let id = tool.chosen.as_ref()?;
    let (collection, plans) = document.find::<venture_world::BuildingPlan>()?;
    plans
        .files()
        .iter()
        .enumerate()
        .find_map(|(f, file)| file.entries.iter().position(|plan| &plan.id == id).map(|i| (collection, f, i)))
}

/// The chosen wall's entry in the document, if there is one.
fn chosen_wall(document: &Document, tool: &WallTool) -> Option<EntryRef> {
    let id = tool.chosen.as_ref()?;
    let (collection, walls) = document.find::<venture_structures::WallDef>()?;
    walls
        .files()
        .iter()
        .enumerate()
        .find_map(|(f, file)| file.entries.iter().position(|def| &def.id == id).map(|i| (collection, f, i)))
}

const PLAY_HERE_HELP: &str = "Saves, then starts the game (--local) with a new character at the camera's spot. \
It uses a profile of its own (\"editor\"), with admin powers, so your usual character is left alone.";

fn world_action(world: &mut World, shell: &mut Shell, document: &mut Document, action: WorldAction, now: f64) {
    let mut cameras = world.query::<&EditorCamera>();
    let focus = cameras.single(world).map(|camera| from_bevy(camera.focus)).unwrap_or_default();
    let chosen = match action {
        WorldAction::Add(kind) => match add_place(document, kind, focus, now) {
            Ok(key) => Some((kind, key)),
            Err(error) => {
                shell.status = format!("Couldn't add it: {error}.");
                None
            }
        },
        WorldAction::Copy(place) => copy_place(document, &place, now).map(|key| (place.kind, key)),
        WorldAction::Delete(place) => {
            delete_place(document, &place, now);
            None
        }
        WorldAction::Bake => {
            let project = world.resource::<Project>().clone();
            let seed = world.get_resource::<WorldInfo>().map(|info| info.0.seed);
            let mut tool = world.resource_mut::<WallTool>();
            save_and_bake(&mut tool, document, &project, seed);
            shell.status = tool.status.clone();
            return;
        }
        WorldAction::PlayHere => {
            save_all(shell, document);
            let project = world.resource::<Project>().clone();
            shell.status = match play_here(&project, focus) {
                Ok(()) => "Starting the game…".to_string(),
                Err(error) => format!("Couldn't start the game: {error}"),
            };
            return;
        }
    };
    world.resource_mut::<Selection>().0 = chosen;
    shell.check_due = Some(now + CHECK_DELAY);
}

/// Starts the game in `--local` mode with a new character at a spot, as an
/// admin, on a profile of its own.
fn play_here(project: &Project, (x, y): (f32, f32)) -> Result<(), String> {
    let world = project.world.to_string_lossy().to_string();
    std::process::Command::new("cargo")
        .args(["run", "-p", "venture_client", "--", "--local", "--new-character", "--admin", "--world", &world])
        .env("VENTURE_PROFILE", "editor")
        .env("VENTURE_CAMERA", format!("{x},{y},0.6,0.4,14"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// How far from the camera's point markers get their names shown, in metres.
const NAMES_WITHIN: f32 = 150.0;

/// Writes each marker's name over it, in the 3D view.
fn name_markers(root: &egui::Ui, world: &mut World, rect: egui::Rect) {
    let mut cameras = world.query::<(&Camera, &GlobalTransform, &EditorCamera)>();
    let mut markers = world.query::<(&PlaceMarker, &Transform)>();
    let world = &*world;
    let Ok((camera, camera_place, orbit)) = cameras.single(world) else { return };
    let focus = orbit.focus;
    let places = world.resource::<Places>();
    let selection = world.resource::<Selection>().clone();
    let painter = root.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, "marker names".into()));
    let painter = painter.with_clip_rect(rect);
    for (marker, transform) in markers.iter(world) {
        if transform.translation.distance(focus) > NAMES_WITHIN {
            continue;
        }
        let above = Vec3::Y * marker_heights(marker.kind).1;
        let Ok(spot) = camera.world_to_viewport(camera_place, transform.translation + above) else {
            continue;
        };
        let Some(place) = places.list.iter().find(|place| place.kind == marker.kind && place.key == marker.key) else {
            continue;
        };
        let colour = if selection.is(place) { egui::Color32::WHITE } else { egui::Color32::from_gray(225) };
        // (Bevy's viewport positions are already from the window's corner.)
        let at = egui::pos2(spot.x, spot.y);
        let font = egui::FontId::proportional(14.0);
        painter.text(
            at + egui::vec2(1.0, 1.0),
            egui::Align2::CENTER_BOTTOM,
            &place.label,
            font.clone(),
            egui::Color32::BLACK,
        );
        painter.text(at, egui::Align2::CENTER_BOTTOM, &place.label, font, colour);
    }
}
