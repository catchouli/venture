//! The toolbar in the top-right corner: a button (and a key) for each of the
//! player's own windows, the quest journal (**J**) and the bag (**I**).
//! Which windows are open is kept in [`OpenPanels`], which the windows follow.

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::dev::settings::DevSettings;
use crate::network::{ClientState, FromServer};
use crate::ui::text_input::KeyboardFocus;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND};

/// Colour of the buttons' labels.
const LABEL_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);

/// Shows the toolbar.
pub struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenPanels>()
            .add_systems(Startup, spawn_toolbar)
            .add_systems(Update, (toggle_panels, show_only_while_playing))
            .add_systems(OnExit(ClientState::Playing), close_panels);
    }
}

/// Which of the player's windows are open.
#[derive(Resource, Default)]
pub struct OpenPanels {
    /// The bag.
    pub bag: bool,
    /// The quest journal.
    pub journal: bool,
}

/// One of the player's windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Journal,
    Bag,
}

impl Panel {
    /// In the order they appear, left to right.
    const ALL: [Panel; 2] = [Panel::Journal, Panel::Bag];

    fn label(self) -> &'static str {
        match self {
            Panel::Journal => "Quests (J)",
            Panel::Bag => "Bag (I)",
        }
    }

    /// Its name in `VENTURE_OPEN` (see [`DevSettings`]).
    fn name(self) -> &'static str {
        match self {
            Panel::Journal => "journal",
            Panel::Bag => "bag",
        }
    }

    fn key(self) -> KeyCode {
        match self {
            Panel::Journal => KeyCode::KeyJ,
            Panel::Bag => KeyCode::KeyI,
        }
    }

    fn is_open(self, panels: &mut OpenPanels) -> &mut bool {
        match self {
            Panel::Journal => &mut panels.journal,
            Panel::Bag => &mut panels.bag,
        }
    }
}

/// The toolbar.
#[derive(Component)]
struct Toolbar;

/// A button opening and closing a window.
#[derive(Component)]
struct PanelButton(Panel);

fn spawn_toolbar(mut commands: Commands) {
    let font = TextFont { font_size: FontSize::Px(14.0), ..default() };
    commands
        .spawn((
            Toolbar,
            Node { position_type: PositionType::Absolute, top: px(12), right: px(12), column_gap: px(6), ..default() },
        ))
        .with_children(|toolbar| {
            for panel in Panel::ALL {
                toolbar
                    .spawn((
                        PanelButton(panel),
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
                    .with_child((Text::new(panel.label()), font.clone(), TextColor(LABEL_COLOUR)));
            }
        });
}

/// Each window's key or button opens and closes it (keys only while the
/// player isn't typing). A shop opening opens the bag too, to sell from.
fn toggle_panels(
    mut events: MessageReader<FromServer>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    buttons: Query<(&Interaction, &PanelButton), Changed<Interaction>>,
    dev: Res<DevSettings>,
    mut opened_for_dev: Local<bool>,
    mut panels: ResMut<OpenPanels>,
) {
    for panel in Panel::ALL {
        let pressed_key = focus.0.is_none() && keys.just_pressed(panel.key());
        let clicked =
            buttons.iter().any(|(interaction, button)| *interaction == Interaction::Pressed && button.0 == panel);
        if pressed_key || clicked {
            let open = panel.is_open(&mut panels);
            *open = !*open;
        }
    }
    if events.read().any(|FromServer(message)| matches!(message, ServerMessage::ShopOpened { .. })) {
        panels.bag = true;
    }
    if !*opened_for_dev {
        for panel in Panel::ALL.into_iter().filter(|panel| dev.open.iter().any(|name| name == panel.name())) {
            *panel.is_open(&mut panels) = true;
        }
        *opened_for_dev = true;
    }
}

fn show_only_while_playing(state: Res<State<ClientState>>, mut toolbar: Single<&mut Visibility, With<Toolbar>>) {
    **toolbar = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
}

fn close_panels(mut panels: ResMut<OpenPanels>) {
    *panels = OpenPanels::default();
}
