//! The skills tab: the character's level in each skill (gathering and
//! making things), with its experience towards the next level. Skills are
//! content (`skills/` in the content pack); the server sends each one's
//! progress in the character sheet.

use bevy::prelude::*;

use crate::content::ContentStore;
use crate::ui::character_window::{
    EXPERIENCE_COLOUR, HEADING_COLOUR, Pages, QUIET_COLOUR, Sheet, Tab, bar, heading, icon_box, text,
};
use crate::ui::tooltip::Tooltip;

/// Size of a skill's box, in pixels.
const BOX_SIZE: f32 = 40.0;
/// Size of an icon in its box, in pixels.
const ICON_SIZE: f32 = 32.0;

/// Shows the skills tab.
pub struct SkillsPlugin;

impl Plugin for SkillsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window)).add_systems(Update, show_skills);
    }
}

/// Where the skills go.
#[derive(Component)]
struct SkillList;

/// A skill's experience bar's fill.
#[derive(Component)]
struct SkillFill;

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Skills)).with_children(|page| {
        page.spawn(heading("Skills"));
        page.spawn((SkillList, Node { flex_direction: FlexDirection::Column, row_gap: px(8), ..default() }));
    });
}

/// Lists each skill from the latest sheet: its icon, name, level and experience.
fn show_skills(
    mut commands: Commands,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    list: Single<Entity, With<SkillList>>,
) {
    if !sheet.is_changed() && !content.is_changed() {
        return;
    }
    let Some(sheet) = &sheet.0 else { return };
    commands.entity(*list).despawn_children().with_children(|list| {
        for progress in &sheet.skills {
            let def = content.skills.get(&progress.skill);
            let name = def.map_or(progress.skill.as_str(), |def| def.name.as_str());
            let icon = def.and_then(|def| content.icons.get(&def.icon)).cloned().unwrap_or_default();
            let tooltip =
                Tooltip { title: name.to_string(), body: def.map(|def| def.description.clone()).unwrap_or_default() };
            let fraction = progress.experience as f32 / progress.experience_to_next.max(1) as f32;
            let experience = if progress.experience_to_next == 0 {
                "Top level".to_string()
            } else {
                format!("{} / {} experience", progress.experience, progress.experience_to_next)
            };
            list.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }).with_children(
                |row| {
                    row.spawn(icon_box(BOX_SIZE)).insert(tooltip).with_child((
                        ImageNode::new(icon),
                        Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                    ));
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: px(3),
                        ..default()
                    })
                    .with_children(|words| {
                        words.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(
                            |line| {
                                line.spawn(text(name, 13.0, HEADING_COLOUR));
                                line.spawn(text(format!("Level {}", progress.level), 13.0, HEADING_COLOUR));
                            },
                        );
                        words.spawn(bar(8.0, EXPERIENCE_COLOUR, fraction, SkillFill));
                        words.spawn(text(experience, 11.0, QUIET_COLOUR));
                    });
                },
            );
        }
    });
}
