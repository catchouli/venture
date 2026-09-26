//! Small pieces of interface shared by several windows.

use std::f32::consts::FRAC_PI_4;

use bevy::prelude::*;

use crate::ui::theme::{FlatOnly, Piece, Themed};

/// Size of a close (✕) button, in pixels.
const CLOSE_SIZE: f32 = 16.0;
/// Space between a window's frame and what's in it, in pixels (room for the
/// theme's border).
pub const WINDOW_PADDING: f32 = 13.0;

/// Colour of window borders.
pub const BORDER_COLOUR: Color = Color::srgba(0.85, 0.7, 0.4, 0.8);
/// How windows are layered above the nameplates drawn over the world
/// (which would otherwise show through them). The connection overlay and
/// tooltips are higher still.
pub const WINDOW_LAYER: GlobalZIndex = GlobalZIndex(5);

/// Colour of window backgrounds.
pub const WINDOW_BACKGROUND: Color = Color::srgba(0.05, 0.04, 0.03, 0.85);

/// The colours of a kind of bar: its fill, and the darker empty part behind it.
#[derive(Clone, Copy, Debug)]
pub struct BarColours {
    /// The full part.
    pub fill: Color,
    /// The empty part.
    pub back: Color,
}

/// Health bars' colours.
pub const HEALTH_BAR: BarColours =
    BarColours { fill: Color::srgb(0.75, 0.15, 0.12), back: Color::srgb(0.24, 0.05, 0.04) };
/// Experience bars' colours.
pub const EXPERIENCE_BAR: BarColours =
    BarColours { fill: Color::srgb(0.85, 0.7, 0.3), back: Color::srgb(0.22, 0.16, 0.05) };

/// A bar (health, mana, experience) `height` pixels high, filled `fraction`
/// of the way (0 to 1). `back` and `fill` are bundles (such as markers) added
/// to the bar and its fill, to find them later. With the theme, the empty
/// part is `bar_back` tinted `colours.back`, the full part `bar_fill` tinted
/// `colours.fill`, and `bar_frame` is drawn over both.
pub fn bar(height: f32, colours: BarColours, fraction: f32, back: impl Bundle, fill: impl Bundle) -> impl Bundle {
    let corners = BorderRadius::all(px(3));
    (
        back,
        Node { height: px(height), width: percent(100), border_radius: corners, ..default() },
        BackgroundColor(colours.back),
        Themed(Piece::BarBack),
        children![
            (
                fill,
                Node {
                    height: percent(100),
                    width: percent(fraction.clamp(0.0, 1.0) * 100.0),
                    border_radius: corners,
                    ..default()
                },
                BackgroundColor(colours.fill),
                Themed(Piece::BarFill),
            ),
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    bottom: px(0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                Themed(Piece::BarFrame),
            ),
        ],
    )
}

/// The layout of a window's frame (a column, with room inside for the
/// theme's border), to fill in with its position and size.
pub fn window_frame() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        padding: UiRect::all(px(WINDOW_PADDING)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(4)),
        ..default()
    }
}

/// Adds a ✕ button in the top-right corner of `parent`, with `marker` on it
/// to tell it apart. It's drawn as two crossed bars, as the default font has
/// no ✕ character.
pub fn spawn_close_button(parent: &mut ChildSpawnerCommands, marker: impl Bundle) {
    parent
        .spawn((
            marker,
            Button,
            BackgroundColor(Color::NONE),
            Themed(Piece::Close),
            Node {
                position_type: PositionType::Absolute,
                top: px(4),
                right: px(4),
                width: px(CLOSE_SIZE),
                height: px(CLOSE_SIZE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            for angle in [FRAC_PI_4, -FRAC_PI_4] {
                button.spawn((
                    Node { position_type: PositionType::Absolute, width: px(CLOSE_SIZE), height: px(2), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                    UiTransform::from_rotation(Rot2::radians(angle)),
                    // The theme's close button has its own cross.
                    FlatOnly(Piece::Close),
                ));
            }
        });
}
