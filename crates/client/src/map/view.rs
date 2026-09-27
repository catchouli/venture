//! A view of the world map in the interface: [`MapView`] on a UI node (use
//! [`map_view`]). The minimap and the full map are both one.
//!
//! Each frame a view draws, clipped to its node:
//! - **Tiles**, in three layers: the whole world at the coarsest level
//!   underneath (so nothing is ever blank while tiles load), then the two
//!   levels nearest the zoom, the finer one faded by how far the zoom is
//!   from it. Zooming then blends smoothly from one level to the next.
//! - **Place names**, by kind ([`LabelKind`]): each kind shows over its own
//!   range of zooms (fading in and out at the ends), sized with the zoom but
//!   kept readable.
//! - **Markers**: everything with a [`MapMarker`], where it is.
//!
//! The mouse wheel over a view zooms it (towards the mouse, if
//! [`MapView::zoom_at_cursor`]); dragging a [`MapView::draggable`] view moves it.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use venture_world::LabelKind;

use crate::characters::player::LocalPlayer;
use crate::common::coords::from_bevy;
use crate::common::procedural_image::{distance_to_polygon_edge, inside_polygon, paint_image};
use crate::map::MapMarker;
use crate::map::tiles::{MapTiles, PlaceNames, TileKey};
use crate::world::source::WorldSource;

/// How much one step of the mouse wheel zooms.
const ZOOM_STEP: f32 = 1.2;
/// Size of the player's arrow, in pixels.
const ARROW_SIZE: f32 = 20.0;
/// The colour behind the tiles (the sea, and the world's edge).
const BACKGROUND: Color = Color::srgb(0.13, 0.27, 0.45);

/// Draws map views.
pub struct MapViewPlugin;

impl Plugin for MapViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_arrow).add_systems(
            Update,
            (build_views, (zoom_and_drag, follow_player), draw_tiles, draw_labels, draw_markers).chain(),
        );
    }
}

/// A view of the map.
#[derive(Component, Clone, Debug)]
pub struct MapView {
    /// The world point in the middle of the view, in metres.
    pub centre: Vec2,
    /// How zoomed in it is: metres a pixel covers.
    pub metres_per_pixel: f32,
    /// The closest it zooms in (metres a pixel).
    pub closest: f32,
    /// The furthest it zooms out (metres a pixel).
    pub furthest: f32,
    /// Whether it stays centred on the player's character.
    pub follow: bool,
    /// Whether the wheel zooms towards the mouse (rather than the middle).
    pub zoom_at_cursor: bool,
    /// Whether it can be dragged about.
    pub draggable: bool,
    /// How big place names are written in it (1 is their usual size).
    pub label_scale: f32,
}

/// What a map view needs on its node: the view, a node that clips what's
/// drawn in it (sized and placed by `node`), and what notices the mouse.
pub fn map_view(view: MapView, node: Node) -> impl Bundle {
    (
        view,
        Node { overflow: Overflow::clip(), ..node },
        BackgroundColor(BACKGROUND),
        // (Being noticed by the mouse also stops the wheel zooming the camera.)
        Interaction::default(),
        RelativeCursorPosition::default(),
    )
}

/// The parts of a view, made when it appears.
#[derive(Component)]
struct ViewParts {
    /// Where things are drawn, bottom to top: the three tile layers, place
    /// names, and markers.
    layers: [Entity; 5],
    /// The tiles shown, by layer and tile.
    tiles: HashMap<(usize, TileKey), Entity>,
    /// Each place name's node (in the order of the labels).
    labels: Vec<Entity>,
    /// Each marked thing's marker.
    markers: HashMap<Entity, Entity>,
    /// Where the mouse was last frame while dragging.
    dragging: Option<Vec2>,
}

/// The arrow for the player's marker.
#[derive(Resource)]
struct Arrow(Handle<Image>);

/// Paints the player's arrow: pale gold with a dark outline, pointing up.
fn make_arrow(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let shape = [Vec2::new(0.0, -0.9), Vec2::new(0.7, 0.75), Vec2::new(0.0, 0.35), Vec2::new(-0.7, 0.75)];
    let image = paint_image(48, RenderAssetUsages::RENDER_WORLD, |u, v| {
        let point = Vec2::new(u, v);
        let edge = distance_to_polygon_edge(point, &shape);
        if inside_polygon(point, &shape) {
            if edge < 0.14 { [0.1, 0.08, 0.05, 1.0] } else { [1.0, 0.86, 0.35, 1.0] }
        } else {
            // A soft dark halo, so it shows on any ground.
            [0.0, 0.0, 0.0, (0.5 - edge * 3.0).max(0.0)]
        }
    });
    commands.insert_resource(Arrow(images.add(image)));
}

