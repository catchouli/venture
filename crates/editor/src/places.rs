//! Things placed in the world: NPCs, spawners and respawn points, shown and
//! moved on the World tab.
//!
//! The [`Document`] stays the one truth. Each placed thing gets a marker
//! entity ([`PlaceMarker`]), kept in step with the document whenever it
//! changes. Clicking a marker chooses it; dragging it moves only the marker,
//! and letting go writes the new position into its entry (as one undo
//! step).

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;
use venture_client::characters::humans::HumanLook;
use venture_client::characters::model::{CharacterAssets, appearance_look, spawn_character};
use venture_client::characters::monsters::spawn_monster;
use venture_client::common::coords::{from_bevy, to_bevy};
use venture_client::content::ContentStore;
use venture_client::world::query::{ground_height, raycast_ground};
use venture_client::world::streaming::ChunkStore;
use venture_content::{AppearanceDef, MonsterDef, NpcDef, RespawnPoint, SpawnerDef};
use venture_protocol::MonsterLook;

use crate::camera::EditorCamera;
use crate::document::{Document, Entry};
use crate::shell::ViewRect;

/// How close (metres) the pointer's ray must pass to a marker to pick it.
const PICK_RADIUS: f32 = 1.2;
/// How far the pointer must move (pixels) before a press becomes a drag.
const DRAG_START: f32 = 4.0;

/// Shows placed things and lets them be picked and moved.
pub struct PlacesPlugin;

impl Plugin for PlacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Places>()
            .init_resource::<Selection>()
            .init_resource::<Drag>()
            .add_systems(Startup, make_marker_assets)
            .add_systems(
                Update,
                (list_places, sync_markers, pick_and_drag.in_set(PickPlaces), keep_on_ground, draw_guides).chain(),
            );
    }
}

/// Where clicks pick placed things (walls' clicks are ordered around it).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PickPlaces;

/// The kinds of things placed in the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceKind {
    /// A non-player character.
    Npc,
    /// A monster spawner.
    Spawner,
    /// Where knocked-out players wake up.
    RespawnPoint,
}

impl PlaceKind {
    /// Every kind, in the order the outliner lists them.
    pub const ALL: [PlaceKind; 3] = [PlaceKind::Npc, PlaceKind::Spawner, PlaceKind::RespawnPoint];

    /// Its name in the outliner.
    pub fn title(self) -> &'static str {
        match self {
            PlaceKind::Npc => "NPCs",
            PlaceKind::Spawner => "Spawners",
            PlaceKind::RespawnPoint => "Respawn points",
        }
    }
}

/// An entry with a place in the world.
pub trait Placeable: Entry {
    /// Its kind.
    const KIND: PlaceKind;
    /// Where it is (metres).
    fn at(&self) -> (f32, f32);
    /// Where it is, to change.
    fn at_mut(&mut self) -> &mut (f32, f32);
    /// What to call it in the world view.
    fn label(&self) -> String;
}

impl Placeable for NpcDef {
    const KIND: PlaceKind = PlaceKind::Npc;
    fn at(&self) -> (f32, f32) {
        self.at
    }
    fn at_mut(&mut self) -> &mut (f32, f32) {
        &mut self.at
    }
    fn label(&self) -> String {
        self.name.clone()
    }
}

impl Placeable for SpawnerDef {
    const KIND: PlaceKind = PlaceKind::Spawner;
    fn at(&self) -> (f32, f32) {
        self.at
    }
    fn at_mut(&mut self) -> &mut (f32, f32) {
        &mut self.at
    }
    fn label(&self) -> String {
        format!("{} ({})", self.monster, self.id)
    }
}

impl Placeable for RespawnPoint {
    const KIND: PlaceKind = PlaceKind::RespawnPoint;
    fn at(&self) -> (f32, f32) {
        self.at
    }
    fn at_mut(&mut self) -> &mut (f32, f32) {
        &mut self.at
    }
    fn label(&self) -> String {
        format!("Respawn: {}", self.name)
    }
}

