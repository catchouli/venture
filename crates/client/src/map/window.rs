//! The full map (**M**, or the minimap's button): a big window in the middle
//! of the screen. It opens on the player; the mouse wheel zooms towards the
//! mouse, all the way out to the whole world, and dragging moves the map
//! (it then stays put, rather than following the player). The bottom right
//! says where the mouse is, in the numbers `/tp` takes.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::dev::settings::DevSettings;
use crate::map::view::{MapView, map_view, point_under_mouse};
use crate::network::ClientState;
use crate::ui::text_input::KeyboardFocus;
use crate::ui::theme::{Piece, Themed};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER, spawn_close_button, window_frame};

/// Size of the map in the window, in pixels.
const MAP_SIZE: Vec2 = Vec2::new(720.0, 520.0);
/// How far in it opens (metres a pixel), and the closest it zooms.
const START_ZOOM: f32 = 3.0;
const CLOSEST: f32 = 0.3;

/// Shows the map window.
pub struct MapWindowPlugin;

impl Plugin for MapWindowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenMap>()
            .add_systems(Startup, spawn_window)
            .add_systems(Update, ((toggle, show_window).chain(), show_coordinates))
            .add_systems(OnExit(ClientState::Playing), |mut open: ResMut<OpenMap>| open.0 = false);
    }
}

/// Whether the map window is open.
#[derive(Resource, Default)]
pub struct OpenMap(pub bool);

/// The window.
#[derive(Component)]
struct MapWindow;

/// The ✕ button.
#[derive(Component)]
struct CloseButton;

/// The window's map.
#[derive(Component)]
struct WindowMap;

/// Where the mouse is on the map.
#[derive(Component)]
struct Coordinates;

fn spawn_window(mut commands: Commands) {
    let view = MapView {
        centre: Vec2::ZERO,
        metres_per_pixel: START_ZOOM,
        closest: CLOSEST,
        // (As far as the whole world: see `MapView`'s zooming.)
        furthest: f32::INFINITY,
        follow: true,
        zoom_at_cursor: true,
        draggable: true,
        label_scale: 1.0,
    };
    // A row and column filling the screen, centring the window.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            WINDOW_LAYER,
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    MapWindow,
                    // Blocks clicks on the world behind it.
                    Interaction::default(),
                    window_frame(),
                    BackgroundColor(WINDOW_BACKGROUND),
                    BorderColor::all(BORDER_COLOUR),
                    Themed(Piece::Window),
                    Visibility::Hidden,
                ))
                .with_children(|window| {
                    window
                        .spawn((
                            Node {
                                justify_content: JustifyContent::Center,
                                padding: UiRect::axes(px(8), px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            Themed(Piece::Header),
                        ))
                        .with_children(|header| {
                            header.spawn((
                                Text::new("Map"),
                                TextFont { font_size: FontSize::Px(15.0), ..default() },
                                TextColor(Color::srgb(0.96, 0.9, 0.75)),
                                TextShadow { offset: Vec2::splat(1.0), ..default() },
                            ));
                            spawn_close_button(header, CloseButton);
                        });
                    window.spawn((
                        WindowMap,
                        map_view(view, Node { width: px(MAP_SIZE.x), height: px(MAP_SIZE.y), ..default() }),
                    ));
                    let small = |text: &str| {
                        (
                            Text::new(text),
                            TextFont { font_size: FontSize::Px(11.0), ..default() },
                            TextColor(Color::srgb(0.6, 0.6, 0.56)),
                        )
                    };
                    window
                        .spawn(Node { width: percent(100), justify_content: JustifyContent::SpaceBetween, ..default() })
                        .with_children(|footer| {
                            footer.spawn(small("Scroll to zoom, drag to move the map."));
                            footer.spawn((Coordinates, small("")));
                        });
                });
        });
}

/// M (unless typing) opens and closes the map; the ✕ closes it. (For
/// development, `VENTURE_OPEN=map` opens it once in the game.)
fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    close: Query<&Interaction, (With<CloseButton>, Changed<Interaction>)>,
    dev: Res<DevSettings>,
    state: Res<State<ClientState>>,
    mut opened_for_dev: Local<bool>,
    mut open: ResMut<OpenMap>,
    mut map: Single<&mut MapView, With<WindowMap>>,
) {
    if !*opened_for_dev && *state.get() == ClientState::Playing {
        *opened_for_dev = true;
        open.0 |= dev.open.iter().any(|name| name == "map");
        if let Some(zoom) = dev.map_zoom {
            map.metres_per_pixel = zoom;
        }
    }
    if keys.just_pressed(KeyCode::KeyM) && !focus.is_typing() {
        open.0 = !open.0;
    }
    if close.iter().any(|interaction| *interaction == Interaction::Pressed) {
        open.0 = false;
    }
}

/// Shows the window while it's open (and playing); opening it centres the
/// map on the player again.
fn show_window(
    open: Res<OpenMap>,
    state: Res<State<ClientState>>,
    mut window: Single<&mut Visibility, With<MapWindow>>,
    mut map: Single<&mut MapView, With<WindowMap>>,
) {
    let showing = open.0 && *state.get() == ClientState::Playing;
    window.set_if_neq(if showing { Visibility::Inherited } else { Visibility::Hidden });
    if open.is_changed() && open.0 {
        map.follow = true;
    }
}

/// Shows the world point under the mouse (as `x, y` in metres, which `/tp`
/// takes), or nothing when the mouse is off the map.
fn show_coordinates(
    map: Single<(&MapView, &ComputedNode, &RelativeCursorPosition), With<WindowMap>>,
    mut text: Single<&mut Text, With<Coordinates>>,
) {
    let (view, node, relative) = *map;
    let said = point_under_mouse(view, node, relative).map_or(String::new(), |at| format!("{:.0}, {:.0}", at.x, at.y));
    if text.0 != said {
        text.0 = said;
    }
}
