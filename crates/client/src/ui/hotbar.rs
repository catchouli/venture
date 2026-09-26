//! The hotbar, at the bottom of the screen: ten slots for abilities and actions,
//! used with keys 1 to 9 and 0 or by clicking. Abilities show their cooldown
//! (a shade that shrinks, and the seconds left) and fade when there isn't
//! enough mana for them.
//!
//! Anything marked [`Draggable`] (abilities and actions in the character
//! window, and the hotbar's own slots) can be dragged onto a slot. Dragging a
//! slot onto another swaps them; dragging it off the bar clears it. The
//! server keeps the layout ([`ServerMessage::Hotbar`] when joining) and is
//! sent every change ([`ClientMessage::SetHotbar`]).
//!
//! A press and release without moving is a click: it uses the ability or does
//! the action, wherever it was clicked.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use venture_protocol::{ClientMessage, HOTBAR_SLOTS, HotbarEntry, ServerMessage};

use crate::content::ContentStore;
use crate::interaction::abilities::{AbilityCooldowns, UseAbility};
use crate::interaction::actions::{Action, PerformAction};
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::character_window::{BOX_COLOUR, HoverBox};
use crate::ui::text_input::KeyboardFocus;
use crate::ui::theme::{Piece, Themed};
use crate::ui::tooltip::Tooltip;
use crate::ui::vitals::Vitals;
use crate::ui::widgets::{BORDER_COLOUR, WINDOW_BACKGROUND, WINDOW_LAYER};

/// Size of a slot, in pixels.
const SLOT_SIZE: f32 = 44.0;
/// Size of an icon in its slot, in pixels.
const ICON_SIZE: f32 = 36.0;
/// How far the mouse must move while held before it's a drag, in pixels.
const DRAG_THRESHOLD: f32 = 5.0;
/// The keys for the slots, in order.
const KEYS: [KeyCode; HOTBAR_SLOTS] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];
/// Colour of the shade over an ability cooling down.
const COOLDOWN_SHADE: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
/// Tint of an ability there isn't enough mana for.
const NO_MANA_TINT: Color = Color::srgb(0.45, 0.5, 0.75);
/// Colour of the key numbers.
const KEY_COLOUR: Color = Color::srgb(0.9, 0.85, 0.7);

/// Shows the hotbar.
pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HotbarLayout>()
            .init_resource::<Drag>()
            .add_systems(Startup, spawn_bar)
            .add_systems(
                Update,
                (record_layout, show_bar, show_slots, show_cooldowns, use_keys, start_dragging, drag_and_drop).chain(),
            )
            .add_systems(OnExit(ClientState::Playing), |mut layout: ResMut<HotbarLayout>| {
                *layout = HotbarLayout::default();
            });
    }
}

/// What's in each slot, as the server last said (and changed since).
#[derive(Resource)]
pub struct HotbarLayout(Vec<Option<HotbarEntry>>);

impl Default for HotbarLayout {
    fn default() -> Self {
        Self(vec![None; HOTBAR_SLOTS])
    }
}

/// Something that can be dragged onto the hotbar (and clicked to use it).
#[derive(Component, Clone)]
pub struct Draggable(pub HotbarEntry);

/// The bar.
#[derive(Component)]
struct Bar;

/// A slot, by position (0 for key 1).
#[derive(Component)]
struct Slot(usize);

/// A slot's icon.
#[derive(Component)]
struct SlotIcon(usize);

/// A slot's label, for entries without an icon.
#[derive(Component)]
struct SlotLabel(usize);

/// The shade over a slot cooling down.
#[derive(Component)]
struct CooldownShade(usize);

/// The seconds left on a slot's cooldown.
#[derive(Component)]
struct CooldownText(usize);

/// The icon following the mouse while dragging.
#[derive(Component)]
struct DragGhost;

/// What the mouse is holding, if anything.
#[derive(Resource, Default)]
struct Drag(Option<Held>);

struct Held {
    entry: HotbarEntry,
    /// The slot it came from, if it came from the bar.
    from_slot: Option<usize>,
    /// Where the mouse was pressed.
    start: Vec2,
    /// Whether it has moved far enough to be a drag (rather than a click).
    dragging: bool,
}

