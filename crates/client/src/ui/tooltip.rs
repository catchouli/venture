//! Tooltips: a small box by the mouse describing what it's pointing at. Give
//! any interface element with an [`Interaction`] a [`Tooltip`], and the box
//! appears while the mouse is over it.

use bevy::prelude::*;

use crate::ui::theme::{Piece, Themed};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND};

/// How far from the mouse the box sits, in pixels.
const OFFSET: Vec2 = Vec2::new(16.0, 16.0);
/// The widest the box's text grows before wrapping, in pixels.
const TEXT_WIDTH: f32 = 240.0;
/// Colour of the title.
const TITLE_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// Colour of the text below it.
const BODY_COLOUR: Color = Color::srgb(0.85, 0.85, 0.82);

/// Shows tooltips.
pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_box).add_systems(Update, show_tooltip);
    }
}

/// What to say about an interface element when the mouse is over it.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Tooltip {
    /// The first line, highlighted. No tooltip is shown if it's empty.
    pub title: String,
    /// More about it, below the title (may be empty).
    pub body: String,
}

/// The box.
#[derive(Component)]
struct TooltipBox;

/// The box's title.
#[derive(Component)]
struct TooltipTitle;

/// The box's text.
#[derive(Component)]
struct TooltipBody;

fn spawn_box(mut commands: Commands) {
    commands
        .spawn((
            TooltipBox,
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                padding: UiRect::axes(px(9), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            Themed(Piece::Tooltip),
            // Drawn above every other window.
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|tooltip| {
            tooltip.spawn((
                TooltipTitle,
                Text::default(),
                TextFont { font_size: FontSize::Px(14.0), ..default() },
                TextColor(TITLE_COLOUR),
                // (The text itself is given the width limit, so its height is
                // worked out for the lines it wraps onto; a limit only on the
                // box leaves the text spilling out of the bottom.)
                Node { max_width: px(TEXT_WIDTH), ..default() },
            ));
            tooltip.spawn((
                TooltipBody,
                Text::default(),
                TextFont { font_size: FontSize::Px(13.0), ..default() },
                TextColor(BODY_COLOUR),
                Node { max_width: px(TEXT_WIDTH), ..default() },
            ));
        });
}

/// Shows the tooltip of whatever the mouse is over, next to the mouse.
fn show_tooltip(
    window: Single<&Window>,
    hovered: Query<(&Interaction, &Tooltip, &InheritedVisibility)>,
    tooltip_box: Single<(&mut Node, &mut Visibility, &ComputedNode), With<TooltipBox>>,
    mut title: Single<&mut Text, (With<TooltipTitle>, Without<TooltipBody>)>,
    mut body: Single<(&mut Text, &mut Node), (With<TooltipBody>, Without<TooltipTitle>, Without<TooltipBox>)>,
) {
    let (mut node, mut visibility, computed) = tooltip_box.into_inner();
    let shown = hovered.iter().find(|(interaction, tooltip, visible)| {
        **interaction != Interaction::None && visible.get() && !tooltip.title.is_empty()
    });
    let (Some((_, tooltip, _)), Some(cursor)) = (shown, window.cursor_position()) else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Inherited;
    if title.0 != tooltip.title {
        title.0.clone_from(&tooltip.title);
    }
    let (body_text, body_node) = &mut *body;
    if body_text.0 != tooltip.body {
        body_text.0.clone_from(&tooltip.body);
    }
    body_node.display = if tooltip.body.is_empty() { Display::None } else { Display::Flex };
    // Keep it on screen: flip to the mouse's other side near the right or
    // bottom edge. (It's always placed by its top left corner, using its size
    // as last laid out: placing it by its right edge makes the text wrap at a
    // different width than its box was sized for, so the text spills out.)
    let size = computed.size() * computed.inverse_scale_factor();
    let position = cursor + OFFSET;
    let left = if position.x + size.x > window.width() { cursor.x - OFFSET.x - size.x } else { position.x };
    let top = if position.y + size.y > window.height() { cursor.y - OFFSET.y - size.y } else { position.y };
    node.left = px(left.max(0.0));
    node.top = px(top.max(0.0));
    node.right = Val::Auto;
    node.bottom = Val::Auto;
}
