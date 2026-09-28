//! Fishing traps: setting one from the bag, drawing everyone's in the water,
//! and collecting your own (the server decides everything: see its
//! `fishing.rs`).
//!
//! - Using a trap in the bag (clicking it) sets it in the nearest water
//!   within reach, baited with the first bait in the bag that crayfish or
//!   crabs go for, if there is one.
//! - A trap shows as a cage on the bottom and a buoy on the surface, with
//!   its owner's name. Clicking your own when it's selected collects it.

use bevy::prelude::*;
use venture_content::FishingPart;
use venture_navigation::Point;
use venture_protocol::{ClientMessage, EntityId, FishingCommand};
use venture_rpg::fishing::Diet;
use venture_world::CellPos;

use crate::characters::player::LocalPlayer;
use crate::common::coords::{from_bevy, to_bevy};
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::network::{ClientState, ToServer};
use crate::ui::character_window::inventory::PlayerInventory;
use crate::ui::chat::SystemMessage;
use crate::world::query::{ground_height, water_level};
use crate::world::streaming::{ChunkStore, StreamingProgress};

/// How far from the player a trap may be set, in metres (a little less than
/// the server allows).
const REACH: f32 = 4.5;
/// The buoy's radius, in metres.
const BUOY_RADIUS: f32 = 0.14;

/// Traps.
pub struct TrapsPlugin;

impl Plugin for TrapsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SetTrapFromBag>().add_message::<CollectTrap>().add_systems(Startup, make_assets).add_systems(
            Update,
            (set_trap_on_startup_if_requested, set_traps, collect_traps).chain().run_if(in_state(ClientState::Playing)),
        );
    }
}

/// Set this trap (an item id) from the bag (sent by the inventory).
#[derive(Message)]
pub struct SetTrapFromBag(pub String);

/// Collect this trap (sent by clicks on a selected trap).
#[derive(Message)]
pub struct CollectTrap(pub Entity);

/// Whose a trap is.
#[derive(Component)]
pub struct TrapOwner(pub EntityId);

/// The trap's meshes and materials.
#[derive(Resource)]
pub struct TrapAssets {
    cage: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    buoy: Handle<Mesh>,
    paint: Handle<StandardMaterial>,
    rope: Handle<Mesh>,
    rope_colour: Handle<StandardMaterial>,
}

fn make_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(TrapAssets {
        cage: meshes.add(Cuboid::new(0.6, 0.35, 0.4)),
        wood: materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.24, 0.16),
            perceptual_roughness: 0.9,
            ..default()
        }),
        buoy: meshes.add(Sphere::new(BUOY_RADIUS).mesh().uv(12, 8)),
        paint: materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.45, 0.1), ..default() }),
        rope: meshes.add(Cylinder::new(0.012, 1.0).mesh().resolution(4)),
        rope_colour: materials.add(StandardMaterial { base_color: Color::srgb(0.7, 0.65, 0.5), ..default() }),
    });
}

/// A trap at `at`: its buoy on the water's surface (the entity itself, so its
/// name shows above the water), its cage on the bottom, and a rope between.
pub fn spawn_trap(commands: &mut Commands, assets: &TrapAssets, store: &ChunkStore, (x, y): Point) -> Entity {
    let bottom = ground_height(store, x, y).unwrap_or(0.0);
    let surface = water_level(store, x, y).unwrap_or(bottom).max(bottom);
    let depth = surface - bottom;
    commands
        .spawn((Transform::from_translation(to_bevy(x, y, surface)), Visibility::default()))
        .with_children(|trap| {
            trap.spawn((Mesh3d(assets.buoy.clone()), MeshMaterial3d(assets.paint.clone()), Transform::default()));
            trap.spawn((
                Mesh3d(assets.cage.clone()),
                MeshMaterial3d(assets.wood.clone()),
                Transform::from_xyz(0.0, -depth + 0.17, 0.0),
            ));
            if depth > 0.05 {
                trap.spawn((
                    Mesh3d(assets.rope.clone()),
                    MeshMaterial3d(assets.rope_colour.clone()),
                    Transform::from_xyz(0.0, -depth / 2.0, 0.0).with_scale(Vec3::new(1.0, depth, 1.0)),
                ));
            }
        })
        .id()
}

