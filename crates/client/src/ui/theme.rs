//! The interface's look, from the content's theme (`ui/theme.ron` and its
//! pictures): each frame, button, slot, tab and bar is marked with the
//! [`Piece`] it is ([`Themed`]), and drawn with that piece's picture as a
//! nine-slice (see [`ThemeDef`]). Buttons, slots, tabs and close buttons
//! change picture under the mouse (`_hover`) and when pressed (`_pressed`),
//! if the theme has those pictures. Bars' backs and fills are tinted their
//! bar's colours.
//!
//! A piece the theme has no picture for (or everything, before the content
//! has arrived) keeps its plain look: the flat colours it was made with.
//! Changing the theme's files while the game runs restyles everything at once.
//!
//! The theme can also name a font for all the interface's text.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::sprite::{BorderRect, SliceScaleMode, TextureSlicer};
use bevy::ui::{FocusPolicy, UiSystems, widget::NodeImageMode};
use venture_content::ThemeDef;

use crate::content::ContentStore;

/// Applies the theme.
pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .add_systems(PreUpdate, load_theme)
            .add_systems(PostUpdate, (skin, hide_flat_details, use_theme_font).before(UiSystems::Prepare));
    }
}

/// A kind of interface piece, each with its own picture in the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Piece {
    /// A window's frame.
    Window,
    /// The strip across the top of a window.
    Header,
    /// A smaller frame.
    Panel,
    /// The chat log's backing.
    Chat,
    /// A tooltip.
    Tooltip,
    /// A button with words on it.
    Button,
    /// A button for the option that's chosen (such as a class, when making a
    /// character). Drawn as a pressed button if the theme has no picture for it.
    ButtonChosen,
    /// A box holding an icon.
    Slot,
    /// The empty part of a bar.
    BarBack,
    /// The full part of a bar (tinted the bar's colour).
    BarFill,
    /// Drawn over a whole bar (its rim, or a shine), not tinted.
    BarFrame,
    /// A close button.
    Close,
    /// A line under headings.
    Divider,
    /// A box to type in.
    TextField,
    /// A toolbar button.
    ToolbarButton,
    /// The toolbar button for the tab that's open. Drawn as a toolbar button
    /// under the mouse if the theme has no picture for it.
    ToolbarButtonOpen,
}

impl Piece {
    /// Its name in the theme.
    pub fn name(self) -> &'static str {
        match self {
            Piece::Window => "window",
            Piece::Header => "header",
            Piece::Panel => "panel",
            Piece::Chat => "chat",
            Piece::Tooltip => "tooltip",
            Piece::Button => "button",
            Piece::ButtonChosen => "button_chosen",
            Piece::Slot => "slot",
            Piece::BarBack => "bar_back",
            Piece::BarFill => "bar_fill",
            Piece::BarFrame => "bar_frame",
            Piece::Close => "close",
            Piece::Divider => "divider",
            Piece::TextField => "text_field",
            Piece::ToolbarButton => "toolbar_button",
            Piece::ToolbarButtonOpen => "toolbar_button_open",
        }
    }
}

/// Marks an interface node as a piece of the theme, drawn with its picture.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Themed(pub Piece);

/// Something only shown in the plain look: hidden when the theme has a
/// picture for this piece (such as the plain close button's cross, which the
/// theme's picture draws itself).
#[derive(Component, Clone, Copy, Debug)]
pub struct FlatOnly(pub Piece);

/// A node drawn with the theme: its backdrop (a child behind everything
/// else in it, drawing the picture), and the plain look it had before (to go
/// back to if the theme loses its picture).
///
/// (The picture is drawn by a child, not the node itself, because an image
/// on a node would make the layout size the node to the picture instead of
/// to what's in it.)
#[derive(Component)]
pub struct Skinned {
    backdrop: Entity,
    background: Color,
    border: Option<BorderColor>,
}

/// The child of a themed node that draws its picture.
#[derive(Component)]
struct Backdrop;

/// The theme, ready to draw with.
#[derive(Resource, Default)]
pub struct Theme {
    /// Each picture, and how to slice it, by piece name (including `_hover`
    /// and `_pressed` ones).
    pieces: HashMap<String, (Handle<Image>, TextureSlicer)>,
    /// The toolbar's icon for each tab, by tab name.
    tab_icons: HashMap<String, Handle<Image>>,
    /// The font for all text, if not the built-in one.
    font: Option<Handle<Font>>,
}

impl Theme {
    /// The icon for a tab (by its name, such as `"stats"`), if the theme has one.
    pub fn tab_icon(&self, tab: &str) -> Option<Handle<Image>> {
        self.tab_icons.get(tab).cloned()
    }

    /// The picture for a piece in a state (`""`, `"_hover"` or `"_pressed"`),
    /// falling back to the plainer states it has.
    fn picture(&self, piece: Piece, state: &str) -> Option<&(Handle<Image>, TextureSlicer)> {
        if piece == Piece::ButtonChosen && !self.pieces.contains_key("button_chosen") {
            return self.picture(Piece::Button, "_pressed");
        }
        if piece == Piece::ToolbarButtonOpen && !self.pieces.contains_key("toolbar_button_open") {
            return self.picture(Piece::ToolbarButton, "_hover");
        }
        let name = piece.name();
        let fallbacks: &[&str] = match state {
            "_pressed" => &["_pressed", "_hover", ""],
            "_hover" => &["_hover", ""],
            _ => &[""],
        };
        fallbacks.iter().find_map(|suffix| self.pieces.get(&format!("{name}{suffix}")))
    }
}