/// Where an entry is in the document: (collection, file, entry).
pub type EntryRef = (usize, usize, usize);

/// A placed thing, as the World tab shows it.
#[derive(Clone, Debug)]
pub struct Place {
    /// Its kind.
    pub kind: PlaceKind,
    /// Its entry.
    pub entry: EntryRef,
    /// Its id (or name).
    pub key: String,
    /// What to call it.
    pub label: String,
    /// Where it is in the world (metres).
    pub position: (f32, f32),
    /// For spawners: how far from it monsters appear, and how far away
    /// aggressive ones notice players (if they do).
    pub radii: Option<(f32, Option<f32>)>,
    /// How its marker looks.
    pub look: PlaceLook,
}

/// How a placed thing's marker looks: the game's own models where there are
/// some to show.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaceLook {
    /// A person, in their appearance (if it exists).
    Person(Option<AppearanceDef>),
    /// A monster (if it exists).
    Monster(Option<MonsterLook>),
    /// Just a marker.
    Point,
}

/// Every placed thing, worked out from the document.
#[derive(Resource, Default)]
pub struct Places {
    /// The things, by kind, in document order.
    pub list: Vec<Place>,
    /// The document revision they're from.
    made_from: Option<u64>,
}

/// The chosen placed thing, by kind and key (which survive other entries
/// being added or removed).
#[derive(Resource, Default, Clone, PartialEq, Eq)]
pub struct Selection(pub Option<(PlaceKind, String)>);

impl Selection {
    /// Whether this place is the chosen one.
    pub fn is(&self, place: &Place) -> bool {
        self.0.as_ref().is_some_and(|(kind, key)| *kind == place.kind && *key == place.key)
    }
}

/// A press on a marker, which becomes a drag once the pointer moves.
#[derive(Resource, Default)]
struct Drag {
    /// The marker being pressed, and where the pointer was pressed (window pixels).
    pressed: Option<(Entity, Vec2)>,
    /// Whether it's being dragged (not just clicked).
    moving: bool,
}

/// A marker in the world for a placed thing, standing on the ground where it is.
#[derive(Component)]
pub struct PlaceMarker {
    /// What it marks.
    pub kind: PlaceKind,
    /// Its entry's key.
    pub key: String,
    /// How it looks (to tell when it needs making again).
    look: PlaceLook,
}

/// How high above a marker's feet its middle is (for picking it), and where
/// its name goes.
pub fn marker_heights(kind: PlaceKind) -> (f32, f32) {
    match kind {
        PlaceKind::Npc => (0.9, 2.0),
        PlaceKind::Spawner => (0.5, 1.3),
        PlaceKind::RespawnPoint => (0.1, 0.6),
    }
}

/// Meshes and materials for markers.
#[derive(Resource)]
struct MarkerAssets {
    shapes: HashMap<PlaceKind, (Handle<Mesh>, Handle<StandardMaterial>)>,
}

fn make_marker_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut material = |colour: Color| {
        materials.add(StandardMaterial { base_color: colour, emissive: colour.to_linear() * 0.4, ..default() })
    };
    let shapes = HashMap::from([
        (PlaceKind::Npc, (meshes.add(Capsule3d::new(0.3, 1.1)), material(Color::srgb(0.3, 0.55, 0.95)))),
        (PlaceKind::Spawner, (meshes.add(Sphere::new(0.45)), material(Color::srgb(0.9, 0.3, 0.25)))),
        (PlaceKind::RespawnPoint, (meshes.add(Cylinder::new(0.6, 0.15)), material(Color::srgb(0.95, 0.8, 0.3)))),
    ]);
    commands.insert_resource(MarkerAssets { shapes });
}

