//! Interface layouts: each window (or other piece of the interface) is a tree
//! of elements in `ui/layouts/<name>.ron`, with named text styles, colours
//! and bar colours in `ui/styles.ron`. See `docs/ui-layouts.md`.
//!
//! Layouts say where things go and how they look; the game's code says what
//! they show and do. The two meet in two ways:
//! - **Data:** the game publishes named values (see [`UI_DATA`]), which text
//!   (`"{health} / {health_max}"`) and bars (`bar: "health"`) show.
//! - **Element ids:** the game finds elements by `id` to give them behaviour
//!   (clicks, lists, icons). [`LAYOUTS`] lists the ids each layout must have.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The layouts the game knows, and the element ids it looks for in each
/// (the content check reports any that are missing).
pub const LAYOUTS: [(&str, &[&str]); 4] = [
    ("player_frame", &["player_frame"]),
    ("hotbar", &["hotbar", "slots", "slot", "icon", "label", "cooldown_shade", "cooldown_text", "key"]),
    ("party_frame", &["party_frame", "member"]),
    ("party_invite", &["party_invite", "invite_accept", "invite_decline"]),
];

/// The data the game publishes for layouts to show, with what each is.
///
/// The `member_` data is only shown inside the party frame's `member`
/// template: each copy shows its own member's.
pub const UI_DATA: [(&str, &str); 20] = [
    ("name", "the player's character's name"),
    ("level", "the player's level"),
    ("health", "the player's health"),
    ("health_max", "the player's most health"),
    ("mana", "the player's mana"),
    ("mana_max", "the player's most mana"),
    ("experience", "the player's experience towards the next level"),
    ("experience_max", "the experience the next level takes"),
    ("invite_from", "who invited the player to a party"),
    ("invite_party", "the name of the party the player's invited to"),
    ("invite_seconds", "whole seconds left to answer the party invitation"),
    ("invite_time", "time left to answer the party invitation (for a bar)"),
    ("invite_time_max", "the time the party invitation was open for"),
    ("member_name", "a party member's name"),
    ("member_class", "a party member's class"),
    ("member_level", "a party member's level"),
    ("member_health", "a party member's health"),
    ("member_health_max", "a party member's most health"),
    ("member_mana", "a party member's mana"),
    ("member_mana_max", "a party member's most mana"),
];

