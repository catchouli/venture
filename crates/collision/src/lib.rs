//! The world's collision: its solid shapes (see [`shapes`]), and moving a
//! character's capsule through them with Rapier's kinematic character
//! controller (sliding along walls, stepping up kerbs, sliding down slopes
//! too steep to climb). See `docs/plan-collision.md`.
//!
//! There's no physics simulation, only queries, so a [`Collision`] is just
//! Rapier's collider set and a bounding volume hierarchy over it, kept up to
//! date as chunks come and go.
//!
//! Deep water and lava aren't solid: they're cells no one may enter (see
//! [`Collision::no_entry`]).

pub mod shapes;

use std::collections::HashMap;

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::parry::partitioning::Bvh;
use rapier3d::parry::query::{DefaultQueryDispatcher, ShapeCastOptions};
use rapier3d::parry::shape::Capsule;
use rapier3d::pipeline::{QueryFilter, QueryPipeline};
use rapier3d::prelude::{Collider, ColliderHandle, ColliderSet, IslandManager, Ray, RigidBodySet, SoftBodySet};
use venture_structures::buildings::BuildingStyle;
use venture_world::{BakedBuilding, BakedWall, CHUNK_SIZE, CellPos, Chunk, ChunkPos, FluidKind};

pub use rapier3d::math::Vector;
pub use shapes::to_rapier;

/// A character's capsule: its radius, in metres.
pub const CHARACTER_RADIUS: f32 = 0.3;
/// A character's capsule: its height (feet to crown), in metres.
pub const CHARACTER_HEIGHT: f32 = 1.8;
/// The deepest water a character can wade through, in metres.
pub const MAX_WADING_DEPTH: f32 = 0.6;
/// How far under a character's feet counts as standing on the ground, in metres.
const GROUND_REACH: f32 = 0.1;

/// How a character moves through the world (see [`Collision::move_character`]).
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterSettings {
    /// The steepest slope a character can walk up, in degrees.
    pub max_climb_degrees: f32,
    /// Slopes at least this steep slide characters down them, in degrees.
    pub min_slide_degrees: f32,
    /// The highest ledge a character steps up without jumping, in metres.
    pub step_height: f32,
    /// How far down a character running downhill keeps to the ground, in metres.
    pub snap_to_ground: f32,
}

impl Default for CharacterSettings {
    fn default() -> Self {
        Self { max_climb_degrees: 50.0, min_slide_degrees: 50.0, step_height: 0.4, snap_to_ground: 0.5 }
    }
}

/// Where a character ended up after a move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    /// Its feet: world `(x, y)`, and height.
    pub feet: (f32, f32, f32),
    /// Whether it's standing on something.
    pub grounded: bool,
    /// Whether it's sliding down a slope too steep to stand on.
    pub sliding: bool,
}

/// What's loaded of one chunk.
#[derive(Default)]
struct LoadedChunk {
    /// Its ground's collider.
    ground: Option<ColliderHandle>,
    /// Its props' colliders, by their index among its props (and those taken
    /// away, such as felled trees, kept to put back).
    props: HashMap<u16, (Option<ColliderHandle>, Collider)>,
    /// Its cells no one may enter (deep water, lava), `y * CHUNK_SIZE + x`.
    no_entry: Vec<bool>,
}

/// The solid world, as far as it's loaded.
pub struct Collision {
    colliders: ColliderSet,
    bodies: RigidBodySet,
    islands: IslandManager,
    soft_bodies: SoftBodySet,
    bvh: Bvh,
    chunks: HashMap<ChunkPos, LoadedChunk>,
    structures: Vec<ColliderHandle>,
}

impl Default for Collision {
    fn default() -> Self {
        Self::new()
    }
}

impl Collision {
    /// An empty world (nothing loaded).
    pub fn new() -> Self {
        Self {
            colliders: ColliderSet::new(),
            bodies: RigidBodySet::new(),
            islands: IslandManager::new(),
            soft_bodies: SoftBodySet::new(),
            bvh: Bvh::new(),
            chunks: HashMap::new(),
            structures: Vec::new(),
        }
    }

    fn insert(&mut self, collider: Collider) -> ColliderHandle {
        let aabb = collider.compute_aabb();
        let handle = self.colliders.insert(collider);
        self.bvh.insert(aabb, leaf(handle));
        handle
    }

    fn remove(&mut self, handle: ColliderHandle) -> Option<Collider> {
        self.bvh.remove(leaf(handle));
        self.colliders.remove(handle, &mut self.islands, &mut self.bodies, &mut self.soft_bodies, false)
    }

