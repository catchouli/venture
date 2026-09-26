//! The skills tab: the **actions** every character has (attacking, talking,
//! following), the **skills** the character knows, and the class skills they
//! can learn with skill points (from a level, for a cost).
//!
//! Clicking an action does it, just like typing its chat command. (Using
//! skills comes with fighting: see `docs/plan-combat.md`.)

use bevy::prelude::*;
use venture_protocol::ClientMessage;

use crate::content::ContentStore;
use crate::interaction::actions::{Action, PerformAction};
use crate::network::ToServer;
use crate::ui::character_window::{
    Pages, QUIET_COLOUR, Sheet, TEXT_COLOUR, Tab, heading, icon_box, small_button, text,
};
use crate::ui::chat::SystemMessage;
use crate::ui::tooltip::Tooltip;

/// Size of an action's or skill's box, in pixels.
const BOX_SIZE: f32 = 40.0;
/// Size of an icon in its box, in pixels.
const ICON_SIZE: f32 = 32.0;
/// Colour of unspent skill points.
const POINTS_COLOUR: Color = Color::srgb(0.6, 0.9, 0.55);
/// How faded a skill not yet learned looks.
const LOCKED_TINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);
/// The actions shown, in order.
const ACTIONS: [Action; 3] = [Action::Attack, Action::Talk, Action::Follow];

/// Shows the skills tab.
pub struct SkillsPlugin;

impl Plugin for SkillsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (show_actions, show_skills, use_on_click, learn_on_click));
    }
}

/// An action's box.
#[derive(Component)]
struct ActionButton(Action);

/// A known skill's box.
#[derive(Component)]
struct SkillButton(String);

/// The button learning a skill.
#[derive(Component)]
struct LearnButton(String);

/// Where the known skills go.
#[derive(Component)]
struct KnownSkills;

/// Where the skills to learn go.
#[derive(Component)]
struct SkillsToLearn;

/// How many skill points are left.
#[derive(Component)]
struct PointsText;

/// The content icon for an action.
fn action_icon(action: Action) -> &'static str {
    match action {
        Action::Attack => "action_attack.png",
        Action::Talk => "action_talk.png",
        Action::Follow => "action_follow.png",
        Action::Wave => "emote_wave.png",
        _ => "",
    }
}

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Skills)).with_children(|page| {
        page.spawn(heading("Actions"));
        page.spawn(Node { column_gap: px(6), ..default() }).with_children(|row| {
            for action in ACTIONS {
                row.spawn((ActionButton(action), icon_box(BOX_SIZE))).with_child((
                    ImageNode::default(),
                    Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                ));
            }
        });
        page.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|row| {
            row.spawn(heading("Skills"));
            row.spawn((PointsText, text("", 12.0, POINTS_COLOUR), Node { margin: UiRect::top(px(6)), ..default() }));
        });
        page.spawn((KnownSkills, Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }));
        page.spawn((SkillsToLearn, Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }));
    });
}

/// Gives the actions their icons and tooltips.
fn show_actions(
    content: Res<ContentStore>,
    new_actions: Query<(), Added<ActionButton>>,
    mut actions: Query<(&ActionButton, &Children, &mut Tooltip)>,
    mut icons: Query<&mut ImageNode>,
) {
    if !content.is_changed() && new_actions.is_empty() {
        return;
    }
    for (button, children, mut tooltip) in &mut actions {
        let name = button.0.name();
        let mut title = name.to_string();
        title[..1].make_ascii_uppercase();
        *tooltip = Tooltip { title, body: format!("{} (/{name})", capitalised(button.0.description())) };
        if let (Some(&child), Some(icon)) = (children.first(), content.icons.get(action_icon(button.0)))
            && let Ok(mut image) = icons.get_mut(child)
        {
            image.image = icon.clone();
        }
    }
}

/// The text with its first letter in capitals.
fn capitalised(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map(|first| first.to_uppercase().chain(characters).collect()).unwrap_or_default()
}