/// One element of a layout: a box, placed and sized like a CSS flexbox
/// element, with an optional look (a theme piece, a picture, a colour), text,
/// bar, and children inside it. Every field is optional.
///
/// ```ron
/// (id: "player_frame", position: Absolute, left: 10, top: 10, width: 220,
///  direction: Column, gap: 5, padding: (7, 10), piece: "panel",
///  children: [(text: "{name}  Level {level}", style: "frame_name"), (bar: "health", height: 14)])
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiNode {
    /// A name the game uses to find this element (see [`LAYOUTS`]).
    pub id: Option<String>,
    /// Whether it's placed in the flow of its parent (`Relative`) or at a
    /// fixed place within it (`Absolute`, with `left`/`right`/`top`/`bottom`).
    pub position: UiPosition,
    /// Distance from the parent's left edge.
    pub left: Option<UiSize>,
    /// Distance from the parent's right edge.
    pub right: Option<UiSize>,
    /// Distance from the parent's top edge.
    pub top: Option<UiSize>,
    /// Distance from the parent's bottom edge.
    pub bottom: Option<UiSize>,
    /// Width (pixels, or `"50%"` of the parent).
    pub width: Option<UiSize>,
    /// Height.
    pub height: Option<UiSize>,
    /// Smallest width.
    pub min_width: Option<UiSize>,
    /// Smallest height.
    pub min_height: Option<UiSize>,
    /// Largest width.
    pub max_width: Option<UiSize>,
    /// Largest height.
    pub max_height: Option<UiSize>,
    /// Space outside it.
    pub margin: UiEdges,
    /// Space between its edge and its children.
    pub padding: UiEdges,
    /// Width of its plain border (drawn only without a theme picture).
    pub border: f32,
    /// Which way its children go: `Row` (left to right) or `Column` (top to bottom).
    pub direction: UiDirection,
    /// Space between its children.
    pub gap: f32,
    /// How its children line up across `direction`.
    pub align: UiAlign,
    /// How its children spread along `direction`.
    pub justify: UiJustify,
    /// Whether its children wrap onto more rows (or columns).
    pub wrap: bool,
    /// How much of the spare room along its parent's direction it takes.
    pub grow: f32,
    /// How it lines up in its parent, if not as the parent says.
    pub align_self: Option<UiAlign>,
    /// Rounded corners (pixels), for the plain look.
    pub corners: f32,
    /// A theme piece to draw it with (such as `"panel"`: see `ui/theme.ron`).
    pub piece: Option<String>,
    /// A picture to draw it with: a PNG in `ui/`, stretched to fit.
    pub image: Option<String>,
    /// Whether the game puts a picture here (an item or ability icon).
    pub icon: bool,
    /// Its background colour (a colour from `ui/styles.ron`, or `(r, g, b)`
    /// or `(r, g, b, a)`), for the plain look.
    pub colour: Option<UiColour>,
    /// Its plain border's colour.
    pub border_colour: Option<UiColour>,
    /// Text to show. `{name}` shows the game's data called `name` (see [`UI_DATA`]).
    pub text: Option<String>,
    /// The text's style, from `ui/styles.ron`.
    pub style: Option<String>,
    /// Makes it a bar showing the data called this (`"health"` shows `health`
    /// out of `health_max`), in the bar colours of the same name from
    /// `ui/styles.ron` (or `bar_style`'s). Its children are drawn over it.
    pub bar: Option<String>,
    /// Bar colours to use instead of the ones named after `bar`.
    pub bar_style: Option<String>,
    /// Whether it can be clicked (and so lights up under the mouse).
    pub button: bool,
    /// A tooltip for it (the first line is the title). Like `text`, it can
    /// show the game's data: `{name}`.
    pub tooltip: Option<String>,
    /// Draws it above other interface (such as names over characters'
    /// heads): higher is on top. Windows use 5.
    pub layer: Option<i32>,
    /// Whether it starts hidden (the game shows it when it's needed).
    pub hidden: bool,
    /// Whether it's a template the game copies (for lists and rows of
    /// slots), rather than an element shown as it is.
    pub template: bool,
    /// The elements inside it.
    pub children: Vec<UiNode>,
}

/// How an element is placed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiPosition {
    /// In the flow of its parent's children.
    #[default]
    Relative,
    /// At a fixed place within its parent.
    Absolute,
}

/// Which way an element's children go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiDirection {
    /// Left to right.
    #[default]
    Row,
    /// Top to bottom.
    Column,
}

/// How children line up across their parent's direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiAlign {
    /// Stretched across.
    #[default]
    Stretch,
    /// At the start (left, or top).
    Start,
    /// In the middle.
    Center,
    /// At the end (right, or bottom).
    End,
}

/// How children spread along their parent's direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiJustify {
    /// Packed at the start.
    #[default]
    Start,
    /// Packed in the middle.
    Center,
    /// Packed at the end.
    End,
    /// Spread out, first and last at the edges.
    SpaceBetween,
    /// Spread out, with equal room around each.
    SpaceAround,
}

/// A size or distance: pixels (`10`), a share of the parent (`"50%"`), or
/// `"auto"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UiSize {
    /// Pixels.
    Pixels(f32),
    /// `"50%"` or `"auto"`.
    Text(String),
}

