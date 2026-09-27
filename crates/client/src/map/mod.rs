//! The map: a minimap in the top-right corner, and the full map (**M**).
//!
//! Both are a [`view::MapView`]: the world map's tiles (baked with the world:
//! see `venture_world::MapInfo` and `docs/plan-map.md`), fetched as needed
//! ([`tiles`]), with the place names written over them, sized and shown by
//! zoom, and markers for things in the world that have a [`MapMarker`]
//! (only the player's own character, so far).
//!
//! - [`minimap`]: follows the player; the mouse wheel zooms it, and its
//!   button opens the full map.
//! - [`window`]: the full map, opened with **M** or the minimap's button;
//!   the wheel zooms towards the mouse, and dragging moves it.

pub mod minimap;
pub mod tiles;
pub mod view;
pub mod window;

use bevy::prelude::*;

use crate::characters::player::LocalPlayer;

/// Shows the minimap and the map.
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((tiles::MapTilesPlugin, view::MapViewPlugin, minimap::MinimapPlugin, window::MapWindowPlugin))
            .add_systems(Update, mark_the_player);
    }
}

/// Something shown on maps, where it is in the world (its [`Transform`]).
/// Add one to anything that should appear on the minimap and map.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapMarker {
    /// The player's own character: an arrow pointing the way they face.
    Player,
}

/// Puts a marker on the player's character.
fn mark_the_player(mut commands: Commands, players: Query<Entity, (With<LocalPlayer>, Without<MapMarker>)>) {
    for player in &players {
        commands.entity(player).insert(MapMarker::Player);
    }
}
