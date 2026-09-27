//! The toolbar along the bottom edge, left of the hotbar: an icon button for
//! each tab of the character window, which opens just above it. (The icons
//! come from the theme; the window itself handles every tab's key: see
//! [`crate::ui::character_window`].)

use bevy::prelude::*;

use crate::network::ClientState;
use crate::ui::character_window::{OpenTab, RIGHT, Tab};
use crate::ui::theme::{Piece, Theme, Themed};
use crate::ui::tooltip::Tooltip;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER};

/// Size of a button, in pixels.
const BUTTON_SIZE: f32 = 38.0;
/// Size of an icon on its button, in pixels.
const ICON_SIZE: f32 = 26.0;
/// Colour of a button's short name, when the theme has no icon for it.
const LABEL_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);

/// Shows the toolbar.
pub struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_toolbar)
            .add_systems(Update, (open_tabs, show_open_tab, show_icons, show_only_while_playing));
    }
}

/// The toolbar.
#[derive(Component)]
struct Toolbar;

/// A button opening a tab of the character window.
#[derive(Component)]
struct TabButton(Tab);

/// A button's icon.
#[derive(Component)]
struct Icon(Tab);

/// A button's short name, shown when the theme has no icon for it.
#[derive(Component)]
struct Label(Tab);

fn spawn_toolbar(mut commands: Commands) {
    commands
        .spawn((
            Toolbar,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(10),
                right: px(RIGHT),
                column_gap: px(4),
                ..default()
            },
            WINDOW_LAYER,
        ))
        .with_children(|toolbar| {
            for tab in Tab::ALL {
                toolbar
                    .spawn((
                        TabButton(tab),
                        Button,
                        Tooltip { title: tab.label().to_string(), body: format!("Key: {}", tab.key_name()) },
                        Node {
                            width: px(BUTTON_SIZE),
                            height: px(BUTTON_SIZE),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(WINDOW_BACKGROUND),
                        BorderColor::all(BORDER_COLOUR),
                        Themed(Piece::ToolbarButton),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Icon(tab),
                            ImageNode::default(),
                            Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                            Visibility::Hidden,
                        ));
                        button.spawn((
                            Label(tab),
                            Text::new(&tab.label()[..3]),
                            TextFont { font_size: FontSize::Px(11.0), ..default() },
                            TextColor(LABEL_COLOUR),
                            Node { position_type: PositionType::Absolute, ..default() },
                        ));
                    });
            }
        });
}

/// Each button opens its tab (or closes the window, if that tab is showing).
fn open_tabs(buttons: Query<(&Interaction, &TabButton), Changed<Interaction>>, mut open: ResMut<OpenTab>) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            open.toggle(button.0);
        }
    }
}

/// Lights up the button for the tab that's open.
fn show_open_tab(open: Res<OpenTab>, mut buttons: Query<(&TabButton, &mut Themed)>) {
    if !open.is_changed() {
        return;
    }
    for (button, mut themed) in &mut buttons {
        themed.0 = if open.0 == Some(button.0) { Piece::ToolbarButtonOpen } else { Piece::ToolbarButton };
    }
}

/// Gives the buttons their icons from the theme (or their short names, if it has none).
fn show_icons(
    theme: Res<Theme>,
    mut icons: Query<(&Icon, &mut ImageNode, &mut Visibility)>,
    mut labels: Query<(&Label, &mut Visibility), Without<Icon>>,
) {
    if !theme.is_changed() {
        return;
    }
    for (Icon(tab), mut image, mut visibility) in &mut icons {
        let icon = theme.tab_icon(tab.id());
        *visibility = if icon.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        image.image = icon.unwrap_or_default();
    }
    for (Label(tab), mut visibility) in &mut labels {
        *visibility = if theme.tab_icon(tab.id()).is_some() { Visibility::Hidden } else { Visibility::Inherited };
    }
}

fn show_only_while_playing(state: Res<State<ClientState>>, mut toolbar: Single<&mut Visibility, With<Toolbar>>) {
    let wanted = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
    if **toolbar != wanted {
        **toolbar = wanted;
    }
}
