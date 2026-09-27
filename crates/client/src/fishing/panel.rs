//! The fishing panel, in the lower middle of the screen while a line is in:
//! what's happening and what to do, and in a fight three bars: the line's
//! tension (against its strength: too much and it snaps), how much line is
//! out (against the reel's capacity: all of it and the fish is gone), and
//! how much fight the fish has left. A ✕ reels in.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, FishingCommand};
use venture_rpg::fishing::Method;
use venture_simulation::fishing::Fight;

use crate::fishing::{Angling, Stage};
use crate::network::ToServer;
use crate::ui::theme::{Piece, Themed};
use crate::ui::widgets::{BORDER_COLOUR, BarColours, WINDOW_BACKGROUND, bar, spawn_close_button};

/// Width of the panel, in pixels.
const WIDTH: f32 = 300.0;
/// How far above the bottom of the screen it sits (clear of the hotbar), in pixels.
const BOTTOM: f32 = 150.0;
/// How long "nibble" shows after one, in seconds.
const NIBBLE_SHOWN: f32 = 0.8;
/// The tension bar's colours, easy and strained.
const TENSION: BarColours = BarColours { fill: Color::srgb(0.35, 0.7, 0.3), back: Color::srgb(0.08, 0.16, 0.07) };
const STRAINED: Color = Color::srgb(0.9, 0.2, 0.12);
/// Share of the line's strength from which the tension shows as strained.
const STRAIN: f32 = 0.8;
/// The line-out bar's colours.
const LINE_OUT: BarColours = BarColours { fill: Color::srgb(0.3, 0.55, 0.85), back: Color::srgb(0.06, 0.1, 0.18) };
/// The fish's strength bar's colours.
const FISH: BarColours = BarColours { fill: Color::srgb(0.85, 0.7, 0.3), back: Color::srgb(0.22, 0.16, 0.05) };

/// Shows the fishing panel.
pub struct PanelPlugin;

impl Plugin for PanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel).add_systems(Update, (update_panel, reel_in_on_click));
    }
}

#[derive(Component)]
struct Panel;

/// What's happening.
#[derive(Component)]
struct Status;

/// What to do.
#[derive(Component)]
struct Hint;

/// The bars' rows (shown in fights).
#[derive(Component)]
struct FightBars;

/// Each bar's fill.
#[derive(Component)]
enum Fill {
    Tension,
    LineOut,
    Strength,
}

#[derive(Component)]
struct ReelIn;

fn spawn_panel(mut commands: Commands) {
    let label = |text: &str| {
        (
            Text::new(text),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::srgb(0.75, 0.73, 0.66)),
        )
    };
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(BOTTOM),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Panel,
                Interaction::default(),
                Node {
                    width: px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    row_gap: px(4),
                    padding: UiRect::axes(px(20), px(10)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(WINDOW_BACKGROUND),
                BorderColor::all(BORDER_COLOUR),
                Themed(Piece::Panel),
                Visibility::Hidden,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Status,
                    Text::default(),
                    TextFont { font_size: FontSize::Px(16.0), ..default() },
                    TextColor(Color::srgb(0.96, 0.9, 0.75)),
                    TextLayout::justify(Justify::Center),
                ));
                panel.spawn((
                    Hint,
                    Text::default(),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    TextColor(Color::srgb(0.8, 0.78, 0.7)),
                    TextLayout::justify(Justify::Center),
                ));
                panel
                    .spawn((FightBars, Node { flex_direction: FlexDirection::Column, row_gap: px(3), ..default() }))
                    .with_children(|bars| {
                        bars.spawn(label("Tension"));
                        bars.spawn(bar(10.0, TENSION, 0.0, (), Fill::Tension));
                        bars.spawn(label("Line out"));
                        bars.spawn(bar(10.0, LINE_OUT, 0.0, (), Fill::LineOut));
                        bars.spawn(label("Fish's strength"));
                        bars.spawn(bar(10.0, FISH, 1.0, (), Fill::Strength));
                    });
                spawn_close_button(panel, ReelIn);
            });
        });
}

/// What the panel says for a line: the status and the hint.
fn words(stage: Stage, method: Method, nibbling: bool) -> (&'static str, &'static str) {
    match stage {
        Stage::Waiting if nibbling => ("Something's nibbling...", "Not yet: wait for the bite."),
        Stage::Waiting => match method {
            Method::Ledger => ("Waiting for a bite", "Watch the rod tip. Strike when a fish bites."),
            Method::Float => ("Waiting for a bite", "Watch the float. Strike when it goes under."),
            _ => ("Drawing the lure through the water", "A fish that takes it will hook itself."),
        },
        Stage::Biting => ("A bite!", "Click (or press Space) to strike, now!"),
        Stage::Fighting(fight) if fight.running => ("It's running!", "Let it run: reeling now could snap the line."),
        Stage::Fighting(_) => ("Hooked!", "Hold the mouse button (or Space) to reel it in."),
    }
}

fn update_panel(
    time: Res<Time>,
    angling: Res<Angling>,
    mut panel: Single<&mut Visibility, (With<Panel>, Without<FightBars>)>,
    mut status: Single<&mut Text, (With<Status>, Without<Hint>)>,
    mut hint: Single<&mut Text, (With<Hint>, Without<Status>)>,
    mut bars: Single<&mut Node, (With<FightBars>, Without<Fill>)>,
    mut fills: Query<(&Fill, &mut Node, &mut BackgroundColor), Without<FightBars>>,
) {
    let Some(line) = &angling.line else {
        **panel = Visibility::Hidden;
        return;
    };
    **panel = Visibility::Inherited;
    let nibbling = line.nibbled.is_some_and(|at| time.elapsed_secs() - at < NIBBLE_SHOWN);
    let (said, what_to_do) = words(line.stage, line.method, nibbling);
    if status.0 != said {
        status.0 = said.to_string();
    }
    if hint.0 != what_to_do {
        hint.0 = what_to_do.to_string();
    }
    let Stage::Fighting(fight) = line.stage else {
        bars.display = Display::None;
        return;
    };
    bars.display = Display::Flex;
    for (fill, mut node, mut colour) in &mut fills {
        let fraction = shares(&fight, fill);
        node.width = percent(fraction.clamp(0.0, 1.0) * 100.0);
        if let Fill::Tension = fill {
            colour.0 = if fraction >= STRAIN { STRAINED } else { TENSION.fill };
        }
    }
}

/// How full a bar is, from 0 to 1.
fn shares(fight: &Fight, fill: &Fill) -> f32 {
    match fill {
        Fill::Tension => fight.tension / fight.setup.strength.max(0.01),
        Fill::LineOut => fight.distance / fight.setup.capacity.max(0.01),
        Fill::Strength => fight.strength,
    }
}

/// The ✕ reels in and stops fishing.
fn reel_in_on_click(
    button: Option<Single<&Interaction, (With<ReelIn>, Changed<Interaction>)>>,
    mut to_server: MessageWriter<ToServer>,
) {
    if button.is_some_and(|button| **button == Interaction::Pressed) {
        to_server.write(ToServer(ClientMessage::Fishing(FishingCommand::Stop)));
    }
}
