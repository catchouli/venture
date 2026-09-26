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
//!
//! How the bar looks is its layout (`ui/layouts/hotbar.ron`): this copies its
//! slot template for each slot, and fills the slots in.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use venture_protocol::{ClientMessage, HOTBAR_SLOTS, HotbarEntry, ServerMessage};

use crate::content::ContentStore;
use crate::interaction::abilities::{AbilityCooldowns, UseAbility};
use crate::interaction::actions::{Action, PerformAction};
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::layout::{LayoutBuilt, LayoutSystems, Ui, UiRoot};
use crate::ui::text_input::KeyboardFocus;
use crate::ui::tooltip::Tooltip;
use crate::ui::vitals::Vitals;

/// The hotbar's layout (`ui/layouts/hotbar.ron`).
const LAYOUT: &str = "hotbar";
/// Size of the icon that follows the mouse while dragging, in pixels.
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
/// Tint of an ability there isn't enough mana for.
const NO_MANA_TINT: Color = Color::srgb(0.45, 0.5, 0.75);

/// Shows the hotbar.
pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HotbarLayout>()
            .init_resource::<Drag>()
            .add_systems(Startup, spawn_ghost)
            .add_systems(
                Update,
                (
                    fill_in_layout,
                    record_layout,
                    show_bar,
                    show_slots,
                    show_cooldowns,
                    use_keys,
                    start_dragging,
                    drag_and_drop,
                )
                    .chain()
                    .after(LayoutSystems),
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

/// Makes the icon that follows the mouse while dragging (hidden until then).
fn spawn_ghost(mut commands: Commands) {
    commands.spawn((
        DragGhost,
        ImageNode::default(),
        Node { position_type: PositionType::Absolute, width: px(ICON_SIZE), height: px(ICON_SIZE), ..default() },
        GlobalZIndex(20),
        Visibility::Hidden,
    ));
}

/// Fills in the hotbar's layout whenever it's built: a copy of its slot
/// template for each slot, with its key's number, and the parts the other
/// systems fill in marked.
fn fill_in_layout(mut built: MessageReader<LayoutBuilt>, mut ui: Ui) {
    for _ in built.read().filter(|built| built.layout == LAYOUT) {
        for (index, copy) in ui.copy(LAYOUT, "slot", HOTBAR_SLOTS).into_iter().enumerate() {
            let commands = ui.commands();
            commands.entity(copy.root).insert((Slot(index), Tooltip::default(), RelativeCursorPosition::default()));
            let part = |id: &str| copy.ids.get(id).copied();
            if let Some(icon) = part("icon") {
                commands.entity(icon).insert((SlotIcon(index), Visibility::Hidden));
            }
            if let Some(label) = part("label") {
                commands.entity(label).insert(SlotLabel(index));
            }
            if let Some(shade) = part("cooldown_shade") {
                commands.entity(shade).insert(CooldownShade(index));
            }
            if let Some(text) = part("cooldown_text") {
                commands.entity(text).insert(CooldownText(index));
            }
            if let Some(key) = part("key") {
                commands.entity(key).insert(Text::new(key_name(KEYS[index])));
            }
        }
    }
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
fn show_bar(state: Res<State<ClientState>>, mut roots: Query<(&UiRoot, &mut Visibility)>) {
    let wanted = if *state.get() == ClientState::Playing { Visibility::Inherited } else { Visibility::Hidden };
    for (root, mut visibility) in &mut roots {
        if root.0 == LAYOUT && *visibility != wanted {
            *visibility = wanted;
        }
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
    rebuilt: Query<(), Added<SlotIcon>>,
) {
    if !layout.is_changed() && !content.is_changed() && rebuilt.is_empty() {
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