/// The nearest water to a point within `reach`, if any (the middle of the
/// nearest water cell).
fn nearest_water(store: &ChunkStore, (x, y): Point, reach: f32) -> Option<Point> {
    let cells = reach.ceil() as i32;
    let centre = CellPos::containing(x, y);
    (-cells..=cells)
        .flat_map(|dy| (-cells..=cells).map(move |dx| (centre.x + dx, centre.y + dy)))
        .map(|(cx, cy)| (cx as f32 + 0.5, cy as f32 + 0.5))
        .filter(|&(cx, cy)| (cx - x).hypot(cy - y) <= reach && water_level(store, cx, cy).is_some())
        .min_by(|a, b| (a.0 - x).hypot(a.1 - y).total_cmp(&(b.0 - x).hypot(b.1 - y)))
}

/// For development (`VENTURE_TRAP`): once the world has loaded, sets the
/// first trap in the bag.
fn set_trap_on_startup_if_requested(
    dev: Res<DevSettings>,
    progress: Res<StreamingProgress>,
    content: Res<ContentStore>,
    inventory: Res<PlayerInventory>,
    mut requests: MessageWriter<SetTrapFromBag>,
    mut done: Local<bool>,
) {
    if !dev.trap || *done || !progress.measured || progress.waiting > 0 {
        return;
    }
    let is_trap = |item: &str| {
        matches!(content.items.get(item).and_then(|def| def.fishing.as_ref()), Some(FishingPart::Trap { .. }))
    };
    if let Some(stack) = inventory.slots.iter().flatten().find(|stack| is_trap(&stack.item)) {
        requests.write(SetTrapFromBag(stack.item.clone()));
        *done = true;
    }
}

fn set_traps(
    mut requests: MessageReader<SetTrapFromBag>,
    store: Res<ChunkStore>,
    content: Res<ContentStore>,
    inventory: Res<PlayerInventory>,
    player: Option<Single<&Transform, With<LocalPlayer>>>,
    mut to_server: MessageWriter<ToServer>,
    mut system: MessageWriter<SystemMessage>,
) {
    let Some(SetTrapFromBag(trap)) = requests.read().last() else { return };
    let Some(player) = player else { return };
    let Some((x, y)) = nearest_water(&store, from_bevy(player.translation), REACH) else {
        system.write(SystemMessage("Stand at the water's edge to set a trap.".to_string()));
        return;
    };
    let for_traps = |item: &str| match content.items.get(item).and_then(|def| def.fishing.as_ref()) {
        Some(FishingPart::Bait(bait)) => {
            bait.diets.iter().any(|diet| matches!(diet, Diet::Crustacean | Diet::Scavenger))
        }
        _ => false,
    };
    let bait = inventory.slots.iter().flatten().map(|stack| &stack.item).find(|item| for_traps(item)).cloned();
    to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::SetTrap { x, y, trap: trap.clone(), bait })));
}

fn collect_traps(
    mut requests: MessageReader<CollectTrap>,
    traps: Query<&TrapOwner>,
    me: Option<Single<&LocalPlayer>>,
    mut to_server: MessageWriter<ToServer>,
    mut system: MessageWriter<SystemMessage>,
) {
    let Some(me) = me else { return };
    for &CollectTrap(trap) in requests.read() {
        let Ok(owner) = traps.get(trap) else { continue };
        if owner.0 == me.id {
            to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::CollectTrap)));
        } else {
            system.write(SystemMessage("That's someone else's trap.".to_string()));
        }
    }
}
