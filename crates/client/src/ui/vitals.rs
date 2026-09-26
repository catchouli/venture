//! The player frame, in the top-left corner: the player's name, level and
//! health. The server sends health whenever it changes
//! ([`ServerMessage::Vitals`]); the level comes from the character sheet.

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::characters::nameplates::Nameplate;
use crate::characters::player::LocalPlayer;
use crate::network::{ClientState, FromServer};
use crate::ui::character_window::Sheet;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND};

/// Width of the frame, in pixels.
const WIDTH: f32 = 220.0;
/// Colour of the health bar.
const HEALTH_COLOUR: Color = Color::srgb(0.75, 0.15, 0.12);
/// Colour of the name.
const NAME_COLOUR: Color = Color::srgb(0.96, 0.9, 0.75);

/// Shows the player frame.
pub struct VitalsPlugin;

impl Plugin for VitalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Vitals>()
            .add_systems(Startup, spawn_frame)
            .add_systems(Update, (record_vitals, show_frame).chain())
            .add_systems(OnExit(ClientState::Playing), forget_vitals);
    }
}

/// The player's health, as the server last said: (health, most health).
#[derive(Resource, Default)]
pub struct Vitals(pub Option<(u32, u32)>);

/// The frame.
#[derive(Component)]
struct PlayerFrame;

/// The name and level.
#[derive(Component)]
struct NameText;

/// The health bar's fill.
#[derive(Component)]
struct HealthFill;

/// The health numbers.
#[derive(Component)]
struct HealthText;

fn spawn_frame(mut commands: Commands) {
    commands
        .spawn((
            PlayerFrame,
            Node {
                position_type: PositionType::Absolute,
                left: px(10),
                top: px(10),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                padding: UiRect::axes(px(10), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(WINDOW_BACKGROUND),
            BorderColor::all(BORDER_COLOUR),
            Visibility::Hidden,
        ))
        .with_children(|frame| {
            frame.spawn((
                NameText,
                Text::default(),
                TextFont { font_size: FontSize::Px(14.0), ..default() },
                TextColor(NAME_COLOUR),
            ));
            frame
                .spawn((
                    Node { width: percent(100), height: px(14), border_radius: BorderRadius::all(px(3)), ..default() },
                    BackgroundColor(Color::srgb(0.2, 0.05, 0.05)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        HealthFill,
                        Node {
                            width: percent(100),
                            height: percent(100),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(HEALTH_COLOUR),
                    ));
                    // The numbers, centred over the bar.
                    bar.spawn(Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_child((
                        HealthText,
                        Text::default(),
                        TextFont { font_size: FontSize::Px(11.0), ..default() },
                        TextColor(Color::WHITE),
                        TextShadow { offset: Vec2::splat(1.0), ..default() },
                    ));
                });
        });
}

fn record_vitals(mut events: MessageReader<FromServer>, mut vitals: ResMut<Vitals>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Vitals { health, max_health } = message {
            vitals.0 = Some((*health, *max_health));
        }
    }
}

/// Fills the frame in (shown only while playing).
fn show_frame(
    state: Res<State<ClientState>>,
    vitals: Res<Vitals>,
    sheet: Res<Sheet>,
    player: Option<Single<&Nameplate, With<LocalPlayer>>>,
    mut frame: Single<&mut Visibility, With<PlayerFrame>>,
    mut name: Single<&mut Text, (With<NameText>, Without<HealthText>)>,
    mut fill: Single<&mut Node, With<HealthFill>>,
    mut numbers: Single<&mut Text, (With<HealthText>, Without<NameText>)>,
) {
    let playing = *state.get() == ClientState::Playing;
    **frame = if playing { Visibility::Inherited } else { Visibility::Hidden };
    let level = sheet.0.as_ref().map(|sheet| format!("  Level {}", sheet.level)).unwrap_or_default();
    let label = format!("{}{level}", player.map(|nameplate| nameplate.0.clone()).unwrap_or_default());
    if name.0 != label {
        name.0 = label;
    }
    let Some((health, max)) = vitals.0 else { return };
    fill.width = percent(health as f32 / max.max(1) as f32 * 100.0);
    let text = format!("{health} / {max}");
    if numbers.0 != text {
        numbers.0 = text;
    }
}

fn forget_vitals(mut vitals: ResMut<Vitals>) {
    vitals.0 = None;
}
