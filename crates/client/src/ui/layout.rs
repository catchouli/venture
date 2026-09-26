//! Interface layouts from the content: each window is built from its file in
//! `ui/layouts/` (see `docs/ui-layouts.md` and [`venture_content::layout`]),
//! with the text styles, colours and bar colours in `ui/styles.ron`. Until
//! the content has arrived (and for any layout it lacks, or can't read) the
//! copies built into the game are used. Changing a layout while the game
//! runs rebuilds it at once.
//!
//! Layouts say where things go and how they look; code says what they show
//! and do:
//! - **Data:** code puts named values in [`UiData`] (such as `health`); text
//!   (`"{health} / {health_max}"`) and bars (`bar: "health"`) show them.
//! - **Elements:** each element with an `id` gets a [`UiId`]. Code finds
//!   elements by id (or listens for [`LayoutBuilt`]) and adds its own
//!   components to them, to fill them in and react to clicks. Elements marked
//!   `template` aren't shown; code makes copies of them with [`Ui::copy`].
//!
//! A window's root element has a [`UiRoot`] naming its layout; code shows and
//! hides the window by setting the root's [`Visibility`].

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::layout::{
    TextStyle, UiAlign, UiColour, UiDirection, UiJustify, UiLength, UiNode, UiPosition, UiSize, UiStyles, UiTextAlign,
    fill_in, parse_layout, parse_styles,
};

use crate::content::ContentStore;
use crate::ui::theme::{Piece, Themed};
use crate::ui::tooltip::Tooltip;

/// The layouts built into the game, used until the content's arrive (and for
/// any the content doesn't have): the same files as in the content pack.
const BUILT_IN_LAYOUTS: [(&str, &str); 2] = [
    ("player_frame", include_str!("../../../../content/ui/layouts/player_frame.ron")),
    ("hotbar", include_str!("../../../../content/ui/layouts/hotbar.ron")),
];
/// The styles built into the game.
const BUILT_IN_STYLES: &str = include_str!("../../../../content/ui/styles.ron");

/// Builds layouts.
pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiData>()
            .init_resource::<UiTemplates>()
            .add_message::<LayoutBuilt>()
            .add_systems(Startup, use_built_in_layouts)
            .add_systems(PreUpdate, use_content_layouts)
            .add_systems(Update, (build_layouts, fill_texts, fill_tooltips, fill_bars).chain().in_set(LayoutSystems));
    }
}

/// Where layouts are built and filled in. Code reacting to [`LayoutBuilt`]
/// runs after this.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LayoutSystems;

/// Values code gives layouts to show, by name (see
/// [`venture_content::layout::UI_DATA`] for the names).
#[derive(Resource, Default)]
pub struct UiData(HashMap<String, String>);

impl UiData {
    /// Sets a value (only marking it changed if it did).
    pub fn set(data: &mut ResMut<UiData>, name: &str, value: impl ToString) {
        let value = value.to_string();
        if data.0.get(name) != Some(&value) {
            data.0.insert(name.to_string(), value);
        }
    }