fn spawn_bar(mut commands: Commands) {
    commands
        .spawn((
            Bar,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(10),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Above names and speech bubbles, like the windows.
            WINDOW_LAYER,
            Visibility::Hidden,
        ))
        .with_children(|centre| {
            centre
                .spawn((
                    Node {
                        column_gap: px(4),
                        padding: UiRect::all(px(7)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(WINDOW_BACKGROUND),
                    BorderColor::all(BORDER_COLOUR),
                    Themed(Piece::Panel),
                ))
                .with_children(|bar| {
                    for (index, key) in KEYS.iter().enumerate() {
                        spawn_slot(bar, index, key_name(*key));
                    }
                });
        });
    commands.spawn((
        DragGhost,
        ImageNode::default(),
        Node { position_type: PositionType::Absolute, width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
        GlobalZIndex(20),
        Visibility::Hidden,
    ));
}

fn spawn_slot(bar: &mut ChildSpawnerCommands, index: usize, key: &str) {
    let fill = Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() };
    bar.spawn((
        Slot(index),
        Button,
        HoverBox,
        Tooltip::default(),
        RelativeCursorPosition::default(),
        Node {
            width: px(SLOT_SIZE),
            height: px(SLOT_SIZE),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(3)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(BOX_COLOUR),
        Themed(Piece::Slot),
    ))
    .with_children(|slot| {
        slot.spawn((
            SlotIcon(index),
            ImageNode::default(),
            Node { width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
            Visibility::Hidden,
        ));
        slot.spawn((
            SlotLabel(index),
            Text::default(),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::WHITE),
            Node { position_type: PositionType::Absolute, ..default() },
        ));
        // The shade grows up from the bottom, so it shrinks downwards as the ability gets ready.
        slot.spawn((
            CooldownShade(index),
            Node { bottom: px(0), height: percent(0), ..fill.clone() },
            BackgroundColor(COOLDOWN_SHADE),
        ));
        slot.spawn(Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..fill })
            .with_child((
                CooldownText(index),
                Text::default(),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                TextColor(Color::WHITE),
                TextShadow { offset: Vec2::splat(1.0), ..default() },
            ));
        slot.spawn((
            Text::new(key),
            TextFont { font_size: FontSize::Px(10.0), ..default() },
            TextColor(KEY_COLOUR),
            TextShadow { offset: Vec2::splat(1.0), ..default() },
            Node { position_type: PositionType::Absolute, left: px(3), top: px(1), ..default() },
        ));
    });
}

/// The number shown on a slot for its key.
fn key_name(key: KeyCode) -> &'static str {
    match key {
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        _ => "0",
    }
}

fn record_layout(mut events: MessageReader<FromServer>, mut layout: ResMut<HotbarLayout>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::Hotbar { slots } = message {
            let mut slots = slots.clone();
            slots.resize(HOTBAR_SLOTS, None);
            layout.0 = slots;
        }
    }
}

/// Shows the bar only while playing.
fn show_bar(state: Res<State<ClientState>>, mut bar: Single<&mut Visibility, With<Bar>>) {
    let wanted = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
    if **bar != wanted {
        **bar = wanted;
    }
}

/// An entry's icon, if it has one, and its name and description.
fn describe(entry: &HotbarEntry, content: &ContentStore) -> (Option<Handle<Image>>, String, String) {
    match entry {
        HotbarEntry::Ability(ability) => match content.abilities.get(ability) {
            Some(def) => {
                let mut body = def.description.clone();
                if def.mana > 0 {
                    body.push_str(&format!("\nMana {}.", def.mana));
                }
                (content.icons.get(&def.icon).cloned(), def.name.clone(), body)
            }
            None => (None, ability.clone(), String::new()),
        },
        HotbarEntry::Action(action) => {
            let action = Action::from_hotbar(*action);
            let icon = action.icon().and_then(|icon| content.icons.get(icon)).cloned();
            (icon, action.title(), format!("/{}", action.name()))
        }
    }
}

/// Puts each slot's icon (or name) and tooltip in, when the layout or content changes.
fn show_slots(
    layout: Res<HotbarLayout>,
    content: Res<ContentStore>,
    mut slots: Query<(&Slot, &mut Tooltip)>,
    mut icons: Query<(&SlotIcon, &mut ImageNode, &mut Visibility)>,
    mut labels: Query<(&SlotLabel, &mut Text)>,
) {
    if !layout.is_changed() && !content.is_changed() {
        return;
    }
    let described: Vec<_> =
        layout.0.iter().map(|entry| entry.as_ref().map(|entry| describe(entry, &content))).collect();
    for (Slot(index), mut tooltip) in &mut slots {
        *tooltip = match &described[*index] {
            Some((_, title, body)) => Tooltip { title: title.clone(), body: body.clone() },
            None => Tooltip::default(),
        };
    }
    for (SlotIcon(index), mut image, mut visibility) in &mut icons {
        let icon = described[*index].as_ref().and_then(|(icon, ..)| icon.clone());
        *visibility = if icon.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        image.image = icon.unwrap_or_default();
    }
    for (SlotLabel(index), mut text) in &mut labels {
        // A name only for entries without an icon.
        text.0 = match &described[*index] {
            Some((None, title, _)) => title.clone(),
            _ => String::new(),
        };
    }
}

