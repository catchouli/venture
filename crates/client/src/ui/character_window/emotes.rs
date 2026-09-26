//! The emotes tab: gestures for everyone nearby to see. Clicking one does
//! it, just like typing its chat command (`/wave`); they can be dragged onto
//! the hotbar too (see [`crate::ui::hotbar`], which does the clicking).

use bevy::prelude::*;

use venture_protocol::HotbarEntry;

use crate::content::ContentStore;
use crate::interaction::actions::Action;
use crate::ui::character_window::{Pages, Tab, heading, icon_box};
use crate::ui::hotbar::Draggable;
use crate::ui::tooltip::Tooltip;

/// Size of an emote's box, in pixels.
const BOX_SIZE: f32 = 40.0;
/// Size of an icon in its box, in pixels.
const ICON_SIZE: f32 = 32.0;
/// The emotes shown: the action each does, its name, and its content icon.
const EMOTES: [(Action, &str, &str); 1] = [(Action::Wave, "Wave", "emote_wave.png")];

/// Shows the emotes tab.
pub struct EmotesPlugin;

impl Plugin for EmotesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, fill_page.after(super::spawn_window)).add_systems(Update, show_icons);
    }
}

/// An emote's box, by its position in [`EMOTES`].
#[derive(Component)]
struct EmoteButton(usize);

fn fill_page(mut commands: Commands, pages: Res<Pages>) {
    commands.entity(pages.of(Tab::Emotes)).with_children(|page| {
        page.spawn(heading("Emotes"));
        page.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }).with_children(
            |row| {
                for (index, (action, name, _)) in EMOTES.iter().enumerate() {
                    let tooltip = Tooltip { title: name.to_string(), body: format!("/{}", action.name()) };
                    let mut button = row.spawn((EmoteButton(index), icon_box(BOX_SIZE)));
                    button.insert(tooltip);
                    if let Some(on_hotbar) = action.on_hotbar() {
                        button.insert(Draggable(HotbarEntry::Action(on_hotbar)));
                    }
                    button.with_child((
                        ImageNode::default(),
                        Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
                    ));
                }
            },
        );
    });
}

fn show_icons(
    content: Res<ContentStore>,
    new_buttons: Query<(), Added<EmoteButton>>,
    buttons: Query<(&EmoteButton, &Children)>,
    mut images: Query<&mut ImageNode>,
) {
    if !content.is_changed() && new_buttons.is_empty() {
        return;
    }
    for (button, children) in &buttons {
        let Some(icon) = content.icons.get(EMOTES[button.0].2) else { continue };
        if let Some(mut image) = children.first().and_then(|&child| images.get_mut(child).ok()) {
            image.image = icon.clone();
        }
    }
}
