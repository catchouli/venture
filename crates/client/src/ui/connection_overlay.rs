//! A full-screen overlay while not in the game: "Connecting…", or, when the
//! connection is lost, the game fades out under a red message while the
//! client keeps trying to reconnect.

use bevy::prelude::*;

use crate::network::{ClientState, Session};
use crate::world::source::WorldLoadError;

/// How quickly the overlay fades in and out (per second).
const FADE_RATE: f32 = 3.0;

/// Shows the connection overlay.
pub struct ConnectionOverlayPlugin;

impl Plugin for ConnectionOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_overlay).add_systems(Update, update_overlay);
    }
}

#[derive(Component)]
struct Overlay;

#[derive(Component)]
struct Headline;

#[derive(Component)]
struct Detail;

fn spawn_overlay(mut commands: Commands) {
    commands
        .spawn((
            Overlay,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            // Drawn above the game's other interface.
            GlobalZIndex(10),
        ))
        .with_children(|overlay| {
            overlay.spawn((Headline, Text::default(), TextFont { font_size: FontSize::Px(28.0), ..default() }));
            overlay.spawn((
                Detail,
                Text::default(),
                TextFont { font_size: FontSize::Px(16.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
            ));
        });
}

/// Fades the overlay to suit the connection state, and sets its message.
fn update_overlay(
    time: Res<Time>,
    state: Res<State<ClientState>>,
    session: Res<Session>,
    world_error: Option<Res<WorldLoadError>>,
    overlay: Single<(&mut BackgroundColor, &mut Visibility), With<Overlay>>,
    mut headline: Single<(&mut Text, &mut TextColor), (With<Headline>, Without<Detail>)>,
    mut detail: Single<&mut Text, (With<Detail>, Without<Headline>)>,
) {
    let (target, title, colour) = match (state.get(), &world_error) {
        (_, Some(_)) => (0.85, "Can't load the world", Color::srgb(1.0, 0.35, 0.3)),
        (ClientState::Connecting, _) => (0.8, "Connecting…", Color::WHITE),
        (ClientState::ChoosingName, _) => (0.5, "", Color::WHITE),
        (ClientState::Playing, _) => (0.0, "", Color::WHITE),
        (ClientState::Disconnected, _) => {
            (0.75, "Disconnected from the server. Reconnecting…", Color::srgb(1.0, 0.35, 0.3))
        }
    };
    let reason = match (&world_error, state.get()) {
        (Some(error), _) => error.0.clone(),
        (None, ClientState::Disconnected | ClientState::Connecting) => {
            session.disconnect_reason.clone().unwrap_or_default()
        }
        _ => String::new(),
    };

    let (mut background, mut visibility) = overlay.into_inner();
    let alpha = background.0.alpha();
    let step = FADE_RATE * time.delta_secs();
    let alpha = if alpha < target { (alpha + step).min(target) } else { (alpha - step).max(target) };
    background.0.set_alpha(alpha);
    *visibility = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };

    if headline.0.0 != title {
        headline.0.0 = title.to_string();
    }
    headline.1.0 = colour;
    if detail.0 != reason {
        detail.0 = reason;
    }
}