impl UiSize {
    /// What it means, if it's written correctly.
    pub fn parse(&self) -> Result<UiLength, String> {
        match self {
            UiSize::Pixels(pixels) if pixels.is_finite() => Ok(UiLength::Pixels(*pixels)),
            UiSize::Pixels(_) => Err("sizes must be real numbers".to_string()),
            UiSize::Text(text) if text == "auto" => Ok(UiLength::Auto),
            UiSize::Text(text) => text
                .strip_suffix('%')
                .and_then(|number| number.trim().parse::<f32>().ok())
                .filter(|number| number.is_finite())
                .map(UiLength::Percent)
                .ok_or_else(|| format!("\"{text}\" isn't a size (use a number of pixels, \"50%\" or \"auto\")")),
        }
    }
}

/// A size, understood.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiLength {
    /// Pixels.
    Pixels(f32),
    /// A percentage of the parent.
    Percent(f32),
    /// Whatever the layout works out.
    Auto,
}

/// Space around the four edges: one number for all, `(vertical, horizontal)`,
/// or `(top, right, bottom, left)` as in CSS.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UiEdges {
    /// The same all round.
    All(f32),
    /// Top and bottom, then left and right.
    Two(f32, f32),
    /// Top, right, bottom, left.
    Four(f32, f32, f32, f32),
}

impl Default for UiEdges {
    fn default() -> Self {
        UiEdges::All(0.0)
    }
}

impl UiEdges {
    /// Top, right, bottom, left.
    pub fn sides(self) -> [f32; 4] {
        match self {
            UiEdges::All(all) => [all; 4],
            UiEdges::Two(vertical, horizontal) => [vertical, horizontal, vertical, horizontal],
            UiEdges::Four(top, right, bottom, left) => [top, right, bottom, left],
        }
    }
}

/// A colour: a name from `ui/styles.ron`, or sRGB numbers from 0 to 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UiColour {
    /// A named colour.
    Named(String),
    /// Red, green, blue.
    Rgb(f32, f32, f32),
    /// Red, green, blue, and how opaque.
    Rgba(f32, f32, f32, f32),
}

/// The interface's named colours, text styles and bar colours
/// (`ui/styles.ron`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiStyles {
    /// Colours, by name.
    pub colours: BTreeMap<String, UiColour>,
    /// Text styles, by name.
    pub text: BTreeMap<String, TextStyle>,
    /// Bar colours, by name.
    pub bars: BTreeMap<String, BarStyle>,
}

/// How text looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextStyle {
    /// Size, in pixels.
    pub size: f32,
    /// Colour.
    pub colour: Option<UiColour>,
    /// A font in `ui/` (TTF or OTF), if not the theme's.
    pub font: Option<String>,
    /// Whether it has a dark shadow (easier to read over busy backgrounds).
    pub shadow: bool,
    /// How lines line up: `Left`, `Center` or `Right`.
    pub align: UiTextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { size: 14.0, colour: None, font: None, shadow: false, align: UiTextAlign::Left }
    }
}

/// How lines of text line up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiTextAlign {
    /// Along the left.
    #[default]
    Left,
    /// In the middle.
    Center,
    /// Along the right.
    Right,
}

/// A bar's colours: its fill, and the (usually darker) empty part behind it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarStyle {
    /// The full part.
    pub fill: UiColour,
    /// The empty part.
    pub back: UiColour,
}

/// Reads a layout file: its root element. (Optional values are written
/// plainly, without `Some(...)`.)
pub fn parse_layout(text: &str) -> Result<UiNode, String> {
    lenient().from_str(text).map_err(|error| error.to_string())
}

/// Reads the styles file.
pub fn parse_styles(text: &str) -> Result<UiStyles, String> {
    lenient().from_str(text).map_err(|error| error.to_string())
}

/// RON that doesn't need `Some(...)` around optional values.
fn lenient() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// The `{names}` of data a text asks for.
pub fn data_names(text: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let Some(length) = rest[start..].find('}') else { break };
        names.push(&rest[start + 1..start + length]);
        rest = &rest[start + length + 1..];
    }
    names
}