/// Gives each new view its layers.
fn build_views(mut commands: Commands, views: Query<Entity, Added<MapView>>) {
    for view in &views {
        let layers = std::array::from_fn(|_| {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        right: px(0),
                        top: px(0),
                        bottom: px(0),
                        ..default()
                    },
                    Pickable::IGNORE,
                    ChildOf(view),
                ))
                .id()
        });
        commands.entity(view).insert(ViewParts {
            layers,
            tiles: HashMap::new(),
            labels: Vec::new(),
            markers: HashMap::new(),
            dragging: None,
        });
    }
}

/// A view's size in logical pixels.
fn size_of(node: &ComputedNode) -> Vec2 {
    node.size() * node.inverse_scale_factor()
}

/// The wheel zooms a view under the mouse; dragging moves a draggable one.
fn zoom_and_drag(
    scroll: Res<AccumulatedMouseScroll>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    names: Res<PlaceNames>,
    mut views: Query<(&mut MapView, &mut ViewParts, &ComputedNode, &RelativeCursorPosition, &Interaction)>,
) {
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    let cursor = window.cursor_position();
    for (mut view, mut parts, node, relative, interaction) in &mut views {
        let size = size_of(node);
        if relative.cursor_over() && lines != 0.0 {
            // (Zoomed all the way out, the whole world fits.)
            let whole_world = names.world_size / size.min_element().max(1.0);
            let furthest = view.furthest.min(whole_world.max(view.closest));
            let zoomed = (view.metres_per_pixel * ZOOM_STEP.powf(-lines)).clamp(view.closest, furthest);
            if view.zoom_at_cursor
                && let Some(at) = relative.normalized
            {
                // Keep the point under the mouse where it is (which means
                // no longer following the player).
                let offset = at * size;
                let before = view.metres_per_pixel;
                view.centre += offset * (before - zoomed);
                view.follow = false;
            }
            view.metres_per_pixel = zoomed;
        }
        if !view.draggable {
            continue;
        }
        match (*interaction == Interaction::Pressed && mouse.pressed(MouseButton::Left), cursor, parts.dragging) {
            (true, Some(now), Some(before)) => {
                let moved = now - before;
                let metres = view.metres_per_pixel;
                view.centre -= moved * metres;
                view.follow = false;
                parts.dragging = Some(now);
            }
            (true, Some(now), None) => parts.dragging = Some(now),
            _ => parts.dragging = None,
        }
    }
}

/// Views that follow the player stay centred on them.
fn follow_player(player: Option<Single<&Transform, With<LocalPlayer>>>, mut views: Query<&mut MapView>) {
    let Some(player) = player else { return };
    let (x, y) = from_bevy(player.translation);
    for mut view in &mut views {
        if view.follow {
            view.centre = Vec2::new(x, y);
        }
    }
}

/// Shows the tiles each view needs.
fn draw_tiles(
    mut commands: Commands,
    mut tiles: ResMut<MapTiles>,
    names: Res<PlaceNames>,
    source: Res<WorldSource>,
    mut views: Query<(&MapView, &mut ViewParts, &ComputedNode)>,
    mut shown: Query<(&mut Node, &mut ImageNode)>,
) {
    let Some(info) = names.info else { return };
    let top = info.levels - 1;
    for (view, mut parts, node) in &mut views {
        let size = size_of(node);
        // The zoom as a level (fractional): 0 is the finest.
        let level = (view.metres_per_pixel / info.finest_metres_per_pixel).log2().clamp(0.0, top as f32);
        let fine = level.floor() as u32;
        let coarse = (fine + 1).min(top);
        let fade = 1.0 - level.fract();
        let mut wanted: Vec<(usize, u32, f32)> = vec![(0, top, 1.0)];
        if coarse != top {
            wanted.push((1, coarse, 1.0));
        }
        if fine != top && fade > 0.01 {
            wanted.push((2, fine, fade));
        }

        let mut keep = Vec::new();
        for (layer, level, alpha) in wanted {
            let tile_metres = info.tile_metres(level);
            let per_side = info.tiles_per_side(level, names.world_size as i32);
            let half = size * 0.5 * view.metres_per_pixel;
            let range = |low: f32, high: f32| {
                let first = (low / tile_metres).floor().max(0.0) as u32;
                let last = ((high / tile_metres).floor().max(0.0) as u32).min(per_side - 1);
                first..=last
            };
            for ty in range(view.centre.y - half.y, view.centre.y + half.y) {
                for tx in range(view.centre.x - half.x, view.centre.x + half.x) {
                    let key = (level, tx, ty);
                    let Some(image) = tiles.get(key, &source) else { continue };
                    let corner = (Vec2::new(tx as f32, ty as f32) * tile_metres - view.centre) / view.metres_per_pixel
                        + size * 0.5;
                    // (A pixel's overlap hides seams between tiles.)
                    let across = tile_metres / view.metres_per_pixel + 1.0;
                    let place = Node {
                        position_type: PositionType::Absolute,
                        left: px(corner.x),
                        top: px(corner.y),
                        width: px(across),
                        height: px(across),
                        ..default()
                    };
                    let tint = Color::srgba(1.0, 1.0, 1.0, alpha);
                    match parts.tiles.get(&(layer, key)).and_then(|&entity| shown.get_mut(entity).ok()) {
                        Some((mut node, mut picture)) => {
                            node.set_if_neq(place);
                            if picture.color != tint {
                                picture.color = tint;
                            }
                        }
                        None => {
                            let entity = commands
                                .spawn((
                                    place,
                                    ImageNode::new(image).with_color(tint),
                                    Pickable::IGNORE,
                                    ChildOf(parts.layers[layer]),
                                ))
                                .id();
                            parts.tiles.insert((layer, key), entity);
                        }
                    }
                    keep.push((layer, key));
                }
            }
        }
        parts.tiles.retain(|shown, entity| {
            let kept = keep.contains(shown);
            if !kept {
                commands.entity(*entity).despawn();
            }
            kept
        });
    }
}

