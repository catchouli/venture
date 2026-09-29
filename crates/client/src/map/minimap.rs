//! The minimap, in the top-right corner: a square view of the map around
//! the player, who's always in the middle. Clicking it runs there (as far as a
//! click on the world can), the mouse wheel over it zooms, and the button in
//! its bottom-right corner opens the full map.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::interaction::orders::Orders;
use crate::map::view::{MapView, map_view, point_under_mouse};
use crate::map::window::OpenMap;
use crate::network::ClientState;
use crate::ui::theme::{Piece, Themed};
use crate::ui::tooltip::Tooltip;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER};

/// Width (and height) of the map inside the frame, in pixels.
const SIZE: f32 = 190.0;
/// How far in it starts (metres a pixel), and how far it zooms each way.
const START_ZOOM: f32 = 1.0;
const CLOSEST: f32 = 0.3;
const FURTHEST: f32 = 6.0;

/// Shows the minimap.
pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_minimap).add_systems(Update, (show_while_playing, open_map, run_to_click));
    }
}

/// The minimap's frame.
#[derive(Component)]
struct Minimap;

/// The map inside it.
#[derive(Component)]
struct MinimapView;

/// The button that opens the full map.
#[derive(Component)]
struct MapButton;

fn spawn_minimap(mut commands: Commands) {
    let view = MapView {
        centre: Vec2::ZERO,
        metres_per_pixel: START_ZOOM,
        closest: CLOSEST,
        furthest: FURTHEST,
        follow: true,
        zoom_at_cursor: false,
        draggable: false,
        label_scale: 0.8,
    };
    commands
        .spawn((
            Minimap,
            Node {
                position_type: PositionType::Absolute,
                top: px(10),
                right: px(10),
                padding: UiRect::all(px(5)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            Themed(Piece::Panel),
            WINDOW_LAYER,
            Visibility::Hidden,
        ))
        .with_children(|frame| {
            frame.spawn((MinimapView, map_view(view, Node { width: px(SIZE), height: px(SIZE), ..default() })));
            frame
                .spawn((
                    MapButton,
                    Button,
                    Tooltip { title: "Map".to_string(), body: "Key: M".to_string() },
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(2),
                        bottom: px(2),
                        padding: UiRect::axes(px(5), px(1)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    BackgroundColor(WINDOW_BACKGROUND),
                    BorderColor::all(BORDER_COLOUR),
                    Themed(Piece::Button),
                ))
                .with_child((
                    Text::new("Map"),
                    TextFont { font_size: FontSize::Px(11.0), ..default() },
                    TextColor(Color::srgb(0.96, 0.9, 0.75)),
                ));
        });
}

fn show_while_playing(state: Res<State<ClientState>>, mut minimap: Single<&mut Visibility, With<Minimap>>) {
    let wanted = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
    minimap.set_if_neq(wanted);
}

/// The button opens (or closes) the full map.
fn open_map(buttons: Query<&Interaction, (With<MapButton>, Changed<Interaction>)>, mut open: ResMut<OpenMap>) {
    if buttons.iter().any(|interaction| *interaction == Interaction::Pressed) {
        open.0 = !open.0;
    }
}

/// A click on the map runs there (unless it's on the map button).
fn run_to_click(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    state: Res<State<ClientState>>,
    views: Query<(&MapView, &ComputedNode, &RelativeCursorPosition), With<MinimapView>>,
    buttons: Query<&Interaction, With<MapButton>>,
    mut orders: Orders,
) {
    if !mouse.just_pressed(MouseButton::Left) || *state.get() != ClientState::Playing {
        return;
    }
    if buttons.iter().any(|interaction| *interaction != Interaction::None) {
        return;
    }
    for (view, node, relative) in &views {
        if let Some(at) = point_under_mouse(view, node, relative) {
            orders.move_to(&mut commands, (at.x, at.y));
        }
    }
}