/// Fills in a text's `{names}` with the data (unknown names are left as they are).
pub fn fill_in(text: &str, data: impl Fn(&str) -> Option<String>) -> String {
    let mut filled = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let Some(length) = rest[start..].find('}') else { break };
        filled.push_str(&rest[..start]);
        let name = &rest[start + 1..start + length];
        match data(name) {
            Some(value) => filled.push_str(&value),
            None => filled.push_str(&rest[start..start + length + 1]),
        }
        rest = &rest[start + length + 1..];
    }
    filled.push_str(rest);
    filled
}

/// Every problem with a layout: unknown styles, pieces, pictures, fonts and
/// data, bad sizes, and (for layouts the game knows) missing element ids.
/// `pieces`, `files` and `styles` are what the theme and `ui/` have.
pub fn check_layout(name: &str, root: &UiNode, styles: &UiStyles, pieces: &[&str], files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut ids = Vec::new();
    check_node(root, styles, pieces, files, &mut ids, &mut problems);
    if let Some((_, needed)) = LAYOUTS.iter().find(|(known, _)| *known == name) {
        for id in needed.iter().filter(|id| !ids.contains(id)) {
            problems.push(format!("there's no element with id \"{id}\", which the game needs"));
        }
    }
    problems
}

fn check_node<'a>(
    node: &'a UiNode,
    styles: &UiStyles,
    pieces: &[&str],
    files: &[String],
    ids: &mut Vec<&'a str>,
    problems: &mut Vec<String>,
) {
    let place = node.id.as_deref().map(|id| format!("element \"{id}\": ")).unwrap_or_default();
    let mut problem = |message: String| problems.push(format!("{place}{message}"));
    if let Some(id) = &node.id {
        ids.push(id);
    }
    let sizes = [
        &node.left,
        &node.right,
        &node.top,
        &node.bottom,
        &node.width,
        &node.height,
        &node.min_width,
        &node.min_height,
        &node.max_width,
        &node.max_height,
    ];
    for size in sizes.into_iter().flatten() {
        if let Err(message) = size.parse() {
            problem(message);
        }
    }
    if let Some(piece) = node.piece.as_ref().filter(|piece| !pieces.contains(&piece.as_str())) {
        problem(format!("there's no theme piece called \"{piece}\""));
    }
    if let Some(image) = node.image.as_ref().filter(|image| !files.contains(image)) {
        problem(format!("there's no picture called \"{image}\" in ui/"));
    }
    for colour in [&node.colour, &node.border_colour].into_iter().flatten() {
        if let Some(message) = colour_problem(colour, styles) {
            problem(message);
        }
    }
    if let Some(style) = node.style.as_ref().filter(|style| !styles.text.contains_key(*style)) {
        problem(format!("there's no text style called \"{style}\" in styles.ron"));
    }
    for text in [&node.text, &node.tooltip].into_iter().flatten() {
        for name in data_names(text).into_iter().filter(|name| !UI_DATA.iter().any(|(known, _)| known == name)) {
            problem(format!("the game has no data called \"{name}\" (see docs/ui-layouts.md)"));
        }
    }
    if let Some(bar) = &node.bar {
        let max = format!("{bar}_max");
        if !UI_DATA.iter().any(|(known, _)| *known == bar) || !UI_DATA.iter().any(|(known, _)| *known == max) {
            problem(format!("the game has no data called \"{bar}\" (and \"{max}\") to show in a bar"));
        }
        let style = node.bar_style.as_ref().unwrap_or(bar);
        if !styles.bars.contains_key(style) {
            problem(format!("there are no bar colours called \"{style}\" in styles.ron"));
        }
    }
    for child in &node.children {
        check_node(child, styles, pieces, files, ids, problems);
    }
}

