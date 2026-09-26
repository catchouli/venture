//! The abilities tab: the **actions** every character has (attacking, talking,
//! following), the **abilities** the character knows, and the class abilities they
//! can learn with ability points (from a level, for a cost).
//!
//! Clicking an action does it, just like typing its chat command, and
//! clicking an ability uses it on the selected target. Both can be dragged onto
//! the hotbar (see [`crate::ui::hotbar`]), which does the clicking too.

use bevy::prelude::*;
use venture_protocol::{ClientMessage, HotbarEntry};

use crate::content::ContentStore;
use crate::interaction::actions::Action;
use crate::network::ToServer;
use crate::ui::character_window::{
    Pages, QUIET_COLOUR, Sheet, TEXT_COLOUR, Tab, heading, icon_box, small_button, text,
};
use crate::ui::hotbar::Draggable;
use crate::ui::tooltip::Tooltip;

/// Size of an action's or ability's box, in pixels.
const BOX_SIZE: f32 = 40.0;
/// Size of an icon in its box, in pixels.
const ICON_SIZE: f32 = 32.0;
/// Colour of unspent ability points.
const POINTS_COLOUR: Color = Color::srgb(0.6, 0.9, 0.55);
/// How faded an ability not yet learned looks.
const LOCKED_TINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);
/// The actions shown, in order.
const ACTIONS: [Action; 4] = [Action::Attack, Action::Talk, Action::Follow, Action::Town];

/// Shows the abilities tab.
pub struct AbilitiesPlugin;

impl Plugin for AbilitiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (show_actions, show_abilities, learn_on_click));
    }
}

/// An action's box.
#[derive(Component)]
struct ActionButton(Action);

/// The button learning an ability.
#[derive(Component)]
struct LearnButton(String);

/// Where the known abilities go.
#[derive(Component)]
struct KnownAbilities;

/// Where the abilities to learn go.
#[derive(Component)]
struct AbilitiesToLearn;

/// How many ability points are left.
#[derive(Component)]
struct PointsText;

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Abilities)).with_children(|page| {
        page.spawn(heading("Actions"));
        page.spawn(Node { column_gap: px(6), ..default() }).with_children(|row| {
            for action in ACTIONS {
                let mut button = row.spawn((ActionButton(action), icon_box(BOX_SIZE)));
                if let Some(on_hotbar) = action.on_hotbar() {
                    button.insert(Draggable(HotbarEntry::Action(on_hotbar)));
                }
                button.with_child((
                    ImageNode::default(),
                    Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                ));
            }
        });
        page.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|row| {
            row.spawn(heading("Abilities"));
            row.spawn((PointsText, text("", 12.0, POINTS_COLOUR), Node { margin: UiRect::top(px(6)), ..default() }));
        });
        page.spawn((
            KnownAbilities,
            Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() },
        ));
        page.spawn((AbilitiesToLearn, Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }));
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
        let body = format!(
            "{} (/{name}).\nDrag it onto the hotbar to use it with a key.",
            capitalised(button.0.description())
        );
        *tooltip = Tooltip { title: button.0.title(), body };
        if let (Some(&child), Some(icon)) = (children.first(), button.0.icon().and_then(|icon| content.icons.get(icon)))
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

/// Shows the abilities the character knows, and the ones they can learn.
fn show_abilities(
    mut commands: Commands,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    known: Single<Entity, With<KnownAbilities>>,
    to_learn: Single<Entity, With<AbilitiesToLearn>>,
    mut points: Single<&mut Text, With<PointsText>>,
) {
    if !sheet.is_changed() && !content.is_changed() {
        return;
    }
    let Some(sheet) = &sheet.0 else { return };
    points.0 = match sheet.ability_points {
        0 => String::new(),
        1 => "1 ability point to spend".to_string(),
        points => format!("{points} ability points to spend"),
    };
    let icon = |ability: &str| content.abilities.get(ability).and_then(|def| content.icons.get(&def.icon)).cloned();
    let details = |ability: &str| match content.abilities.get(ability) {
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
        None => Tooltip { title: ability.to_string(), body: String::new() },
    };

    commands.entity(*known).despawn_children().with_children(|known| {
        for ability in &sheet.abilities {
            let mut tooltip = details(ability);
            tooltip.body.push_str("\nClick to use it, or drag it onto the hotbar.");
            known
                .spawn((Draggable(HotbarEntry::Ability(ability.clone())), icon_box(BOX_SIZE)))
                .insert(tooltip)
                .with_child((
                    ImageNode::new(icon(ability).unwrap_or_default()),
                    Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                ));
        }
    });

    commands.entity(*to_learn).despawn_children().with_children(|list| {
        let unlearned: Vec<_> =
            sheet.class_abilities.iter().filter(|offer| !sheet.abilities.contains(&offer.ability)).collect();
        if unlearned.is_empty() {
            return;
        }
        list.spawn(heading("To learn"));
        for offer in unlearned {
            let name = content.abilities.get(&offer.ability).map_or(offer.ability.as_str(), |def| def.name.as_str());
            let points = if offer.cost == 1 { "1 point".to_string() } else { format!("{} points", offer.cost) };
            let can_learn = sheet.level >= offer.level && sheet.ability_points >= offer.cost;
            list.spawn(Node { align_items: AlignItems::Center, column_gap: px(8), ..default() }).with_children(|row| {
                row.spawn(icon_box(BOX_SIZE)).insert(details(&offer.ability)).with_child((
                    ImageNode::new(icon(&offer.ability).unwrap_or_default()).with_color(LOCKED_TINT),
                    Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                ));
                row.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(
                    |words| {
                        words.spawn(text(name, 13.0, TEXT_COLOUR));
                        words.spawn(text(format!("Level {}, {points}", offer.level), 12.0, QUIET_COLOUR));
                    },
                );
                if can_learn {
                    row.spawn((LearnButton(offer.ability.clone()), small_button("Learn")));
                }
            });
        }
    });
}

/// Clicking **Learn** asks to learn the ability.
fn learn_on_click(
    buttons: Query<(&Interaction, &LearnButton), Changed<Interaction>>,
    mut to_server: MessageWriter<ToServer>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            to_server.write(ToServer(ClientMessage::LearnAbility { ability: button.0.clone() }));
        }
    }
}