/// Shows the skills the character knows, and the ones they can learn.
fn show_skills(
    mut commands: Commands,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    known: Single<Entity, With<KnownSkills>>,
    to_learn: Single<Entity, With<SkillsToLearn>>,
    mut points: Single<&mut Text, With<PointsText>>,
) {
    if !sheet.is_changed() && !content.is_changed() {
        return;
    }
    let Some(sheet) = &sheet.0 else { return };
    points.0 = match sheet.skill_points {
        0 => String::new(),
        1 => "1 skill point to spend".to_string(),
        points => format!("{points} skill points to spend"),
    };
    let icon = |skill: &str| content.skills.get(skill).and_then(|def| content.icons.get(&def.icon)).cloned();
    let details = |skill: &str| match content.skills.get(skill) {
        Some(def) => {
            let mut parts = Vec::new();
            if def.mana > 0 {
                parts.push(format!("Mana {}", def.mana));
            }
            if def.cooldown > 0.0 {
                parts.push(format!("cooldown {} s", def.cooldown));
            }
            if def.range > 0.0 {
                parts.push(format!("range {} m", def.range));
            }
            let costs = if parts.is_empty() { String::new() } else { format!("\n{}.", parts.join(", ")) };
            Tooltip { title: def.name.clone(), body: format!("{}{costs}", def.description) }
        }
        None => Tooltip { title: skill.to_string(), body: String::new() },
    };

    commands.entity(*known).despawn_children().with_children(|known| {
        for skill in &sheet.skills {
            let mut tooltip = details(skill);
            tooltip.body.push_str("\n(Using skills comes with fighting.)");
            known.spawn((SkillButton(skill.clone()), icon_box(BOX_SIZE))).insert(tooltip).with_child((
                ImageNode::new(icon(skill).unwrap_or_default()),
                Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
            ));
        }
    });

    commands.entity(*to_learn).despawn_children().with_children(|list| {
        let unlearned: Vec<_> =
            sheet.class_skills.iter().filter(|offer| !sheet.skills.contains(&offer.skill)).collect();
        if unlearned.is_empty() {
            return;
        }
        list.spawn(heading("To learn"));
        for offer in unlearned {
            let name = content.skills.get(&offer.skill).map_or(offer.skill.as_str(), |def| def.name.as_str());
            let points = if offer.cost == 1 { "1 point".to_string() } else { format!("{} points", offer.cost) };
            let can_learn = sheet.level >= offer.level && sheet.skill_points >= offer.cost;
            list.spawn(Node { align_items: AlignItems::Center, column_gap: px(8), ..default() }).with_children(|row| {
                row.spawn(icon_box(BOX_SIZE)).insert(details(&offer.skill)).with_child((
                    ImageNode::new(icon(&offer.skill).unwrap_or_default()).with_color(LOCKED_TINT),
                    Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                ));
                row.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(
                    |words| {
                        words.spawn(text(name, 13.0, TEXT_COLOUR));
                        words.spawn(text(format!("Level {}, {points}", offer.level), 12.0, QUIET_COLOUR));
                    },
                );
                if can_learn {
                    row.spawn((LearnButton(offer.skill.clone()), small_button("Learn")));
                }
            });
        }
    });
}

/// Clicking an action does it; clicking a skill (for now) explains they come with fighting.
fn use_on_click(
    content: Res<ContentStore>,
    actions: Query<(&Interaction, &ActionButton), Changed<Interaction>>,
    skills: Query<(&Interaction, &SkillButton), Changed<Interaction>>,
    mut perform: MessageWriter<PerformAction>,
    mut system: MessageWriter<SystemMessage>,
) {
    for (interaction, button) in &actions {
        if *interaction == Interaction::Pressed {
            perform.write(PerformAction(button.0));
        }
    }
    for (interaction, button) in &skills {
        if *interaction == Interaction::Pressed {
            let name = content.skills.get(&button.0).map_or(button.0.as_str(), |skill| skill.name.as_str());
            system.write(SystemMessage(format!("You'll be able to use {name} once there's something to fight.")));
        }
    }
}

/// Clicking **Learn** asks to learn the skill.
fn learn_on_click(
    buttons: Query<(&Interaction, &LearnButton), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            to_server.write(ToServer(ClientMessage::LearnSkill { skill: button.0.clone() }));
        }
    }
}