/// What's wrong with a colour, if anything.
fn colour_problem(colour: &UiColour, styles: &UiStyles) -> Option<String> {
    match colour {
        UiColour::Named(name) if !styles.colours.contains_key(name) => {
            Some(format!("there's no colour called \"{name}\" in styles.ron"))
        }
        UiColour::Rgb(r, g, b) if ![r, g, b].iter().all(|c| (0.0..=1.0).contains(*c)) => {
            Some("colours are numbers from 0 to 1".to_string())
        }
        UiColour::Rgba(r, g, b, a) if ![r, g, b, a].iter().all(|c| (0.0..=1.0).contains(*c)) => {
            Some("colours are numbers from 0 to 1".to_string())
        }
        _ => None,
    }
}

/// Every problem with the styles: colours that are wrong or name missing
/// colours, and fonts that aren't in `ui/`.
pub fn check_styles(styles: &UiStyles, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, colour) in &styles.colours {
        if matches!(colour, UiColour::Named(_)) {
            problems.push(format!("colour \"{name}\": give it numbers, not another colour's name"));
        } else if let Some(message) = colour_problem(colour, styles) {
            problems.push(format!("colour \"{name}\": {message}"));
        }
    }
    for (name, style) in &styles.text {
        if !(style.size > 0.0 && style.size.is_finite()) {
            problems.push(format!("text style \"{name}\": its size must be more than 0"));
        }
        if let Some(message) = style.colour.as_ref().and_then(|colour| colour_problem(colour, styles)) {
            problems.push(format!("text style \"{name}\": {message}"));
        }
        if let Some(font) = style.font.as_ref().filter(|font| !files.contains(*font)) {
            problems.push(format!("text style \"{name}\": there's no font called \"{font}\" in ui/"));
        }
    }
    for (name, bar) in &styles.bars {
        for colour in [&bar.fill, &bar.back] {
            if let Some(message) = colour_problem(colour, styles) {
                problems.push(format!("bar \"{name}\": {message}"));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_pixels_percentages_or_auto() {
        assert_eq!(UiSize::Pixels(10.0).parse(), Ok(UiLength::Pixels(10.0)));
        assert_eq!(UiSize::Text("50%".to_string()).parse(), Ok(UiLength::Percent(50.0)));
        assert_eq!(UiSize::Text("auto".to_string()).parse(), Ok(UiLength::Auto));
        assert!(UiSize::Text("wide".to_string()).parse().is_err());
    }

    #[test]
    fn edges_follow_css() {
        assert_eq!(UiEdges::Two(7.0, 10.0).sides(), [7.0, 10.0, 7.0, 10.0]);
        assert_eq!(UiEdges::All(3.0).sides(), [3.0; 4]);
    }

    #[test]
    fn text_fills_in_data() {
        let data = |name: &str| (name == "name").then(|| "Aldric".to_string());
        assert_eq!(fill_in("{name}  Level {level}", data), "Aldric  Level {level}");
        assert_eq!(data_names("{health} / {health_max}"), ["health", "health_max"]);
    }

    #[test]
    fn layouts_read_like_css_and_are_checked() {
        let layout = r#"(id: "player_frame", position: Absolute, left: 10, width: "50%", padding: (7, 10),
            piece: "panel", children: [(text: "{name} {nonsense}", style: "missing"), (bar: "health")])"#;
        let root = parse_layout(layout).unwrap();
        assert_eq!(root.children.len(), 2);
        let mut styles = UiStyles::default();
        styles.bars.insert(
            "health".to_string(),
            BarStyle { fill: UiColour::Rgb(1.0, 0.0, 0.0), back: UiColour::Rgb(0.2, 0.0, 0.0) },
        );
        let problems = check_layout("player_frame", &root, &styles, &["panel"], &[]);
        assert_eq!(
            problems,
            [
                "there's no text style called \"missing\" in styles.ron",
                "the game has no data called \"nonsense\" (see docs/ui-layouts.md)",
            ]
        );
        let problems = check_layout("hotbar", &root, &styles, &["panel"], &[]);
        assert!(problems.iter().any(|problem| problem.contains("\"slots\", which the game needs")), "{problems:?}");
    }
}