/// Rebuilds the theme whenever the content changes.
fn load_theme(content: Res<ContentStore>, mut theme: ResMut<Theme>, mut fonts: ResMut<Assets<Font>>) {
    if !content.is_changed() {
        return;
    }
    let def: &ThemeDef = &content.theme;
    let pieces = def
        .pieces
        .iter()
        .filter_map(|(name, piece)| {
            let image = content.ui_images.get(&piece.image)?.clone();
            let [left, right, top, bottom] = piece.border.sides();
            let scale = if piece.tile { SliceScaleMode::Tile { stretch_value: 1.0 } } else { SliceScaleMode::Stretch };
            let slicer = TextureSlicer {
                border: BorderRect { min_inset: Vec2::new(left, top), max_inset: Vec2::new(right, bottom) },
                center_scale_mode: scale,
                sides_scale_mode: scale,
                max_corner_scale: 1.0,
            };
            Some((name.clone(), (image, slicer)))
        })
        .collect();
    let tab_icons = def
        .tab_icons
        .iter()
        .filter_map(|(tab, icon)| Some((tab.clone(), content.ui_images.get(icon)?.clone())))
        .collect();
    let font = def.font.as_ref().and_then(|font| content.fonts.get(font)).map(|font| fonts.add(font.clone()));
    *theme = Theme { pieces, tab_icons, font };
}

/// Draws each themed node with its piece's picture (for its state), or puts
/// its plain look back if the theme hasn't got one.
fn skin(
    mut commands: Commands,
    theme: Res<Theme>,
    mut nodes: Query<(
        Entity,
        &Themed,
        Option<&Interaction>,
        Option<&Skinned>,
        &mut BackgroundColor,
        Option<&mut BorderColor>,
    )>,
    mut backdrops: Query<&mut ImageNode, With<Backdrop>>,
) {
    for (entity, themed, interaction, skinned, mut background, border) in &mut nodes {
        let state = match interaction {
            Some(Interaction::Pressed) => "_pressed",
            Some(Interaction::Hovered) => "_hover",
            _ => "",
        };
        let Some((picture, slicer)) = theme.picture(themed.0, state) else {
            // No picture: back to the plain look.
            if let Some(skinned) = skinned {
                background.0 = skinned.background;
                if let (Some(mut border), Some(plain)) = (border, skinned.border) {
                    *border = plain;
                }
                commands.entity(skinned.backdrop).despawn();
                commands.entity(entity).remove::<Skinned>();
            }
            continue;
        };
        let plain = skinned.map_or(background.0, |skinned| skinned.background);
        // Bars' backs and fills are drawn light and tinted the bar's colours; the rest as painted.
        let tint = if matches!(themed.0, Piece::BarBack | Piece::BarFill) { plain } else { Color::WHITE };
        let image = ImageNode {
            image: picture.clone(),
            image_mode: NodeImageMode::Sliced(slicer.clone()),
            color: tint,
            ..default()
        };
        match skinned.and_then(|skinned| backdrops.get_mut(skinned.backdrop).ok()) {
            Some(mut shown) => {
                if shown.image != image.image || shown.image_mode != image.image_mode || shown.color != image.color {
                    *shown = image;
                }
            }
            None => {
                let backdrop = commands
                    .spawn((
                        Backdrop,
                        image,
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: px(0),
                            bottom: px(0),
                            ..default()
                        },
                        FocusPolicy::Pass,
                    ))
                    .id();
                // First, so it's drawn behind everything else in the node.
                commands.entity(entity).insert_children(0, &[backdrop]);
                commands.entity(entity).insert(Skinned {
                    backdrop,
                    background: background.0,
                    border: border.as_deref().copied(),
                });
            }
        }
        if background.0 != Color::NONE {
            background.0 = Color::NONE;
        }
        if let Some(mut border) = border
            && *border != BorderColor::all(Color::NONE)
        {
            *border = BorderColor::all(Color::NONE);
        }
    }
}

/// Hides plain-look details whose piece the theme draws.
fn hide_flat_details(
    theme: Res<Theme>,
    mut details: Query<(&FlatOnly, &mut Visibility)>,
    added: Query<(), Added<FlatOnly>>,
) {
    if !theme.is_changed() && added.is_empty() {
        return;
    }
    for (FlatOnly(piece), mut visibility) in &mut details {
        *visibility = if theme.picture(*piece, "").is_some() { Visibility::Hidden } else { Visibility::Inherited };
    }
}

/// Uses the theme's font (if it has one) for all text that uses the built-in
/// one, and goes back to the built-in one if the theme stops naming a font.
fn use_theme_font(theme: Res<Theme>, mut applied: Local<Option<Handle<Font>>>, mut texts: Query<&mut TextFont>) {
    let built_in = FontSource::default();
    let theme_changed = theme.font != *applied;
    if !theme_changed && applied.is_none() {
        return;
    }
    let old = applied.clone().map(FontSource::Handle);
    let wanted = theme.font.clone().map_or(built_in.clone(), FontSource::Handle);
    for mut text in &mut texts {
        // Everything when the theme changes; otherwise just new text.
        if !theme_changed && !text.is_added() {
            continue;
        }
        let ours = text.font == built_in || old.as_ref() == Some(&text.font);
        if ours && text.font != wanted {
            text.font = wanted.clone();
        }
    }
    applied.clone_from(&theme.font);
}
