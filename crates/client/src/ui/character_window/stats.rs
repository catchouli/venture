//! The stats tab: the character's name, class and level (with an experience
//! bar), their five stats with a **+** to spend each stat point, and
//! the secondary stats the server worked out from them.

use bevy::prelude::*;
use venture_protocol::ClientMessage;
use venture_rpg::{Stat, modifier};

use crate::characters::nameplates::Nameplate;
use crate::characters::player::LocalPlayer;
use crate::network::ToServer;
use crate::ui::character_window::{
    EXPERIENCE_COLOUR, HEADING_COLOUR, Pages, QUIET_COLOUR, Sheet, TEXT_COLOUR, Tab, bar, heading, small_button, text,
};
/// Colour of unspent points.
const POINTS_COLOUR: Color = Color::srgb(0.6, 0.9, 0.55);

/// Shows the stats tab.
pub struct StatsPlugin;

impl Plugin for StatsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (show_sheet, spend_on_click));
    }
}

/// A piece of the page that shows part of the sheet.
#[derive(Component, Clone, Copy)]
enum Field {
    Name,
    ClassAndLevel,
    Experience,
    Points,
    Score(Stat),
    Secondary(Secondary),
}

/// The secondary stats shown.
#[derive(Clone, Copy)]
enum Secondary {
    Health,
    Mana,
    ArmourClass,
    AttackBonus,
    Damage,
    AttackSpeed,
}

impl Secondary {
    const ALL: [Secondary; 6] = [
        Secondary::Health,
        Secondary::Mana,
        Secondary::ArmourClass,
        Secondary::AttackBonus,
        Secondary::Damage,
        Secondary::AttackSpeed,
    ];

    fn label(self) -> &'static str {
        match self {
            Secondary::Health => "Health",
            Secondary::Mana => "Mana",
            Secondary::ArmourClass => "Armour class",
            Secondary::AttackBonus => "Attack bonus",
            Secondary::Damage => "Damage",
            Secondary::AttackSpeed => "Attacks per second",
        }
    }
}

/// The experience bar's fill.
#[derive(Component)]
struct ExperienceFill;

/// The button spending a stat point on a stat.
#[derive(Component)]
struct PlusButton(Stat);

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Stats)).with_children(|page| {
        page.spawn((Field::Name, text("", 18.0, HEADING_COLOUR)));
        page.spawn((Field::ClassAndLevel, text("", 13.0, TEXT_COLOUR)));
        page.spawn(bar(10.0, EXPERIENCE_COLOUR, 0.0, ExperienceFill));
        page.spawn((Field::Experience, text("", 12.0, QUIET_COLOUR)));

        page.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|row| {
            row.spawn(heading("Stats"));
            row.spawn((Field::Points, text("", 12.0, POINTS_COLOUR), Node { margin: UiRect::top(px(6)), ..default() }));
        });
        for stat in Stat::ALL {
            page.spawn(Node { align_items: AlignItems::Center, column_gap: px(8), height: px(22), ..default() })
                .with_children(|row| {
                    row.spawn((text(stat.short_name(), 13.0, HEADING_COLOUR), Node { width: px(40), ..default() }));
                    // (Given a height, as its text starts out empty.)
                    row.spawn((
                        Field::Score(stat),
                        text("", 13.0, TEXT_COLOUR),
                        Node { width: px(90), height: px(18), ..default() },
                    ));
                    row.spawn((PlusButton(stat), small_button("+")));
                });
        }

        page.spawn(heading("Details"));
        for stat in Secondary::ALL {
            page.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|row| {
                row.spawn(text(stat.label(), 13.0, QUIET_COLOUR));
                row.spawn((Field::Secondary(stat), text("", 13.0, TEXT_COLOUR)));
            });
        }
    });
}

/// Fills the page in from the latest sheet.
fn show_sheet(
    sheet: Res<Sheet>,
    player: Option<Single<&Nameplate, With<LocalPlayer>>>,
    mut fields: Query<(&Field, &mut Text)>,
    mut fill: Single<&mut Node, With<ExperienceFill>>,
    mut plus_buttons: Query<&mut Visibility, With<PlusButton>>,
) {
    // (Filled every frame, as it's cheap: only text that differs is changed,
    // and the player's name can arrive after the sheet.)
    let Some(sheet) = &sheet.0 else { return };
    let name = player.map(|nameplate| nameplate.0.clone()).unwrap_or_default();
    let signed = |value: i32| format!("{value:+}");
    for (field, mut shown) in &mut fields {
        let value = match field {
            Field::Name => name.clone(),
            Field::ClassAndLevel => format!("Level {} {}", sheet.level, sheet.class),
            Field::Experience if sheet.experience_to_next == 0 => "The highest level".to_string(),
            Field::Experience => format!("{} / {} experience", sheet.experience, sheet.experience_to_next),
            Field::Points if sheet.stat_points == 1 => "1 point to spend".to_string(),
            Field::Points if sheet.stat_points > 0 => format!("{} points to spend", sheet.stat_points),
            Field::Points => String::new(),
            Field::Score(stat) => {
                let score = sheet.stats.score(*stat);
                format!("{score}  ({})", signed(modifier(score)))
            }
            Field::Secondary(Secondary::Health) => sheet.max_health.to_string(),
            Field::Secondary(Secondary::Mana) => sheet.max_mana.to_string(),
            Field::Secondary(Secondary::ArmourClass) => sheet.armour_class.to_string(),
            Field::Secondary(Secondary::AttackBonus) => signed(sheet.attack_bonus),
            Field::Secondary(Secondary::Damage) => sheet.damage.to_string(),
            Field::Secondary(Secondary::AttackSpeed) => format!("{:.2}", sheet.attacks_per_second),
        };
        if shown.0 != value {
            shown.0 = value;
        }
    }
    let progress =
        if sheet.experience_to_next == 0 { 1.0 } else { sheet.experience as f32 / sheet.experience_to_next as f32 };
    fill.width = percent(progress.clamp(0.0, 1.0) * 100.0);
    let shown = if sheet.stat_points > 0 { Visibility::Inherited } else { Visibility::Hidden };
    for mut visibility in &mut plus_buttons {
        *visibility = shown;
    }
}

/// Clicking a **+** spends a stat point on that stat.
fn spend_on_click(
    buttons: Query<(&Interaction, &PlusButton), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            to_server.write(ToServer(ClientMessage::SpendStatPoint { stat: button.0 }));
        }
    }
}
