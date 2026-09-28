//! Fish shadows in the water around the player: what's there for them to
//! catch (solid), and what they'll be able to catch within ten levels
//! (faint and smaller). Fish they're further from are hidden.
//!
//! Which fish are where comes from the world's seed, the region of water and
//! the hour (`venture_simulation::fishing::shoals`), exactly as the server
//! works it out, so casting where a shadow swims really does make that fish
//! likelier to bite. Each shadow swims from one water cell of its region to
//! another, its shoal around it, and fish near a line in the water gather
//! round the float.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use venture_content::{FishDef, fish_living_in};
use venture_rpg::fishing::{Shadow, Size, shadow};
use venture_simulation::fishing::{SHOAL_REGION, most_common_water, region_cells, shoal_region, shoals};
use venture_world::{Biome, CellPos, FluidBody, FluidKind};

use crate::characters::player::LocalPlayer;
use crate::common::coords::{from_bevy, to_bevy};
use crate::content::ContentStore;
use crate::fishing::{Angling, Stage};
use crate::network::ClientState;
use crate::ui::character_window::Sheet;
use crate::world::query::water_level;
use crate::world::source::WorldInfo;
use crate::world::streaming::ChunkStore;
use crate::world::time_of_day::TimeOfDay;

/// How many regions out from the player's shadows are shown.
const REGIONS_OUT: i32 = 3;
/// The fewest water cells a region needs for fish to be in it.
const MIN_WATER_CELLS: usize = 6;
/// How far above the water's surface shadows are drawn, in metres (just
/// above, so they're drawn over the water rather than hidden in it).
const LIFT: f32 = 0.03;
/// How dark shadows are (their opacity): solid, and faint.
const SOLID: f32 = 0.55;
const FAINT: f32 = 0.18;
/// How much smaller faint shadows are.
const FAINT_SCALE: f32 = 0.75;
/// How near a float fish gather round it, in metres, and how far towards it.
const GATHER_DISTANCE: f32 = 6.0;
const GATHER: f32 = 0.7;
/// How long a fish takes to swim from one cell to the next, in seconds (least and most).
const LEG_SECONDS: (f32, f32) = (4.0, 9.0);
/// How far a shoal's fish spread round their leader, in metres.
const SPREAD: f32 = 0.8;
/// The fishing skill's id.
const SKILL: &str = "fishing";

/// Shows fish shadows.
pub struct ShadowsPlugin;

impl Plugin for ShadowsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShownShadows>()
            .add_systems(Startup, make_assets)
            .add_systems(Update, (choose_shadows, swim).chain().run_if(in_state(ClientState::Playing)));
    }
}

/// The shared mesh and the two materials.
#[derive(Resource)]
struct ShadowAssets {
    mesh: Handle<Mesh>,
    solid: Handle<StandardMaterial>,
    faint: Handle<StandardMaterial>,
}

/// What the shown shadows were chosen for (they're chosen again when it changes).
#[derive(Resource, Default, PartialEq)]
struct ShownShadows {
    key: Option<ShadowsKey>,
}

#[derive(Clone, PartialEq)]
struct ShadowsKey {
    region: (i32, i32),
    hour: u32,
    level: u32,
    /// How many of the regions round the player had their chunks loaded.
    loaded: usize,
    content: Option<String>,
}

/// One fish's shadow.
#[derive(Component)]
struct FishShadow {
    /// The water cells (centres) it swims between.
    cells: Vec<(f32, f32)>,
    /// Varies its path and pace.
    seed: u64,
    /// Where it swims round its shoal's leader.
    offset: (f32, f32),
    /// How long it is, in metres.
    length: f32,
}

fn make_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = |alpha: f32| StandardMaterial {
        base_color: Color::srgba(0.02, 0.05, 0.06, alpha),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    commands.insert_resource(ShadowAssets {
        mesh: meshes.add(fish_mesh()),
        solid: materials.add(material(SOLID)),
        faint: materials.add(material(FAINT)),
    });
}