/// How a kind of place name is written, and at which zooms.
struct LabelStyle {
    /// Its size at its reference zoom, in pixels.
    size: f32,
    /// The zoom (metres a pixel) at which it's `size`: it grows a little
    /// closer in and shrinks a little further out.
    reference: f32,
    colour: Color,
    /// Written in capitals.
    capitals: bool,
    /// Written just above its place (a town's name, leaving the town
    /// itself, and whoever's in it, clear), rather than over it.
    above: bool,
    /// Zooms (metres a pixel) it shows at: fading in from the first to the
    /// second, fully shown to the third, fading out by the fourth.
    shown: [f32; 4],
}

fn style_of(kind: LabelKind) -> LabelStyle {
    match kind {
        LabelKind::Region => LabelStyle {
            size: 17.0,
            reference: 6.0,
            colour: Color::srgba(0.98, 0.92, 0.74, 0.85),
            capitals: true,
            above: false,
            shown: [2.6, 4.0, f32::INFINITY, f32::INFINITY],
        },
        LabelKind::Area => LabelStyle {
            size: 13.0,
            reference: 1.5,
            colour: Color::srgb(1.0, 1.0, 0.97),
            capitals: false,
            above: true,
            shown: [0.0, 0.0, 5.0, 8.0],
        },
        LabelKind::Place => LabelStyle {
            size: 11.0,
            reference: 0.6,
            colour: Color::srgb(0.84, 0.94, 0.86),
            capitals: false,
            above: false,
            shown: [0.0, 0.0, 1.4, 2.2],
        },
    }
}

/// How much a label shows (0 to 1) at a zoom.
fn label_opacity(style: &LabelStyle, metres_per_pixel: f32) -> f32 {
    let [appear, full, fading, gone] = style.shown;
    let fade_in = if full > appear { ((metres_per_pixel - appear) / (full - appear)).clamp(0.0, 1.0) } else { 1.0 };
    let fade_out = if gone > fading { ((gone - metres_per_pixel) / (gone - fading)).clamp(0.0, 1.0) } else { 1.0 };
    fade_in.min(fade_out)
}

/// A label's text size at a zoom: bigger closer in, but never too small or large to read.
fn label_size(style: &LabelStyle, metres_per_pixel: f32) -> f32 {
    style.size * (style.reference / metres_per_pixel).powf(0.3).clamp(0.8, 1.35)
}

