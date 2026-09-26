//! The player frame, in the top-left corner: the player's name, level,
//! health and mana. The server sends health and mana whenever they change
//! ([`ServerMessage::Vitals`]); the level comes from the character sheet.

use bevy::prelude::*;
use venture_protocol::ServerMessage;

use crate::characters::nameplates::Nameplate;
use crate::characters::player::LocalPlayer;
use crate::network::{ClientState, FromServer};
use crate::ui::character_window::Sheet;
use crate::ui::theme::{Piece, Themed};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND};

/// Width of the frame, in pixels.
const WIDTH: f32 = 220.0;
/// Colour of the health bar, and of its empty part.
const HEALTH_COLOURS: (Color, Color) = (Color::srgb(0.75, 0.15, 0.12), Color::srgb(0.2, 0.05, 0.05));
/// Colour of the mana bar, and of its empty part.
const MANA_COLOURS: (Color, Color) = (Color::srgb(0.18, 0.35, 0.8), Color::srgb(0.05, 0.08, 0.2));
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

/// The player's health and mana, as the server last said.
#[derive(Resource, Default)]
pub struct Vitals(pub Option<PlayerVitals>);

/// Health and mana, now and at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerVitals {
    /// Health left.
    pub health: u32,
    /// Most health.
    pub max_health: u32,
    /// Mana left.
    pub mana: u32,
    /// Most mana.
    pub max_mana: u32,
}

/// The frame.
#[derive(Component)]
struct PlayerFrame;

/// The name and level.
#[derive(Component)]
struct NameText;

/// Which bar something belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bar {
    Health,
    Mana,
}

/// A bar's fill.
#[derive(Component)]
struct BarFill(Bar);

/// A bar's numbers.
#[derive(Component)]
struct BarText(Bar);

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
            Themed(Piece::Panel),
            Visibility::Hidden,
        ))
        .with_children(|frame| {
            frame.spawn((
                NameText,
                Text::default(),
                TextFont { font_size: FontSize::Px(14.0), ..default() },
                TextColor(NAME_COLOUR),
            ));
            spawn_bar(frame, Bar::Health, HEALTH_COLOURS, 14.0);
            spawn_bar(frame, Bar::Mana, MANA_COLOURS, 12.0);
        });
}

/// A bar with its numbers centred over it.
fn spawn_bar(frame: &mut ChildSpawnerCommands, bar: Bar, (fill, empty): (Color, Color), height: f32) {
    frame
        .spawn((
            Node { width: percent(100), height: px(height), border_radius: BorderRadius::all(px(3)), ..default() },
            BackgroundColor(empty),
            Themed(Piece::BarBack),
        ))
        .with_children(|parent| {
            parent.spawn((
                BarFill(bar),
                Node {
                    width: percent(100),
                    height: percent(100),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(fill),
                Themed(Piece::BarFill),
            ));
            parent
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_child((
                    BarText(bar),
                    Text::default(),
                    TextFont { font_size: FontSize::Px(height - 3.0), ..default() },
                    TextColor(Color::WHITE),
                    TextShadow { offset: Vec2::splat(1.0), ..default() },
                ));
        });
}

fn record_vitals(mut events: MessageReader<FromServer>, mut vitals: ResMut<Vitals>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Vitals { health, max_health, mana, max_mana } = message {
            vitals.0 =
                Some(PlayerVitals { health: *health, max_health: *max_health, mana: *mana, max_mana: *max_mana });
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
    mut name: Single<&mut Text, (With<NameText>, Without<BarText>)>,
    mut fills: Query<(&BarFill, &mut Node)>,
    mut numbers: Query<(&BarText, &mut Text), Without<NameText>>,
) {
    let playing = *state.get() == ClientState::Playing;
    **frame = if playing { Visibility::Inherited } else { Visibility::Hidden };
    let level = sheet.0.as_ref().map(|sheet| format!("  Level {}", sheet.level)).unwrap_or_default();
    let label = format!("{}{level}", player.map(|nameplate| nameplate.0.clone()).unwrap_or_default());
    if name.0 != label {
        name.0 = label;
    }
    let Some(vitals) = vitals.0 else { return };
    let amounts = |bar| match bar {
        Bar::Health => (vitals.health, vitals.max_health),
        Bar::Mana => (vitals.mana, vitals.max_mana),
    };
    for (BarFill(bar), mut node) in &mut fills {
        let (now, most) = amounts(*bar);
        node.width = percent(now as f32 / most.max(1) as f32 * 100.0);
    }
    for (BarText(bar), mut text) in &mut numbers {
        let (now, most) = amounts(*bar);
        let wanted = format!("{now} / {most}");
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

fn forget_vitals(mut vitals: ResMut<Vitals>) {
    vitals.0 = None;
}
