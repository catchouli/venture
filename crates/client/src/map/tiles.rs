//! Fetching the world map's tiles and place names (see
//! `venture_world::MapInfo`), from wherever the world comes from
//! ([`WorldSource`]: the server, or a folder in `--local` mode).
//!
//! Views ask for the tiles they need each frame ([`MapTiles::get`]); a tile
//! not here yet starts downloading (a few at a time) and shows once it has
//! arrived and been decoded (off the main thread). Tiles no view has used for
//! a while are dropped when there are too many.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use venture_world::{MAP_LABELS_FILE, MapInfo, MapLabels, map_tile_path};

use crate::world::source::{WorldInfo, WorldReloaded, WorldSource};

/// Tiles downloading at once.
const PARALLEL_DOWNLOADS: usize = 6;
/// Tiles kept before the least recently used are dropped.
const KEPT_TILES: usize = 400;

/// Fetches map tiles and place names.
pub struct MapTilesPlugin;

impl Plugin for MapTilesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapTiles>()
            .init_resource::<PlaceNames>()
            .add_systems(Update, (redraw_when_rebaked, load_place_names, receive_tiles, forget_old_tiles).chain());
    }
}

/// A tile: its level, and which it is across and down.
pub type TileKey = (u32, u32, u32);

/// The map's tiles, loaded and loading.
#[derive(Resource, Default)]
pub struct MapTiles {
    tiles: HashMap<TileKey, TileState>,
    /// Counts frames, to know which tiles were used lately.
    frame: u64,
}

enum TileState {
    Loading(Task<Result<Image, String>>),
    Ready {
        image: Handle<Image>,
        used: u64,
    },
    /// It couldn't be loaded (it isn't asked for again).
    Failed,
}

impl MapTiles {
    /// A tile's picture, if it's here; if it isn't, starts fetching it
    /// (when there's room to).
    pub fn get(&mut self, key: TileKey, source: &WorldSource) -> Option<Handle<Image>> {
        let frame = self.frame;
        if let Some(state) = self.tiles.get_mut(&key) {
            return match state {
                TileState::Ready { image, used } => {
                    *used = frame;
                    Some(image.clone())
                }
                _ => None,
            };
        }
        let loading = self.tiles.values().filter(|state| matches!(state, TileState::Loading(_))).count();
        if loading < PARALLEL_DOWNLOADS {
            let (level, x, y) = key;
            let read = source.read(map_tile_path(level, x, y));
            let task = IoTaskPool::get().spawn(async move { decode_tile(&read.await?) });
            self.tiles.insert(key, TileState::Loading(task));
        }
        None
    }
}

/// A tile's JPEG, as an image to draw.
fn decode_tile(bytes: &[u8]) -> Result<Image, String> {
    Image::from_buffer(
        bytes,
        ImageType::Extension("jpg"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|error| error.to_string())
}

fn receive_tiles(mut tiles: ResMut<MapTiles>, mut images: ResMut<Assets<Image>>) {
    let frame = tiles.frame;
    for (key, state) in &mut tiles.tiles {
        let TileState::Loading(task) = state else { continue };
        let Some(result) = check_ready(task) else { continue };
        *state = match result {
            Ok(image) => TileState::Ready { image: images.add(image), used: frame },
            Err(error) => {
                warn!("Couldn't load map tile {key:?}: {error}");
                TileState::Failed
            }
        };
    }
}

/// When the world's been baked again, its map was drawn again: every tile
/// and the place names are read again.
fn redraw_when_rebaked(
    mut reloaded: MessageReader<WorldReloaded>,
    mut tiles: ResMut<MapTiles>,
    mut names: ResMut<PlaceNames>,
) {
    if reloaded.read().count() > 0 {
        tiles.tiles.clear();
        *names = PlaceNames::default();
    }
}

/// Drops the least recently used tiles when there are too many (they're
/// fetched again if needed: from the browser's cache, on the web).
fn forget_old_tiles(mut tiles: ResMut<MapTiles>) {
    tiles.frame += 1;
    let mut ready: Vec<(TileKey, u64)> = tiles
        .tiles
        .iter()
        .filter_map(|(key, state)| match state {
            TileState::Ready { used, .. } => Some((*key, *used)),
            _ => None,
        })
        .collect();
    if ready.len() <= KEPT_TILES {
        return;
    }
    ready.sort_by_key(|(_, used)| *used);
    for (key, _) in &ready[..ready.len() - KEPT_TILES] {
        tiles.tiles.remove(key);
    }
}

/// The world map's layout and place names, once known.
#[derive(Resource, Default)]
pub struct PlaceNames {
    /// The map's layout (none if the world has no map).
    pub info: Option<MapInfo>,
    /// How wide the world is, in metres.
    pub world_size: f32,
    /// The names, once they've loaded.
    pub labels: Option<MapLabels>,
    loading: Option<Task<Result<Vec<u8>, String>>>,
}

fn load_place_names(world: Option<Res<WorldInfo>>, source: Res<WorldSource>, mut names: ResMut<PlaceNames>) {
    let Some(world) = world else { return };
    if names.info.is_none()
        && let Some(info) = world.0.map
    {
        names.info = Some(info);
        names.world_size = world.0.world_size as f32;
        names.loading = Some(IoTaskPool::get().spawn(source.read(MAP_LABELS_FILE.to_string())));
    }
    let Some(task) = names.loading.as_mut() else { return };
    let Some(result) = check_ready(task) else { return };
    names.loading = None;
    let labels = result.and_then(|bytes| MapLabels::from_ron(&String::from_utf8_lossy(&bytes)));
    match labels {
        Ok(labels) => names.labels = Some(labels),
        Err(error) => warn!("Couldn't load the map's place names: {error}"),
    }
}