/// Works out every placed thing whenever the document changes.
fn list_places(document: Res<Document>, mut places: ResMut<Places>) {
    if places.made_from == Some(document.revision) {
        return;
    }
    let appearances: HashMap<String, AppearanceDef> = document
        .find::<AppearanceDef>()
        .map(|(_, all)| {
            all.files().iter().flat_map(|file| file.entries.iter().map(|a| (a.id.clone(), a.clone()))).collect()
        })
        .unwrap_or_default();
    let monsters: HashMap<String, MonsterDef> = document
        .find::<MonsterDef>()
        .map(|(_, monsters)| {
            monsters.files().iter().flat_map(|file| file.entries.iter().map(|c| (c.id.clone(), c.clone()))).collect()
        })
        .unwrap_or_default();
    let mut list = Vec::new();
    add_places::<NpcDef>(&document, &mut list, |npc| {
        (None, PlaceLook::Person(appearances.get(&npc.appearance).cloned()))
    });
    add_places::<SpawnerDef>(&document, &mut list, |spawner| {
        let monster = monsters.get(&spawner.monster);
        let aggro = monster.filter(|monster| monster.aggressive).map(|monster| monster.aggro_radius);
        (Some((spawner.radius, aggro)), PlaceLook::Monster(monster.map(|monster| monster.look.clone())))
    });
    add_places::<RespawnPoint>(&document, &mut list, |_| (None, PlaceLook::Point));
    places.list = list;
    places.made_from = Some(document.revision);
}

fn add_places<T: Placeable>(
    document: &Document,
    list: &mut Vec<Place>,
    extras: impl Fn(&T) -> (Option<(f32, Option<f32>)>, PlaceLook),
) {
    let Some((collection, kind)) = document.find::<T>() else { return };
    for (f, file) in kind.files().iter().enumerate() {
        for (i, entry) in file.entries.iter().enumerate() {
            let (radii, look) = extras(entry);
            list.push(Place {
                kind: T::KIND,
                entry: (collection, f, i),
                key: entry.key(),
                label: entry.label(),
                position: entry.at(),
                radii,
                look,
            });
        }
    }
}

/// Makes the markers match the places: new ones appear, gone ones go, moved
/// ones move (unless one is being dragged), and ones that look different are
/// made again.
fn sync_markers(
    mut commands: Commands,
    places: Res<Places>,
    assets: Option<Res<MarkerAssets>>,
    shapes: Res<CharacterAssets>,
    content: Res<ContentStore>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    store: Res<ChunkStore>,
    drag: Res<Drag>,
    mut markers: Query<(Entity, &PlaceMarker, &mut Transform)>,
) {
    let Some(assets) = assets else { return };
    if !places.is_changed() && !content.is_changed() {
        return;
    }
    let mut existing: HashMap<(PlaceKind, String), Entity> =
        markers.iter().map(|(entity, marker, _)| ((marker.kind, marker.key.clone()), entity)).collect();
    for place in &places.list {
        let (x, y) = place.position;
        let height = ground_height(&store, x, y).unwrap_or(0.0);
        let at = Transform::from_translation(to_bevy(x, y, height));
        if let Some(entity) = existing.remove(&(place.kind, place.key.clone())) {
            let Ok((_, marker, mut transform)) = markers.get_mut(entity) else { continue };
            if marker.look == place.look {
                let dragged = drag.moving && drag.pressed.is_some_and(|(pressed, _)| pressed == entity);
                if !dragged && transform.translation != at.translation {
                    transform.translation = at.translation;
                }
                continue;
            }
            commands.entity(entity).despawn();
        }
        let marker = match &place.look {
            PlaceLook::Person(Some(appearance)) => {
                let look = appearance_look(&mut materials, appearance);
                let person = spawn_character(&mut commands, &shapes, at, &look);
                let models = content.character_models.as_ref();
                commands.entity(person).insert(HumanLook::of_appearance(Some(appearance), models, &content.items));
                person
            }
            PlaceLook::Monster(Some(look)) => spawn_monster(&mut commands, &shapes, &mut materials, at, look),
            _ => {
                let (mesh, material) = assets.shapes[&place.kind].clone();
                let lift = Vec3::Y * marker_heights(place.kind).0;
                commands
                    .spawn((at, Visibility::default()))
                    .with_child((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(lift)))
                    .id()
            }
        };
        commands.entity(marker).insert((
            Name::new(format!("Marker: {}", place.label)),
            PlaceMarker { kind: place.kind, key: place.key.clone(), look: place.look.clone() },
        ));
    }
    for (_, entity) in existing {
        commands.entity(entity).despawn();
    }
}

