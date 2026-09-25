//! The target frame, at the top middle of the screen: the selected target's
//! name, a health bar for creatures, and an ✕ to deselect.

use bevy::prelude::*;

use crate::characters::nameplates::Nameplate;
use crate::interaction::targeting::{Health, Target, TargetKind, Targetable};
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, spawn_close_button};

/// Width of the frame, in pixels.
const WIDTH: f32 = 240.0;

/// Shows the target frame.
pub struct TargetFramePlugin;

impl Plugin for TargetFramePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_frame).add_systems(Update, (update_frame, close_on_click));
    }
}

/// The frame.
#[derive(Component)]
struct TargetFrame;

/// The target's name in the frame.
#[derive(Component)]
struct TargetName;

/// The health bar's row (shown only for creatures).
#[derive(Component)]
struct HealthBar;

/// The filled part of the health bar.
#[derive(Component)]
struct HealthFill;

/// The ✕ button.
#[derive(Component)]
struct CloseButton;

fn spawn_frame(mut commands: Commands) {
    // A full-width row at the top, centring the frame.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(12),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                TargetFrame,
                // Blocks clicks on the world behind it.
                Interaction::default(),
                Node {
                    width: px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(6),
                    padding: UiRect::axes(px(28), px(8)),
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
                    TargetName,
                    Text::default(),
                    TextFont { font_size: FontSize::Px(16.0), ..default() },
                    TextColor(Color::srgb(0.96, 0.9, 0.75)),
                ));
                frame
                    .spawn((
                        HealthBar,
                        Node { width: percent(100), height: px(8), ..default() },
                        BackgroundColor(Color::srgb(0.2, 0.05, 0.05)),
                    ))
                    .with_child((
                        HealthFill,
                        Node { width: percent(100), height: percent(100), ..default() },
                        BackgroundColor(Color::srgb(0.75, 0.12, 0.1)),
                    ));
                spawn_close_button(frame, CloseButton);
            });
        });
}

/// Shows or hides the frame, and fills it in for the current target.
fn update_frame(
    target: Res<Target>,
    targets: Query<(&Targetable, &Nameplate, Option<&Health>)>,
    mut frame: Single<&mut Visibility, (With<TargetFrame>, Without<HealthBar>)>,
    mut name: Single<&mut Text, With<TargetName>>,
    bar: Single<(&mut Node, &mut Visibility), (With<HealthBar>, Without<HealthFill>, Without<TargetFrame>)>,
    mut fill: Single<&mut Node, (With<HealthFill>, Without<HealthBar>)>,
) {
    let Some((&Targetable { kind, .. }, nameplate, health)) = target.0.and_then(|entity| targets.get(entity).ok())
    else {
        **frame = Visibility::Hidden;
        return;
    };
    **frame = Visibility::Inherited;
    if name.0 != nameplate.0 {
        name.0.clone_from(&nameplate.0);
    }
    // Only creatures show health: NPCs can't be attacked, and other players' health is private.
    let shown_health = health.filter(|_| kind == TargetKind::Enemy);
    let (mut bar_node, mut bar_visibility) = bar.into_inner();
    match shown_health {
        Some(health) => {
            bar_node.display = Display::Flex;
            *bar_visibility = Visibility::Inherited;
            fill.width = percent((health.current / health.max.max(1.0)).clamp(0.0, 1.0) * 100.0);
        }
        None => {
            bar_node.display = Display::None;
            *bar_visibility = Visibility::Hidden;
        }
    }
}

/// The ✕ deselects the target.
fn close_on_click(button: Single<&Interaction, (With<CloseButton>, Changed<Interaction>)>, mut target: ResMut<Target>) {
    if **button == Interaction::Pressed {
        target.0 = None;
    }
}