/// Shows cooldowns, and fades abilities there isn't enough mana for.
fn show_cooldowns(
    time: Res<Time>,
    layout: Res<HotbarLayout>,
    content: Res<ContentStore>,
    cooldowns: Res<AbilityCooldowns>,
    vitals: Res<Vitals>,
    mut shades: Query<(&CooldownShade, &mut Node)>,
    mut texts: Query<(&CooldownText, &mut Text)>,
    mut icons: Query<(&SlotIcon, &mut ImageNode)>,
) {
    let now = time.elapsed_secs();
    let ability_in = |index: usize| match &layout.0[index] {
        Some(HotbarEntry::Ability(ability)) => Some(ability.as_str()),
        _ => None,
    };
    let remaining = |index: usize| ability_in(index).and_then(|ability| cooldowns.remaining(ability, now));
    for (CooldownShade(index), mut node) in &mut shades {
        node.height = percent(remaining(*index).map_or(0.0, |(_, fraction)| fraction * 100.0));
    }
    for (CooldownText(index), mut text) in &mut texts {
        let wanted = remaining(*index).map(|(left, _)| format!("{}", left.ceil())).unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let mana = vitals.0.map(|vitals| vitals.mana);
    for (SlotIcon(index), mut image) in &mut icons {
        let cost = ability_in(*index).and_then(|ability| content.abilities.get(ability)).map_or(0, |def| def.mana);
        let short = mana.is_some_and(|mana| mana < cost);
        image.color = if short { NO_MANA_TINT } else { Color::WHITE };
    }
}

/// Keys 1 to 0 use their slots.
fn use_keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    state: Res<State<ClientState>>,
    layout: Res<HotbarLayout>,
    mut abilities: MessageWriter<UseAbility>,
    mut actions: MessageWriter<PerformAction>,
) {
    if focus.is_typing() || *state.get() != ClientState::Playing {
        return;
    }
    for (index, key) in KEYS.iter().enumerate() {
        if keys.just_pressed(*key)
            && let Some(entry) = &layout.0[index]
        {
            activate(entry, &mut abilities, &mut actions);
        }
    }
}

/// Uses an ability or does an action.
fn activate(
    entry: &HotbarEntry,
    abilities: &mut MessageWriter<UseAbility>,
    actions: &mut MessageWriter<PerformAction>,
) {
    match entry {
        HotbarEntry::Ability(ability) => {
            abilities.write(UseAbility(ability.clone()));
        }
        HotbarEntry::Action(action) => {
            actions.write(PerformAction(Action::from_hotbar(*action)));
        }
    }
}

/// Picks something up when the mouse is pressed on it (it's only a drag
/// once the mouse moves).
fn start_dragging(
    window: Single<&Window>,
    layout: Res<HotbarLayout>,
    mut drag: ResMut<Drag>,
    draggables: Query<(&Interaction, &Draggable), Changed<Interaction>>,
    slots: Query<(&Interaction, &Slot), Changed<Interaction>>,
) {
    let Some(start) = window.cursor_position() else { return };
    for (interaction, Draggable(entry)) in &draggables {
        if *interaction == Interaction::Pressed {
            drag.0 = Some(Held { entry: entry.clone(), from_slot: None, start, dragging: false });
        }
    }
    for (interaction, Slot(index)) in &slots {
        if *interaction == Interaction::Pressed
            && let Some(entry) = &layout.0[*index]
        {
            drag.0 = Some(Held { entry: entry.clone(), from_slot: Some(*index), start, dragging: false });
        }
    }
}

/// Moves what's held with the mouse, and drops it (or clicks it) on release.
fn drag_and_drop(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    content: Res<ContentStore>,
    mut drag: ResMut<Drag>,
    mut layout: ResMut<HotbarLayout>,
    slots: Query<(&Slot, &RelativeCursorPosition)>,
    ghost: Single<(&mut Node, &mut ImageNode, &mut Visibility), With<DragGhost>>,
    mut to_server: MessageWriter<ToServer>,
    mut abilities: MessageWriter<UseAbility>,
    mut actions: MessageWriter<PerformAction>,
) {
    let (mut ghost_node, mut ghost_image, mut ghost_visibility) = ghost.into_inner();
    let Some(held) = drag.0.as_mut() else {
        *ghost_visibility = Visibility::Hidden;
        return;
    };
    let cursor = window.cursor_position();
    if let Some(cursor) = cursor {
        if !held.dragging && cursor.distance(held.start) > DRAG_THRESHOLD {
            held.dragging = true;
            ghost_image.image = describe(&held.entry, &content).0.unwrap_or_default();
        }
        if held.dragging {
            ghost_node.left = px(cursor.x - ICON_SIZE / 2.0);
            ghost_node.top = px(cursor.y - ICON_SIZE / 2.0);
            *ghost_visibility = Visibility::Inherited;
        }
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(held) = drag.0.take() else { return };
    *ghost_visibility = Visibility::Hidden;
    if !held.dragging {
        activate(&held.entry, &mut abilities, &mut actions);
        return;
    }
    let over = slots.iter().find(|(_, position)| position.cursor_over()).map(|(Slot(index), _)| *index);
    match (held.from_slot, over) {
        (Some(from), Some(to)) => layout.0.swap(from, to),
        (None, Some(to)) => {
            // Only one of each on the bar: moving it here takes it off wherever it was.
            for slot in layout.0.iter_mut().filter(|slot| slot.as_ref() == Some(&held.entry)) {
                *slot = None;
            }
            layout.0[to] = Some(held.entry);
        }
        (Some(from), None) => layout.0[from] = None,
        (None, None) => return,
    }
    to_server.write(ToServer(ClientMessage::SetHotbar { slots: layout.0.clone() }));
}