/// A flat fish, a metre long, lying in the XZ plane and facing +X: an oval
/// body and a forked tail.
fn fish_mesh() -> Mesh {
    let mut positions = vec![[0.0, 0.0, 0.0]];
    let segments = 16;
    for i in 0..=segments {
        let angle = i as f32 / segments as f32 * TAU;
        // The body: from x -0.3 to 0.5, 0.26 wide.
        positions.push([0.1 + angle.cos() * 0.4, 0.0, angle.sin() * 0.13]);
    }
    let mut indices: Vec<u32> = (1..=segments as u32).flat_map(|i| [0, i + 1, i]).collect();
    // The tail.
    let tail = positions.len() as u32;
    positions.extend([[-0.25, 0.0, 0.0], [-0.5, 0.0, 0.16], [-0.42, 0.0, 0.0], [-0.5, 0.0, -0.16]]);
    indices.extend([tail, tail + 2, tail + 1, tail, tail + 3, tail + 2]);
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

/// How long a fish's shadow is, in metres.
fn length(size: Size) -> f32 {
    match size {
        Size::Tiny => 0.3,
        Size::Small => 0.5,
        Size::Medium => 0.8,
        Size::Large => 1.3,
        Size::Trophy => 2.0,
    }
}

/// A region's water: its cells' centres, and the body and biome most of it
/// is (as the server works it out: `region_water` there).
fn region_water(store: &ChunkStore, region: (i32, i32)) -> Option<(Vec<(f32, f32)>, FluidBody, Biome)> {
    let mut cells = Vec::new();
    let mut kinds = Vec::new();
    for (x, y) in region_cells(region) {
        let cell = CellPos::new(x, y);
        let chunk = store.0.get(&cell.chunk())?;
        let (local_x, local_y) = cell.local();
        let terrain = chunk.cell(local_x, local_y);
        if let Some(fluid) = terrain.fluid.filter(|fluid| fluid.kind == FluidKind::Water) {
            cells.push((x as f32 + 0.5, y as f32 + 0.5));
            kinds.push((fluid.body, terrain.biome));
        }
    }
    let (body, biome) = most_common_water(&kinds)?;
    Some((cells, body, biome))
}

/// Chooses the shadows round the player again when they've moved to another
/// region, the hour's changed, their level has, or more water has loaded.
fn choose_shadows(
    mut commands: Commands,
    mut shown: ResMut<ShownShadows>,
    assets: Res<ShadowAssets>,
    store: Res<ChunkStore>,
    content: Res<ContentStore>,
    sheet: Res<Sheet>,
    time_of_day: Res<TimeOfDay>,
    world: Option<Res<WorldInfo>>,
    player: Option<Single<&Transform, With<LocalPlayer>>>,
    shadows: Query<Entity, With<FishShadow>>,
) {
    let (Some(world), Some(player)) = (world, player) else { return };
    let region = shoal_region(from_bevy(player.translation));
    let level = sheet
        .0
        .as_ref()
        .and_then(|sheet| sheet.skills.iter().find(|skill| skill.skill == SKILL))
        .map_or(1, |skill| skill.level);
    let around: Vec<(i32, i32)> = (-REGIONS_OUT..=REGIONS_OUT)
        .flat_map(|dy| (-REGIONS_OUT..=REGIONS_OUT).map(move |dx| (region.0 + dx, region.1 + dy)))
        .collect();
    let loaded = around
        .iter()
        .filter(|(rx, ry)| {
            store.0.contains_key(&CellPos::new(rx * SHOAL_REGION as i32, ry * SHOAL_REGION as i32).chunk())
        })
        .count();
    let hour = time_of_day.0 as u32 % 24;
    let key = ShadowsKey { region, hour, level, loaded, content: content.version.clone() };
    if shown.key.as_ref() == Some(&key) {
        return;
    }
    shown.key = Some(key);
    for entity in &shadows {
        commands.entity(entity).despawn();
    }

    for &place in &around {
        let Some((cells, body, biome)) = region_water(&store, place) else { continue };
        if cells.len() < MIN_WATER_CELLS {
            continue;
        }
        let living: Vec<(&str, &FishDef)> = fish_living_in(content.items.values(), body, biome);
        let weights = vec![1.0; living.len()];
        for shoal in shoals(u64::from(world.0.seed), place, hour, &weights) {
            let fish = living[shoal.fish].1;
            let (material, scale) = match shadow(level, fish.level) {
                Shadow::Solid => (assets.solid.clone(), 1.0),
                Shadow::Faint => (assets.faint.clone(), FAINT_SCALE),
                Shadow::Hidden => continue,
            };
            for member in 0..shoal.count {
                let seed = shoal.seed;
                let angle = member as f32 * 2.4 + (seed % 7) as f32;
                let offset = if member == 0 { (0.0, 0.0) } else { (angle.cos() * SPREAD, angle.sin() * SPREAD) };
                commands.spawn((
                    FishShadow { cells: cells.clone(), seed, offset, length: length(fish.traits.size) * scale },
                    Mesh3d(assets.mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    bevy::light::NotShadowCaster,
                    bevy::light::NotShadowReceiver,
                    Transform::default(),
                ));
            }
        }
    }
}

/// A number from 0 to 1 from a seed and a step.
fn unit(seed: u64, step: u64) -> f32 {
    let mut z = seed ^ step.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Swims each shadow along its path, and gathers fish round a float.
fn swim(
    time: Res<Time>,
    store: Res<ChunkStore>,
    angling: Res<Angling>,
    mut shadows: Query<(&FishShadow, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    let float = angling.line.as_ref().filter(|line| line.stage == Stage::Waiting).map(|line| line.at);
    for (fish, mut transform) in &mut shadows {
        // Legs of the swim, each from one of its region's water cells to another.
        let leg_seconds = LEG_SECONDS.0 + (LEG_SECONDS.1 - LEG_SECONDS.0) * unit(fish.seed, u64::MAX);
        let legs = now / leg_seconds + unit(fish.seed, u64::MAX - 1) * 10.0;
        let leg = legs.floor() as u64;
        let cell =
            |step: u64| fish.cells[(unit(fish.seed, step) * fish.cells.len() as f32) as usize % fish.cells.len()];
        let (from, to) = (cell(leg), cell(leg + 1));
        let t = legs.fract();
        let eased = t * t * (3.0 - 2.0 * t);
        let mut at =
            (from.0 + (to.0 - from.0) * eased + fish.offset.0, from.1 + (to.1 - from.1) * eased + fish.offset.1);
        if let Some(float) = float {
            let near = (float.0 - at.0).hypot(float.1 - at.1);
            if near < GATHER_DISTANCE {
                let pull = GATHER * (1.0 - near / GATHER_DISTANCE);
                at = (at.0 + (float.0 - at.0) * pull, at.1 + (float.1 - at.1) * pull);
            }
        }
        // Stay in the water: a fish over ground keeps to its leader's path.
        let Some(surface) = water_level(&store, at.0, at.1) else { continue };
        let heading = (to.1 - from.1).atan2(to.0 - from.0);
        *transform = Transform {
            translation: to_bevy(at.0, at.1, surface + LIFT),
            // (The mesh faces +X, which is world x; turning about Y by -heading
            // points it along the world direction (dx, dy).)
            rotation: Quat::from_rotation_y(-heading),
            scale: Vec3::splat(fish.length),
        };
    }
}
