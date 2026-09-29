//! The world's solid shapes, as characters move through them (see
//! `venture_collision` and `docs/plan-collision.md`): the chunks around every
//! character that moves are loaded (and let go a while after no one's near),
//! with the walls and buildings, and felled trees taken out of the way. The
//! only code that changes what's solid.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use venture_collision::Collision;
use venture_protocol::TreeId;
use venture_world::{CellPos, ChunkPos};

use crate::content::LoadedContent;
use crate::entities::{Mover, Position};
use crate::woodcutting::FelledTrees;
use crate::world::{ServerWorld, WorldReloaded};

/// Chunks within this many chunks of a character are loaded.
const REACH: i32 = 1;
/// Chunks no one has been near for this long (seconds) are let go.
const LET_GO_AFTER: f32 = 30.0;

/// Keeps the solid world loaded where it's needed.
pub struct CollisionPlugin;

impl Plugin for CollisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Solid>().init_resource::<Needed>().add_systems(
            FixedUpdate,
            (follow_world_changes, load_near_characters, follow_felling).chain().in_set(crate::ServerSystems::Sessions),
        );
    }
}

/// The solid world, as far as it's loaded.
#[derive(Resource, Default)]
pub struct Solid(pub Collision);

/// When each loaded chunk was last near a character (seconds of game time),
/// and which felled trees are out of the way.
#[derive(Resource, Default)]
struct Needed {
    last_near: HashMap<ChunkPos, f32>,
    felled: HashSet<TreeId>,
}

/// Loads the walls and buildings (at first, and when the world or content
/// changes), and loads again the chunks that changed in a new bake.
fn follow_world_changes(
    world: Res<ServerWorld>,
    content: Res<LoadedContent>,
    mut reloads: MessageReader<WorldReloaded>,
    mut solid: ResMut<Solid>,
    mut needed: ResMut<Needed>,
) {
    if world.is_changed() || content.is_changed() {
        let manifest = &world.manifest;
        solid.0.set_structures(&manifest.walls, &manifest.buildings, &content.pack.building_styles);
    }
    for reload in reloads.read() {
        for &pos in &reload.changed {
            if solid.0.has_chunk(pos) {
                solid.0.remove_chunk(pos);
                needed.last_near.remove(&pos);
            }
        }
        // (The felled trees in them are forgotten too; see woodcutting.)
        needed.felled.retain(|tree| !reload.changed.contains(&ChunkPos::new(tree.chunk_x, tree.chunk_y)));
    }
}

/// Loads the chunks round every character that moves, and lets go of those
/// no one has been near for a while.
fn load_near_characters(
    time: Res<Time>,
    world: Res<ServerWorld>,
    felled: Res<FelledTrees>,
    mut solid: ResMut<Solid>,
    mut needed: ResMut<Needed>,
    movers: Query<&Position, With<Mover>>,
) {
    let now = time.elapsed_secs();
    let mut near = HashSet::new();
    for &Position((x, y)) in &movers {
        let middle = CellPos::new(x.floor() as i32, y.floor() as i32).chunk();
        for dy in -REACH..=REACH {
            for dx in -REACH..=REACH {
                let pos = ChunkPos::new(middle.x + dx, middle.y + dy);
                if world.manifest.contains_chunk(pos) {
                    near.insert(pos);
                }
            }
        }
    }
    for &pos in &near {
        needed.last_near.insert(pos, now);
        if solid.0.has_chunk(pos) {
            continue;
        }
        let Some(chunk) = world.chunk(pos) else { continue };
        solid.0.add_chunk(&chunk);
        for tree in felled.trees().filter(|tree| (tree.chunk_x, tree.chunk_y) == (pos.x, pos.y)) {
            solid.0.set_prop_standing(pos, tree.index, false);
        }
    }
    let stale: Vec<ChunkPos> =
        needed.last_near.iter().filter(|&(_, &last)| now - last > LET_GO_AFTER).map(|(&pos, _)| pos).collect();
    for pos in stale {
        needed.last_near.remove(&pos);
        solid.0.remove_chunk(pos);
    }
}

/// Felled trees are out of the way, until they grow back.
fn follow_felling(felled: Res<FelledTrees>, mut solid: ResMut<Solid>, mut needed: ResMut<Needed>) {
    if !felled.is_changed() {
        return;
    }
    let now: HashSet<TreeId> = felled.trees().collect();
    for &tree in now.difference(&needed.felled) {
        solid.0.set_prop_standing(ChunkPos::new(tree.chunk_x, tree.chunk_y), tree.index, false);
    }
    for &tree in needed.felled.difference(&now) {
        solid.0.set_prop_standing(ChunkPos::new(tree.chunk_x, tree.chunk_y), tree.index, true);
    }
    needed.felled = now;
}
