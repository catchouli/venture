//! The toolbar in the top-right corner: buttons for the character window's
//! most-used tabs (the window itself handles every tab's key; see
//! [`crate::ui::character_window`]).

use bevy::prelude::*;

use crate::network::ClientState;
use crate::ui::character_window::{OpenTab, Tab};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND};

/// Colour of the buttons' labels.
const LABEL_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);
/// The buttons: their label, and the tab each opens.
const BUTTONS: [(&str, Tab); 3] = [("Character (C)", Tab::Stats), ("Quests (J)", Tab::Quests), ("Bag (I)", Tab::Bag)];

/// Shows the toolbar.
pub struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_toolbar).add_systems(Update, (open_tabs, show_only_while_playing));
    }
}

/// The toolbar.
#[derive(Component)]
struct Toolbar;

/// A button opening a tab of the character window.
#[derive(Component)]
struct TabButton(Tab);

fn spawn_toolbar(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    commands
        .spawn((
            Toolbar,
            Node { position_type: PositionType::Absolute, top: px(12), right: px(12), column_gap: px(6), ..default() },
        ))
        .with_children(|toolbar| {
            for (label, tab) in BUTTONS {
                toolbar
                    .spawn((
                        TabButton(tab),
                        Button,
                        Node {
                            padding: UiRect::axes(px(10), px(4)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(WINDOW_BACKGROUND),
                        BorderColor::all(BORDER_COLOUR),
                    ))
                    .with_child((Text::new(label), font.clone(), TextColor(LABEL_COLOUR)));
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

fn show_only_while_playing(state: Res<State<ClientState>>, mut toolbar: Single<&mut Visibility, With<Toolbar>>) {
    **toolbar = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
}