/// Shows the place names in each view.
fn draw_labels(
    mut commands: Commands,
    names: Res<PlaceNames>,
    mut views: Query<(&MapView, &mut ViewParts, &ComputedNode)>,
    mut shown: Query<(&mut Node, &mut TextFont, &mut TextColor, &mut Visibility)>,
) {
    let Some(labels) = &names.labels else { return };
    for (view, mut parts, node) in &mut views {
        if parts.labels.is_empty() {
            let layer = parts.layers[3];
            parts.labels = labels
                .labels
                .iter()
                .map(|label| {
                    let style = style_of(label.kind);
                    let text = if style.capitals { label.name.to_uppercase() } else { label.name.clone() };
                    commands
                        .spawn((
                            Node { position_type: PositionType::Absolute, ..default() },
                            // Centred on its place, or just above it.
                            UiTransform::from_translation(Val2::percent(
                                -50.0,
                                if style.above { -130.0 } else { -50.0 },
                            )),
                            Text::new(text),
                            TextFont { font_size: FontSize::Px(style.size), ..default() },
                            TextColor(style.colour),
                            TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
                            TextLayout::no_wrap(),
                            Visibility::Hidden,
                            Pickable::IGNORE,
                            ChildOf(layer),
                        ))
                        .id()
                })
                .collect();
            continue;
        }
        let size = size_of(node);
        // Where names have been written, so later (smaller) ones that would
        // overlap them are left out.
        let mut taken: Vec<Rect> = Vec::new();
        for (label, &entity) in labels.labels.iter().zip(&parts.labels) {
            let Ok((mut place, mut font, mut colour, mut visibility)) = shown.get_mut(entity) else { continue };
            let style = style_of(label.kind);
            let opacity = label_opacity(&style, view.metres_per_pixel);
            let at = (Vec2::new(label.at.0, label.at.1) - view.centre) / view.metres_per_pixel + size * 0.5;
            let text_size = (label_size(&style, view.metres_per_pixel) * view.label_scale * 2.0).round() / 2.0;
            let area = label_area(&label.name, &style, text_size, at);
            let inside = area.max.x > 0.0 && area.max.y > 0.0 && area.min.x < size.x && area.min.y < size.y;
            let clear = !taken.iter().any(|other| !other.intersect(area).is_empty());
            let wanted = if opacity > 0.01 && inside && clear { Visibility::Inherited } else { Visibility::Hidden };
            visibility.set_if_neq(wanted);
            if wanted == Visibility::Hidden {
                continue;
            }
            taken.push(area);
            if place.left != px(at.x) || place.top != px(at.y) {
                place.left = px(at.x);
                place.top = px(at.y);
            }
            if font.font_size != FontSize::Px(text_size) {
                font.font_size = FontSize::Px(text_size);
            }
            colour.set_if_neq(TextColor(style.colour.with_alpha(style.colour.alpha() * opacity)));
        }
    }
}

/// Roughly where a name is written (the font is monospaced, so its width
/// is about its length), with a little room around it.
fn label_area(name: &str, style: &LabelStyle, text_size: f32, at: Vec2) -> Rect {
    let size = Vec2::new(name.chars().count() as f32 * text_size * 0.6, text_size * 1.2) + Vec2::splat(6.0);
    let middle = if style.above { at - Vec2::new(0.0, size.y * 0.8) } else { at };
    Rect::from_center_size(middle, size)
}

/// Shows each marked thing in each view.
fn draw_markers(
    mut commands: Commands,
    arrow: Res<Arrow>,
    marked: Query<(Entity, &MapMarker, &Transform)>,
    mut views: Query<(&MapView, &mut ViewParts, &ComputedNode)>,
    mut shown: Query<(&mut Node, &mut UiTransform)>,
) {
    for (view, mut parts, node) in &mut views {
        let size = size_of(node);
        let layer = parts.layers[4];
        for (thing, marker, transform) in &marked {
            let (x, y) = from_bevy(transform.translation);
            let at = (Vec2::new(x, y) - view.centre) / view.metres_per_pixel + size * 0.5;
            // The way it faces (its +Z), clockwise from north.
            let facing = transform.rotation * Vec3::Z;
            let (east, south) = from_bevy(facing);
            let turn = UiTransform {
                translation: Val2::percent(-50.0, -50.0),
                rotation: Rot2::radians(east.atan2(-south)),
                ..default()
            };
            let place =
                Node { position_type: PositionType::Absolute, left: px(at.x), top: px(at.y), ..marker_size(*marker) };
            match parts.markers.get(&thing).and_then(|&entity| shown.get_mut(entity).ok()) {
                Some((mut node, mut transform)) => {
                    node.set_if_neq(place);
                    transform.set_if_neq(turn);
                }
                None => {
                    let entity = commands
                        .spawn((place, turn, ImageNode::new(arrow.0.clone()), Pickable::IGNORE, ChildOf(layer)))
                        .id();
                    parts.markers.insert(thing, entity);
                }
            }
        }
        parts.markers.retain(|thing, entity| {
            let still_there = marked.contains(*thing);
            if !still_there {
                commands.entity(*entity).despawn();
            }
            still_there
        });
    }
}

/// How big a marker is drawn.
fn marker_size(marker: MapMarker) -> Node {
    match marker {
        MapMarker::Player => Node { width: px(ARROW_SIZE), height: px(ARROW_SIZE), ..default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_names_fade_in_and_out_by_zoom() {
        let region = style_of(LabelKind::Region);
        assert_eq!(label_opacity(&region, 1.0), 0.0, "regions don't clutter close up");
        assert_eq!(label_opacity(&region, 16.0), 1.0);
        let place = style_of(LabelKind::Place);
        assert_eq!(label_opacity(&place, 1.0), 1.0);
        assert!(label_opacity(&place, 1.8) > 0.0 && label_opacity(&place, 1.8) < 1.0);
        assert_eq!(label_opacity(&place, 4.0), 0.0, "places vanish zoomed out");
        // Sizes change with the zoom, within limits.
        assert!(label_size(&place, 0.3) > label_size(&place, 1.2));
        assert!(label_size(&place, 100.0) >= place.size * 0.8);
    }
}