    /// Loads a chunk's ground and props (replacing it if it's loaded already).
    pub fn add_chunk(&mut self, chunk: &Chunk) {
        self.remove_chunk(chunk.pos);
        let ground = self.insert(shapes::ground(chunk));
        let mut props = HashMap::new();
        for (index, prop) in chunk.props.iter().enumerate() {
            if let Some(collider) = shapes::prop(prop) {
                let handle = self.insert(collider.clone());
                props.insert(index as u16, (Some(handle), collider));
            }
        }
        let no_entry = no_entry(chunk, MAX_WADING_DEPTH);
        self.chunks.insert(chunk.pos, LoadedChunk { ground: Some(ground), props, no_entry });
    }

    /// Unloads a chunk (if it's loaded).
    pub fn remove_chunk(&mut self, pos: ChunkPos) {
        let Some(loaded) = self.chunks.remove(&pos) else { return };
        let handles = loaded.ground.into_iter().chain(loaded.props.into_values().filter_map(|(handle, _)| handle));
        for handle in handles.collect::<Vec<_>>() {
            self.remove(handle);
        }
    }

    /// Whether the chunk under a point (world `(x, y)`) is loaded.
    pub fn is_loaded_at(&self, (x, y): (f32, f32)) -> bool {
        self.has_chunk(CellPos::new(x.floor() as i32, y.floor() as i32).chunk())
    }

    /// Whether a chunk is loaded.
    pub fn has_chunk(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }

    /// The loaded chunks.
    pub fn chunks(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.chunks.keys().copied()
    }

    /// Takes a prop away (a tree felled) or puts it back (grown again), by
    /// its chunk and index among the chunk's props. Does nothing if its
    /// chunk isn't loaded, or it's never in the way.
    pub fn set_prop_standing(&mut self, chunk: ChunkPos, index: u16, standing: bool) {
        let Some((handle, collider)) = self
            .chunks
            .get(&chunk)
            .and_then(|loaded| loaded.props.get(&index))
            .map(|(handle, collider)| (*handle, collider.clone()))
        else {
            return;
        };
        let handle = match (handle, standing) {
            (Some(handle), false) => {
                self.remove(handle);
                None
            }
            (None, true) => Some(self.insert(collider.clone())),
            (handle, _) => handle,
        };
        if let Some(entry) = self.chunks.get_mut(&chunk).and_then(|loaded| loaded.props.get_mut(&index)) {
            entry.0 = handle;
        }
    }

    /// Replaces the walls and buildings (with their styles, by id).
    pub fn set_structures(&mut self, walls: &[BakedWall], buildings: &[BakedBuilding], styles: &[BuildingStyle]) {
        for handle in std::mem::take(&mut self.structures) {
            self.remove(handle);
        }
        let mut colliders: Vec<Collider> = walls.iter().flat_map(shapes::wall).collect();
        for building in buildings {
            if let Some(style) = styles.iter().find(|style| style.id == building.plan.style) {
                colliders.extend(shapes::building(building, style));
            }
        }
        self.structures = colliders.into_iter().map(|collider| self.insert(collider)).collect();
    }