    /// A value, if it's been set.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

/// An element's `id` from its layout.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct UiId(pub String);

/// A window's root element, naming its layout.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct UiRoot(pub String);

/// A layout has just been (re)built. Code gives its elements behaviour here.
#[derive(Message, Clone, Debug)]
#[allow(dead_code, reason = "root and ids are for windows that find their elements; more move to layouts soon")]
pub struct LayoutBuilt {
    /// Which layout.
    pub layout: String,
    /// Its root element.
    pub root: Entity,
    /// Its elements with ids (templates aren't included: copy them with [`Ui::copy`]).
    pub ids: HashMap<String, Entity>,
}

/// A copy of a template.
pub struct UiCopy {
    /// The copy's root element.
    pub root: Entity,
    /// Its elements with ids (the root's included, if it has one).
    pub ids: HashMap<String, Entity>,
}

/// Text that shows data: its text with `{names}`.
#[derive(Component)]
struct UiText(String);

/// A tooltip that shows data: its text with `{names}` (the first line is the title).
#[derive(Component)]
struct UiTooltip(String);

/// A bar's fill, showing the data called this (out of `<this>_max`).
#[derive(Component)]
struct UiBarFill(String);

/// The layouts in use, ready to build.
#[derive(Resource, Default)]
pub struct Layouts {
    roots: HashMap<String, UiNode>,
    styles: UiStyles,
    fonts: HashMap<String, Handle<Font>>,
    /// Pictures in `ui/`, by file name.
    images: HashMap<String, Handle<Image>>,
}

/// The templates in the layouts as last built, by layout and id: the
/// template, and the element copies go in. (Kept apart from [`Layouts`], so
/// that copying them doesn't count as changing the layouts.)
#[derive(Resource, Default)]
struct UiTemplates(HashMap<(String, String), (UiNode, Entity)>);

/// Starts with the layouts built into the game.
fn use_built_in_layouts(mut commands: Commands) {
    let roots = BUILT_IN_LAYOUTS
        .iter()
        .map(|(name, text)| (name.to_string(), parse_layout(text).expect("built-in layouts are valid")))
        .collect();
    let styles = parse_styles(BUILT_IN_STYLES).expect("the built-in styles are valid");
    commands.insert_resource(Layouts { roots, styles, ..default() });
}

/// Uses the content's layouts and styles (with the built-in ones for any it
/// lacks) whenever the content changes.
fn use_content_layouts(content: Res<ContentStore>, mut layouts: ResMut<Layouts>, mut fonts: ResMut<Assets<Font>>) {
    if !content.is_changed() || content.version.is_none() {
        return;
    }
    let mut roots: HashMap<String, UiNode> = BUILT_IN_LAYOUTS
        .iter()
        .map(|(name, text)| (name.to_string(), parse_layout(text).expect("built-in layouts are valid")))
        .collect();
    roots.extend(content.layouts.iter().map(|(name, root)| (name.clone(), root.clone())));
    let styles = content
        .ui_styles
        .clone()
        .unwrap_or_else(|| parse_styles(BUILT_IN_STYLES).expect("the built-in styles are valid"));
    let fonts = styles
        .text
        .values()
        .filter_map(|style| style.font.as_ref())
        .filter_map(|file| Some((file.clone(), fonts.add(content.fonts.get(file)?.clone()))))
        .collect();
    let images = content.ui_images.clone();
    *layouts = Layouts { roots, styles, fonts, images };
}

/// Builds every layout (again) whenever they change.
fn build_layouts(
    mut commands: Commands,
    layouts: Res<Layouts>,
    mut templates: ResMut<UiTemplates>,
    data: Res<UiData>,
    old: Query<Entity, With<UiRoot>>,
    mut built: MessageWriter<LayoutBuilt>,
) {
    if !layouts.is_changed() {
        return;
    }
    for root in &old {
        commands.entity(root).despawn();
    }
    templates.0.clear();
    for (name, root_node) in &layouts.roots {
        let mut builder = Builder { layouts: &layouts, data: &data, ids: HashMap::new(), templates: Vec::new() };
        let root = builder.spawn(&mut commands, root_node, None);
        let Builder { ids, templates: found, .. } = builder;
        commands.entity(root).insert(UiRoot(name.clone()));
        for (id, template, parent) in found {
            templates.0.insert((name.clone(), id), (template, parent));
        }
        let name = name.clone();
        built.write(LayoutBuilt { layout: name, root, ids });
    }
}

/// Keeps data-showing text up to date.
fn fill_texts(data: Res<UiData>, mut texts: Query<(&UiText, &mut Text)>) {
    for (UiText(template), mut text) in &mut texts {
        let wanted = fill_in(template, |name| data.get(name).map(str::to_string));
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// Keeps data-showing tooltips up to date.
fn fill_tooltips(data: Res<UiData>, mut tooltips: Query<(&UiTooltip, &mut Tooltip)>) {
    for (UiTooltip(template), mut tooltip) in &mut tooltips {
        let wanted = tooltip_of(&fill_in(template, |name| data.get(name).map(str::to_string)));
        if *tooltip != wanted {
            *tooltip = wanted;
        }
    }
}

/// A tooltip from its text: the first line is the title, the rest its body.
fn tooltip_of(text: &str) -> Tooltip {
    let (title, body) = text.split_once('\n').unwrap_or((text, ""));
    Tooltip { title: title.to_string(), body: body.to_string() }
}

/// Keeps bars filled to their data.
fn fill_bars(data: Res<UiData>, mut fills: Query<(&UiBarFill, &mut Node)>) {
    for (UiBarFill(name), mut node) in &mut fills {
        let wanted = percent(bar_fraction(&data, name) * 100.0);
        if node.width != wanted {
            node.width = wanted;
        }
    }
}

/// How full a bar showing `name` is, from 0 to 1.
fn bar_fraction(data: &UiData, name: &str) -> f32 {
    let number = |name: &str| data.get(name).and_then(|value| value.parse::<f32>().ok());
    match (number(name), number(&format!("{name}_max"))) {
        (Some(value), Some(max)) if max > 0.0 => (value / max).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// What code uses to work with layouts: copying templates.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Ui<'w, 's> {
    commands: Commands<'w, 's>,
    layouts: Res<'w, Layouts>,
    templates: Res<'w, UiTemplates>,
    data: Res<'w, UiData>,
}

impl<'w, 's> Ui<'w, 's> {
    /// Commands, to add to the elements (use these, rather than a separate
    /// `Commands`, so they come after the elements are made).
    pub fn commands(&mut self) -> &mut Commands<'w, 's> {
        &mut self.commands
    }

    /// Makes `count` copies of a layout's template (by its `id`), in the
    /// element the template is in, and returns them. None if the layout has
    /// no such template.
    pub fn copy(&mut self, layout: &str, template: &str, count: usize) -> Vec<UiCopy> {
        let Some((node, parent)) = self.templates.0.get(&(layout.to_string(), template.to_string())).cloned() else {
            return Vec::new();
        };
        let node = UiNode { template: false, ..node };
        (0..count)
            .map(|_| {
                let mut builder =
                    Builder { layouts: &self.layouts, data: &self.data, ids: HashMap::new(), templates: Vec::new() };
                let root = builder.spawn(&mut self.commands, &node, Some(parent));
                UiCopy { root, ids: builder.ids }
            })
            .collect()
    }
}

/// Builds elements from a layout.
struct Builder<'a> {
    layouts: &'a Layouts,
    data: &'a UiData,
    /// The elements built with ids.
    ids: HashMap<String, Entity>,
    /// The templates met: their id, the template, and where copies go.
    templates: Vec<(String, UiNode, Entity)>,
}

impl Builder<'_> {
    /// Builds an element and everything in it (in `parent`, if given), and
    /// returns it.
    fn spawn(&mut self, commands: &mut Commands, node: &UiNode, parent: Option<Entity>) -> Entity {
        let styles = &self.layouts.styles;
        let background = node.colour.as_ref().map_or(Color::NONE, |colour| colour_of(colour, styles));
        let mut element = commands.spawn((self.layout_of(node), BackgroundColor(background)));
        if let Some(parent) = parent {
            element.insert(ChildOf(parent));
        }
        if let Some(colour) = &node.border_colour {
            element.insert(BorderColor::all(colour_of(colour, styles)));
        }
        if let Some(piece) = node.piece.as_deref().and_then(Piece::named) {
            element.insert(Themed(piece));
        }
        if node.icon {
            element.insert(ImageNode::default());
        }
        if node.text.is_some() || node.style.is_some() {
            let style = node.style.as_ref().and_then(|name| styles.text.get(name)).cloned().unwrap_or_default();
            let shown = node.text.as_ref().map(|text| fill_in(text, |name| self.data.get(name).map(str::to_string)));
            element.insert(self.text_of(&style, shown.unwrap_or_default()));
            if let Some(text) = &node.text {
                element.insert(UiText(text.clone()));
            }
        }
        if node.button {
            element.insert(Button);
        }
        if let Some(tooltip) = &node.tooltip {
            let shown = fill_in(tooltip, |name| self.data.get(name).map(str::to_string));
            // (Tooltips show while the mouse is over something that notices the mouse.)
            element.insert((tooltip_of(&shown), UiTooltip(tooltip.clone()), Interaction::default()));
        }
        if let Some(layer) = node.layer {
            element.insert(GlobalZIndex(layer));
        }
        if node.hidden {
            element.insert(Visibility::Hidden);
        }
        if let Some(id) = &node.id {
            element.insert(UiId(id.clone()));
        }
        let entity = element.id();
        if let Some(id) = &node.id {
            self.ids.insert(id.clone(), entity);
        }

        // A picture goes behind everything else in it, filling it.
        if let Some(image) = node.image.as_ref().and_then(|file| self.layouts.images.get(file).cloned()) {
            commands.spawn((ImageNode::new(image), fill_parent(), ChildOf(entity)));
        }
        if let Some(bar) = &node.bar {
            self.spawn_bar(commands, entity, bar, node.bar_style.as_deref().unwrap_or(bar));
        }
        for child in &node.children {
            if child.template {
                if let Some(id) = &child.id {
                    self.templates.push((id.clone(), child.clone(), entity));
                }
                continue;
            }
            self.spawn(commands, child, Some(entity));
        }
        entity
    }

    /// A bar's look: its empty part (the element itself), its fill, and the
    /// frame over them.
    fn spawn_bar(&mut self, commands: &mut Commands, bar: Entity, data: &str, style: &str) {
        let styles = &self.layouts.styles;
        let (fill, back) = styles.bars.get(style).map_or((Color::WHITE, Color::BLACK), |colours| {
            (colour_of(&colours.fill, styles), colour_of(&colours.back, styles))
        });
        commands.entity(bar).insert((BackgroundColor(back), Themed(Piece::BarBack)));
        let fraction = bar_fraction(self.data, data);
        commands.spawn((
            UiBarFill(data.to_string()),
            Node { width: percent(fraction * 100.0), height: percent(100), ..default() },
            BackgroundColor(fill),
            Themed(Piece::BarFill),
            ChildOf(bar),
        ));
        commands.spawn((fill_parent(), BackgroundColor(Color::NONE), Themed(Piece::BarFrame), ChildOf(bar)));
    }

    /// Text in a style.
    fn text_of(&self, style: &TextStyle, text: String) -> impl Bundle {
        let styles = &self.layouts.styles;
        let colour = style.colour.as_ref().map_or(Color::WHITE, |colour| colour_of(colour, styles));
        let mut font = TextFont { font_size: FontSize::Px(style.size), ..default() };
        if let Some(handle) = style.font.as_ref().and_then(|file| self.layouts.fonts.get(file)) {
            font.font = FontSource::Handle(handle.clone());
        }
        let justify = match style.align {
            UiTextAlign::Left => Justify::Left,
            UiTextAlign::Center => Justify::Center,
            UiTextAlign::Right => Justify::Right,
        };
        let shadow = if style.shadow {
            TextShadow { offset: Vec2::splat(1.0), ..default() }
        } else {
            TextShadow { offset: Vec2::ZERO, color: Color::NONE }
        };
        (Text::new(text), font, TextColor(colour), shadow, TextLayout::justify(justify))
    }

    /// An element's placement and arrangement.
    fn layout_of(&self, node: &UiNode) -> Node {
        let size = |size: &Option<UiSize>| size.as_ref().map_or(Val::Auto, val_of);
        let edges = |edges: [f32; 4]| UiRect::new(px(edges[3]), px(edges[1]), px(edges[0]), px(edges[2]));
        Node {
            position_type: match node.position {
                UiPosition::Relative => PositionType::Relative,
                UiPosition::Absolute => PositionType::Absolute,
            },
            left: size(&node.left),
            right: size(&node.right),
            top: size(&node.top),
            bottom: size(&node.bottom),
            width: size(&node.width),
            height: size(&node.height),
            min_width: size(&node.min_width),
            min_height: size(&node.min_height),
            max_width: size(&node.max_width),
            max_height: size(&node.max_height),
            margin: edges(node.margin.sides()),
            padding: edges(node.padding.sides()),
            border: UiRect::all(px(node.border)),
            flex_direction: match node.direction {
                UiDirection::Row => FlexDirection::Row,
                UiDirection::Column => FlexDirection::Column,
            },
            row_gap: px(node.gap),
            column_gap: px(node.gap),
            align_items: align_items(node.align),
            align_self: node.align_self.map_or(AlignSelf::Auto, |align| match align {
                UiAlign::Stretch => AlignSelf::Stretch,
                UiAlign::Start => AlignSelf::FlexStart,
                UiAlign::Center => AlignSelf::Center,
                UiAlign::End => AlignSelf::FlexEnd,
            }),
            justify_content: match node.justify {
                UiJustify::Start => JustifyContent::FlexStart,
                UiJustify::Center => JustifyContent::Center,
                UiJustify::End => JustifyContent::FlexEnd,
                UiJustify::SpaceBetween => JustifyContent::SpaceBetween,
                UiJustify::SpaceAround => JustifyContent::SpaceAround,
            },
            flex_wrap: if node.wrap { FlexWrap::Wrap } else { FlexWrap::NoWrap },
            flex_grow: node.grow,
            border_radius: BorderRadius::all(px(node.corners)),
            ..default()
        }
    }
}

fn align_items(align: UiAlign) -> AlignItems {
    match align {
        UiAlign::Stretch => AlignItems::Stretch,
        UiAlign::Start => AlignItems::FlexStart,
        UiAlign::Center => AlignItems::Center,
        UiAlign::End => AlignItems::FlexEnd,
    }
}

/// A node filling its parent (placed over it).
fn fill_parent() -> Node {
    Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), ..default() }
}

