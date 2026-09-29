//! The world's solid shapes, for predicting the player's own movement (see
//! `venture_collision`): kept in step with the loaded chunks, the walls and
//! buildings, and the felled trees, just as the server keeps its own.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use venture_collision::Collision;
use venture_protocol::TreeId;
use venture_world::{Chunk, ChunkPos};

use crate::content::ContentStore;
use crate::world::source::WorldInfo;
use crate::world::streaming::ChunkStore;
use crate::world::trees::FelledTrees;

/// Keeps the solid world in step with what's loaded.
pub struct CollisionPlugin;

impl Plugin for CollisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Solid>()
            .init_resource::<Loaded>()
            .add_systems(Update, (follow_structures, follow_chunks, follow_felling).chain().in_set(SolidSystems));
    }
}

/// The systems that keep [`Solid`] up to date (movement runs after them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SolidSystems;

/// The solid world, as far as it's loaded.
#[derive(Resource, Default)]
pub struct Solid(pub Collision);

/// Which chunk data each loaded chunk was made from, and which felled trees
/// are out of the way.
#[derive(Resource, Default)]
struct Loaded {
    chunks: HashMap<ChunkPos, Arc<Chunk>>,
    felled: HashSet<TreeId>,
}

/// The walls and buildings, when the world or content changes.
fn follow_structures(world: Option<Res<WorldInfo>>, content: Res<ContentStore>, mut solid: ResMut<Solid>) {
    let Some(world) = world else { return };
    if world.is_changed() || content.is_changed() {
        solid.0.set_structures(&world.0.walls, &world.0.buildings, &content.building_styles);
    }
}

/// Chunks come and go with the loaded ones (and are made again when their data changes).
fn follow_chunks(
    store: Res<ChunkStore>,
    felled: Res<FelledTrees>,
    mut solid: ResMut<Solid>,
    mut loaded: ResMut<Loaded>,
) {
    if !store.is_changed() {
        return;
    }
    let gone: Vec<ChunkPos> = loaded
        .chunks
        .iter()
        .filter(|(pos, chunk)| store.0.get(pos).is_none_or(|now| !Arc::ptr_eq(now, chunk)))
        .map(|(&pos, _)| pos)
        .collect();
    for pos in gone {
        solid.0.remove_chunk(pos);
        loaded.chunks.remove(&pos);
    }
    for (&pos, chunk) in &store.0 {
        if loaded.chunks.contains_key(&pos) {
            continue;
        }
        solid.0.add_chunk(chunk);
        for tree in felled.0.keys().filter(|tree| (tree.chunk_x, tree.chunk_y) == (pos.x, pos.y)) {
            solid.0.set_prop_standing(pos, tree.index, false);
        }
        loaded.chunks.insert(pos, chunk.clone());
    }
}

/// Felled trees are out of the way, until they grow back.
fn follow_felling(felled: Res<FelledTrees>, mut solid: ResMut<Solid>, mut loaded: ResMut<Loaded>) {
    if !felled.is_changed() {
        return;
    }
    let now: HashSet<TreeId> = felled.0.keys().copied().collect();
    for &tree in now.difference(&loaded.felled) {
        solid.0.set_prop_standing(ChunkPos::new(tree.chunk_x, tree.chunk_y), tree.index, false);
    }
    for &tree in loaded.felled.difference(&now) {
        solid.0.set_prop_standing(ChunkPos::new(tree.chunk_x, tree.chunk_y), tree.index, true);
    }
    loaded.felled = now;
}