/// Markers placed before their ground loaded settle onto it.
fn keep_on_ground(store: Res<ChunkStore>, mut markers: Query<&mut Transform, With<PlaceMarker>>) {
    if !store.is_changed() {
        return;
    }
    for mut transform in &mut markers {
        let (x, y) = from_bevy(transform.translation);
        if let Some(height) = ground_height(&store, x, y)
            && (transform.translation.y - height).abs() > 0.01
        {
            transform.translation.y = height;
        }
    }
}

/// The pointer's ray into the world, if it's over the 3D view.
fn pointer_ray(window: &Window, view: &ViewRect, camera: (&Camera, &GlobalTransform)) -> Option<(Ray3d, Vec2)> {
    let cursor = window.cursor_position()?;
    let rect = view.0?;
    if !rect.contains(cursor) {
        return None;
    }
    // (Bevy's viewport positions are from the window's corner, not the view's.)
    let ray = camera.0.viewport_to_world(camera.1, cursor).ok()?;
    Some((ray, cursor))
}

/// Left click picks a marker (or clears the choice, on bare ground); dragging
/// one moves it along the ground, and letting go saves where it went.
fn pick_and_drag(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<EditorCamera>>,
    store: Res<ChunkStore>,
    mut drag: ResMut<Drag>,
    mut selection: ResMut<Selection>,
    mut document: ResMut<Document>,
    mut markers: Query<(Entity, &PlaceMarker, &mut Transform)>,
) {
    let pointer = pointer_ray(&window, &view, *camera);
    if buttons.just_pressed(MouseButton::Left) && !egui.wants_pointer_input() {
        let Some((ray, cursor)) = pointer else { return };
        // The marker nearest the camera whose centre the ray passes close to.
        let picked = markers
            .iter()
            .filter_map(|(entity, marker, transform)| {
                let middle = transform.translation + Vec3::Y * marker_heights(marker.kind).0;
                let along = (middle - ray.origin).dot(*ray.direction);
                let closest = ray.origin + *ray.direction * along;
                let size = PICK_RADIUS + along * 0.004;
                (along > 0.0 && closest.distance(middle) < size).then_some((along, entity, marker))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match picked {
            Some((_, entity, marker)) => {
                selection.0 = Some((marker.kind, marker.key.clone()));
                *drag = Drag { pressed: Some((entity, cursor)), moving: false };
            }
            None => selection.0 = None,
        }
        return;
    }
    let Some((entity, pressed_at)) = drag.pressed else { return };
    if buttons.pressed(MouseButton::Left) {
        let Some((ray, cursor)) = pointer else { return };
        if !drag.moving && cursor.distance(pressed_at) < DRAG_START {
            return;
        }
        drag.moving = true;
        if let (Some(ground), Ok((_, _, mut transform))) = (raycast_ground(&store, ray), markers.get_mut(entity)) {
            transform.translation = ground;
        }
        return;
    }
    // Let go: a drag writes the marker's new place into its entry.
    let moving = drag.moving;
    *drag = Drag::default();
    let (true, Ok((_, marker, transform))) = (moving, markers.get(entity)) else { return };
    let point = from_bevy(transform.translation);
    let key = marker.key.clone();
    let moved = match marker.kind {
        PlaceKind::Npc => move_entry::<NpcDef>(&mut document, &key, point),
        PlaceKind::Spawner => move_entry::<SpawnerDef>(&mut document, &key, point),
        PlaceKind::RespawnPoint => move_entry::<RespawnPoint>(&mut document, &key, point),
    };
    if let Some((collection, file)) = moved {
        document.changed(collection, file, time.elapsed_secs_f64());
    }
}

/// Moves an entry (by key) to a point, rounded to the centimetre (so it
/// saves as tidy numbers). Returns its (collection, file).
fn move_entry<T: Placeable>(document: &mut Document, key: &str, (x, y): (f32, f32)) -> Option<(usize, usize)> {
    let (collection, kind) = document.find_mut::<T>()?;
    for (f, file) in kind.files_mut().iter_mut().enumerate() {
        if let Some(entry) = file.entries.iter_mut().find(|entry| entry.key() == key) {
            *entry.at_mut() = (round_to_centimetres(x), round_to_centimetres(y));
            return Some((collection, f));
        }
    }
    None
}

/// Rounds to the nearest centimetre.
fn round_to_centimetres(metres: f32) -> f32 {
    (metres * 100.0).round() / 100.0
}

/// Draws spawners' radii on the ground, and a ring around the chosen thing.
fn draw_guides(
    mut gizmos: Gizmos,
    places: Res<Places>,
    selection: Res<Selection>,
    store: Res<ChunkStore>,
    view: Res<ViewRect>,
    markers: Query<(&PlaceMarker, &Transform)>,
) {
    if view.0.is_none() {
        return;
    }
    let positions: HashMap<(PlaceKind, &str), Vec3> =
        markers.iter().map(|(marker, transform)| ((marker.kind, marker.key.as_str()), transform.translation)).collect();
    for place in &places.list {
        let Some(&centre) = positions.get(&(place.kind, place.key.as_str())) else { continue };
        let chosen = selection.is(place);
        if let Some((radius, aggro)) = place.radii {
            let colour = if chosen { Color::srgb(1.0, 0.55, 0.45) } else { Color::srgba(0.9, 0.3, 0.25, 0.7) };
            ground_circle(&mut gizmos, &store, centre, radius, colour);
            if let Some(aggro) = aggro {
                ground_circle(&mut gizmos, &store, centre, aggro, Color::srgba(1.0, 0.6, 0.1, 0.5));
            }
        }
        if chosen {
            ground_circle(&mut gizmos, &store, centre, 1.0, Color::WHITE);
        }
    }
}

/// A circle following the ground.
fn ground_circle(gizmos: &mut Gizmos, store: &ChunkStore, centre: Vec3, radius: f32, colour: Color) {
    let segments = ((radius * 4.0) as usize).clamp(24, 160);
    let points = (0..=segments).map(|i| {
        let angle = i as f32 / segments as f32 * std::f32::consts::TAU;
        let (x, z) = (centre.x + angle.cos() * radius, centre.z + angle.sin() * radius);
        Vec3::new(x, ground_height(store, x, z).unwrap_or(centre.y) + 0.15, z)
    });
    gizmos.linestrip(points, colour);
}

/// Adds a new placed thing of type `T` at a point, made from RON with
/// `{id}` filled in, in the kind's first file. Returns its key.
fn add_new<T: Placeable>(
    document: &mut Document,
    (x, y): (f32, f32),
    prefix: &str,
    template: &str,
    now: f64,
) -> Result<String, String> {
    let (collection, kind) = document.find_mut::<T>().ok_or("no such kind of content")?;
    let taken: Vec<String> = kind.files().iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect();
    let key = (1..).map(|n| format!("{prefix}_{n}")).find(|key| !taken.contains(key)).unwrap_or_default();
    let text = template.replace("{id}", &key);
    let mut entry: T = ron::from_str(&text).map_err(|error| error.to_string())?;
    *entry.at_mut() = (round_to_centimetres(x), round_to_centimetres(y));
    let file = kind.files_mut().first_mut().ok_or("there's no file to add it to yet")?;
    file.entries.push(entry);
    document.changed(collection, 0, now);
    Ok(key)
}

/// Adds a new NPC, spawner or respawn point at a point. Returns its key.
pub fn add_place(document: &mut Document, kind: PlaceKind, point: (f32, f32), now: f64) -> Result<String, String> {
    let first_key = |collection: Option<Vec<String>>| collection.and_then(|keys| keys.into_iter().next());
    match kind {
        PlaceKind::Npc => {
            let appearance = first_key(document.find::<AppearanceDef>().map(|(_, all)| keys_of(all.files())))
                .ok_or("make an appearance first")?;
            let template = format!(r#"(id: "{{id}}", name: "New person", appearance: "{appearance}", at: (0.0, 0.0))"#);
            add_new::<NpcDef>(document, point, "npc", &template, now)
        }
        PlaceKind::Spawner => {
            let monster = first_key(document.find::<MonsterDef>().map(|(_, all)| keys_of(all.files())))
                .ok_or("make a monster first")?;
            let template =
                format!(r#"(id: "{{id}}", monster: "{monster}", at: (0.0, 0.0), radius: 8.0, respawn_seconds: 30.0)"#);
            add_new::<SpawnerDef>(document, point, "spawner", &template, now)
        }
        PlaceKind::RespawnPoint => {
            let template = r#"(name: "{id}", at: (0.0, 0.0))"#;
            add_new::<RespawnPoint>(document, point, "respawn point", template, now)
        }
    }
}

fn keys_of<T: Entry>(files: &[crate::document::EntryFile<T>]) -> Vec<String> {
    files.iter().flat_map(|file| file.entries.iter().map(Entry::key)).collect()
}

/// Copies a placed thing, a couple of metres east of it. Returns the copy's key.
pub fn copy_place(document: &mut Document, place: &Place, now: f64) -> Option<String> {
    let (collection, file, index) = place.entry;
    let copy = document.collections[collection].duplicate(file, index)?;
    let key = document.collections[collection].keys(file).get(copy)?.clone();
    document.changed(collection, file, now);
    let (x, y) = place.position;
    let moved = match place.kind {
        PlaceKind::Npc => move_entry::<NpcDef>(document, &key, (x + 2.0, y)),
        PlaceKind::Spawner => move_entry::<SpawnerDef>(document, &key, (x + 2.0, y)),
        PlaceKind::RespawnPoint => move_entry::<RespawnPoint>(document, &key, (x + 2.0, y)),
    };
    if let Some((collection, file)) = moved {
        document.changed(collection, file, now);
    }
    Some(key)
}

/// Deletes a placed thing.
pub fn delete_place(document: &mut Document, place: &Place, now: f64) {
    let (collection, file, index) = place.entry;
    document.collections[collection].remove(file, index);
    document.changed(collection, file, now);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::document::copy_folder;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("venture-places-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        copy_folder(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"), &folder).unwrap();
        folder
    }

    fn npcs(document: &Document) -> Vec<NpcDef> {
        document.find::<NpcDef>().unwrap().1.files().iter().flat_map(|file| file.entries.clone()).collect()
    }

    fn place_of(document: &Document, key: &str) -> Place {
        let (collection, npcs) = document.find::<NpcDef>().unwrap();
        let index = npcs.files()[0].entries.iter().position(|npc| npc.id == key).unwrap();
        let npc = &npcs.files()[0].entries[index];
        Place {
            kind: PlaceKind::Npc,
            entry: (collection, 0, index),
            key: key.to_string(),
            label: npc.name.clone(),
            position: npc.at,
            radii: None,
            look: PlaceLook::Point,
        }
    }

    #[test]
    fn places_are_added_copied_and_deleted() {
        let folder = scratch("edit");
        let mut document = Document::load(&folder).unwrap();
        let before = npcs(&document).len();

        let key = add_place(&mut document, PlaceKind::Npc, (1010.004, 995.0), 0.0).unwrap();
        let added = npcs(&document).into_iter().find(|npc| npc.id == key).unwrap();
        assert_eq!(added.at, (1010.0, 995.0));

        let place = place_of(&document, &key);
        let copy = copy_place(&mut document, &place, 10.0).unwrap();
        let copied = npcs(&document).into_iter().find(|npc| npc.id == copy).unwrap();
        assert_eq!(copied.at, (1012.0, 995.0));

        let place = place_of(&document, &key);
        delete_place(&mut document, &place, 30.0);
        assert_eq!(npcs(&document).len(), before + 1);
        let mirror = scratch("edit-mirror");
        assert!(document.check(&mirror).is_empty(), "{:?}", document.check(&mirror));
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&mirror);
    }
}