fn val_of(size: &UiSize) -> Val {
    match size.parse() {
        Ok(UiLength::Pixels(pixels)) => px(pixels),
        Ok(UiLength::Percent(share)) => percent(share),
        Ok(UiLength::Auto) | Err(_) => Val::Auto,
    }
}

/// A colour: a named one from the styles, or numbers.
fn colour_of(colour: &UiColour, styles: &UiStyles) -> Color {
    match colour {
        UiColour::Named(name) => match styles.colours.get(name) {
            Some(UiColour::Named(_)) | None => Color::WHITE,
            Some(named) => colour_of(named, styles),
        },
        UiColour::Rgb(r, g, b) => Color::srgb(*r, *g, *b),
        UiColour::Rgba(r, g, b, a) => Color::srgba(*r, *g, *b, *a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_fill_to_their_data() {
        let mut data = UiData::default();
        data.0.insert("health".to_string(), "3".to_string());
        data.0.insert("health_max".to_string(), "12".to_string());
        assert_eq!(bar_fraction(&data, "health"), 0.25);
        assert_eq!(bar_fraction(&data, "mana"), 0.0, "no data, an empty bar");
    }

    #[test]
    fn built_in_layouts_and_styles_read() {
        for (name, text) in BUILT_IN_LAYOUTS {
            assert!(parse_layout(text).is_ok(), "{name}");
        }
        let styles = parse_styles(BUILT_IN_STYLES).unwrap();
        assert_eq!(colour_of(&UiColour::Named("white".to_string()), &styles), Color::srgb(1.0, 1.0, 1.0));
    }
}