    fn queries(&self) -> QueryPipeline<'_> {
        QueryPipeline {
            dispatcher: &DefaultQueryDispatcher,
            bvh: &self.bvh,
            bodies: &self.bodies,
            colliders: &self.colliders,
            filter: QueryFilter::default(),
        }
    }

    /// Whether world cell `(x, y)` may not be entered: deep water or lava,
    /// or not loaded.
    pub fn no_entry(&self, x: i32, y: i32) -> bool {
        let cell = CellPos::new(x, y);
        let Some(loaded) = self.chunks.get(&cell.chunk()) else { return true };
        let (local_x, local_y) = cell.local();
        loaded.no_entry[local_y * CHUNK_SIZE as usize + local_x]
    }

    /// The height of the highest solid surface under a point, looking down
    /// from `from_height`, if there's one within `depth` metres.
    pub fn surface_below(&self, (x, y): (f32, f32), from_height: f32, depth: f32) -> Option<f32> {
        let ray = Ray::new(to_rapier(x, y, from_height), Vector::NEG_Y);
        let (_, distance) = self.queries().cast_ray(&ray, depth, true)?;
        Some(from_height - distance)
    }

    /// Whether a character standing with its feet at `feet` (world `(x, y)`,
    /// height) would be clear of everything solid (and out of cells no one
    /// may enter). The ground under its feet doesn't count.
    pub fn fits(&self, (x, y, height): (f32, f32, f32)) -> bool {
        if self.no_entry(x.floor() as i32, y.floor() as i32) {
            return false;
        }
        // (Lifted a little, so the ground it stands on isn't in the way.)
        let (shape, pose) = capsule_at((x, y, height + 0.05));
        self.queries().intersect_shape(pose, &shape).next().is_none()
    }

    /// Moves a character standing at `feet` by `desired` (world `x`, `y`, and
    /// up) over `dt` seconds, as far as it can: sliding along what it meets,
    /// stepping up low ledges, climbing only slopes it can, and keeping to the
    /// ground as it runs downhill (all as `settings` say). It never enters
    /// cells no one may enter.
    pub fn move_character(
        &self,
        feet: (f32, f32, f32),
        desired: (f32, f32, f32),
        dt: f32,
        settings: &CharacterSettings,
    ) -> Moved {
        let controller = KinematicCharacterController {
            offset: CharacterLength::Absolute(0.02),
            autostep: (settings.step_height > 0.0).then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(settings.step_height),
                min_width: CharacterLength::Absolute(0.3),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: settings.max_climb_degrees.to_radians(),
            min_slope_slide_angle: settings.min_slide_degrees.to_radians(),
            snap_to_ground: Some(CharacterLength::Absolute(settings.snap_to_ground)),
            ..KinematicCharacterController::default()
        };
        let (shape, pose) = capsule_at(feet);
        let movement = controller.move_shape(
            dt,
            &self.queries(),
            &shape,
            &pose,
            to_rapier(desired.0, desired.1, desired.2),
            |_| {},
        );
        let moved = movement.translation;
        let mut end = (feet.0 + moved.x, feet.1 + moved.z, feet.2 + moved.y);
        // Deep water and lava turn characters away, sliding along their edges
        // (unless they're somehow in them already, when they may leave).
        let entering = |(x, y): (f32, f32)| self.no_entry(x.floor() as i32, y.floor() as i32);
        if entering((end.0, end.1)) && !entering((feet.0, feet.1)) {
            let across = (feet.0 + moved.x, feet.1);
            let along = (feet.0, feet.1 + moved.z);
            let open = |(x, y): (f32, f32)| !self.no_entry(x.floor() as i32, y.floor() as i32);
            let (x, y) = [across, along].into_iter().find(|&at| open(at)).unwrap_or((feet.0, feet.1));
            end = (x, y, end.2);
        }
        let grounded = self.stands_on_ground(end, settings);
        Moved { feet: end, grounded, sliding: movement.is_sliding_down_slope }
    }

    /// Whether a character with its feet at `feet` stands on ground it could
    /// climb: something right under it, no steeper than `settings` allow.
    /// (Rapier's own test counts touching almost anything below the
    /// capsule's middle, such as a wall's top edge beside a character
    /// jumping at it.)
    pub fn stands_on_ground(&self, feet: (f32, f32, f32), settings: &CharacterSettings) -> bool {
        let (shape, pose) = capsule_at(feet);
        let options = ShapeCastOptions {
            max_time_of_impact: GROUND_REACH,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        self.queries()
            .cast_shape(&pose, Vector::NEG_Y, &shape, options)
            .is_some_and(|(_, hit)| hit.normal1.y >= settings.max_climb_degrees.to_radians().cos() - 1e-3)
    }
}

/// A character's capsule, and where it is for feet at `feet`.
fn capsule_at((x, y, height): (f32, f32, f32)) -> (Capsule, rapier3d::math::Pose) {
    let half = CHARACTER_HEIGHT / 2.0 - CHARACTER_RADIUS;
    let shape = Capsule::new_y(half, CHARACTER_RADIUS);
    (shape, rapier3d::math::Pose::from_translation(to_rapier(x, y, height + CHARACTER_HEIGHT / 2.0)))
}

/// A collider's leaf in the hierarchy: its index in the set (as the query
/// pipeline looks colliders up by).
fn leaf(handle: ColliderHandle) -> u32 {
    handle.into_raw_parts().0
}

/// A chunk's cells no one may enter: water deeper than `max_wading_depth`, and lava.
pub fn no_entry(chunk: &Chunk, max_wading_depth: f32) -> Vec<bool> {
    let size = CHUNK_SIZE as usize;
    (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            let heights = chunk.cell_corner_heights(x, y);
            let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
            let highest = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let middle = (lowest + highest) / 2.0;
            chunk
                .cell(x, y)
                .fluid
                .is_some_and(|fluid| fluid.kind == FluidKind::Lava || fluid.level - middle > max_wading_depth)
        })
        .collect()
}

#[cfg(test)]
mod tests;
