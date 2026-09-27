//! The party tab (**P**, first on the toolbar). Outside a party, it says so,
//! with a box for a party name and a Create party button (the same as
//! typing `/party NAME`). In a party, it shows its name, everyone in it
//! (level, name and class), and buttons to invite the selected player
//! (`/invite`) and to leave (`/leave`). Everything it does goes through
//! those commands; the server decides, and answers in chat.

use bevy::prelude::*;
use venture_protocol::MAX_PARTY_NAME_CHARS;

use crate::interaction::actions::{Action, PerformAction};
use crate::ui::character_window::{
    HEADING_COLOUR, Pages, QUIET_COLOUR, TEXT_COLOUR, Tab, divider, heading, small_button, text,
};
use crate::ui::party::Party;
use crate::ui::text_input::{FieldText, KeyboardFocus, TextField, TextSubmitted};
use crate::ui::theme::{Piece, Themed};

/// Shows the party tab.
pub struct PartyTabPlugin;

impl Plugin for PartyTabPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window))
            .add_systems(Update, (show_party, type_name, press_buttons));
    }
}

/// Where the page's contents go (rebuilt when the party changes).
#[derive(Component)]
struct Contents;

/// The box to type a new party's name in.
#[derive(Component)]
struct NameField;

/// A button on the page, and the command it gives.
#[derive(Component, Clone, Copy)]
enum PartyButton {
    Create,
    Invite,
    Leave,
}

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands
        .entity(pages.of(Tab::Party))
        .with_child((Contents, Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }));
}

/// Shows the party, or how to start one, whenever it changes.
fn show_party(mut commands: Commands, party: Res<Party>, contents: Single<Entity, With<Contents>>) {
    if !party.is_changed() {
        return;
    }
    commands.entity(*contents).despawn_children().with_children(|page| match &party.info {
        None => {
            page.spawn(heading("No party"));
            page.spawn(text(
                "You aren't in a party. Name one to start it, then invite others to join.",
                12.0,
                TEXT_COLOUR,
            ));
            page.spawn(Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                row.spawn((
                    NameField,
                    Button,
                    TextField {
                        text: String::new(),
                        max_chars: MAX_PARTY_NAME_CHARS,
                        placeholder: "Party name".to_string(),
                    },
                    Node { flex_grow: 1.0, padding: UiRect::axes(px(8), px(4)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                    Themed(Piece::TextField),
                ))
                .with_child((
                    FieldText,
                    Text::default(),
                    TextFont { font_size: FontSize::Px(13.0), ..default() },
                ));
                row.spawn((PartyButton::Create, small_button("Create party")));
            });
            page.spawn(text("Or type /party NAME in chat.", 11.0, QUIET_COLOUR));
        }
        Some(info) => {
            page.spawn(heading(&info.name));
            for member in &info.members {
                page.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(
                    |row| {
                        row.spawn(text(format!("Lv {}", member.level), 12.0, HEADING_COLOUR));
                        row.spawn(text(member.name.clone(), 13.0, TEXT_COLOUR));
                        row.spawn(text(member.class.clone(), 11.0, QUIET_COLOUR));
                    },
                );
            }
            page.spawn(divider());
            page.spawn(Node { column_gap: px(6), ..default() }).with_children(|row| {
                row.spawn((PartyButton::Invite, small_button("Invite target")));
                row.spawn((PartyButton::Leave, small_button("Leave party")));
            });
            page.spawn(text("Select a player, then invite them (or type /invite NAME).", 11.0, QUIET_COLOUR));
        }
    });
}

/// Clicking the name box types in it; Enter in it starts the party.
fn type_name(
    mut focus: ResMut<KeyboardFocus>,
    fields: Query<(Entity, &Interaction), (With<NameField>, Changed<Interaction>)>,
    mut submitted: MessageReader<TextSubmitted>,
    names: Query<(), With<NameField>>,
    mut actions: MessageWriter<PerformAction>,
) {
    for (field, interaction) in &fields {
        if *interaction == Interaction::Pressed && focus.0.is_none() {
            focus.0 = Some(field);
        }
    }
    for submission in submitted.read().filter(|submission| names.contains(submission.field)) {
        focus.0 = None;
        actions.write(PerformAction::with(Action::Party, &submission.text));
    }
}

/// The buttons give their commands.
fn press_buttons(
    buttons: Query<(&Interaction, &PartyButton), Changed<Interaction>>,
    field: Option<Single<(Entity, &TextField), With<NameField>>>,
    mut focus: ResMut<KeyboardFocus>,
    mut actions: MessageWriter<PerformAction>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let action = match button {
            PartyButton::Create => {
                let Some((entity, name)) = field.as_deref().map(|(entity, field)| (*entity, field.text.clone())) else {
                    continue;
                };
                if focus.0 == Some(entity) {
                    focus.0 = None;
                }
                PerformAction::with(Action::Party, &name)
            }
            PartyButton::Invite => PerformAction::new(Action::Invite),
            PartyButton::Leave => PerformAction::new(Action::Leave),
        };
        actions.write(action);
    }
}
