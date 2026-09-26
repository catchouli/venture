//! Making a character, the first time a player joins: choosing a name, a
//! class, a body type and their looks (height, build, head size, hair,
//! beard, and hair and skin colours, from what the content's
//! `characters/characters.ron` offers), with a turning preview of how
//! they'll look. The
//! server offers the classes ([`ServerMessage::CharacterNeeded`]) and turns
//! away names that are taken ([`ServerMessage::CharacterRejected`]).
//!
//! The preview is a character model on a little stage of its own, drawn by
//! a second camera into an image that the window shows. The stage is on its
//! own render layer, so the world's camera never sees it.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::RelativeCursorPosition;
use venture_protocol::{
    BodyType, ClassChoice, ClientMessage, FEATURE_RANGE, Features, MAX_NAME_CHARS, PlayerLook, ServerMessage,
    clean_name,
};

use crate::characters::humans::HumanLook;
use crate::characters::model::{CharacterAssets, Gait, rebuild_body, spawn_character};
use crate::content::ContentStore;
use crate::dev::settings::DevSettings;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::text_input::{FieldText, KeyboardFocus, TextField, TextInputSystems, TextSubmitted};
use crate::ui::theme::{Piece, Themed};
use crate::ui::widgets::BORDER_COLOUR;

/// Colour of the title.
const TITLE_COLOUR: Color = Color::srgb(0.95, 0.85, 0.55);
/// Colour of problems with the name.
const ERROR_COLOUR: Color = Color::srgb(1.0, 0.45, 0.4);
/// Background of an option button.
const OPTION_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);
/// Background of a chosen option's button.
const CHOSEN_COLOUR: Color = Color::srgba(0.85, 0.7, 0.4, 0.35);
/// Width of the name field and the class descriptions, in pixels.
const WIDTH: f32 = 330.0;
/// Size of the preview, in pixels.
const PREVIEW_SIZE: (u32, u32) = (240, 320);
/// The render layer the preview stage is on (the world is on layer 0).
const PREVIEW_LAYER: usize = 1;
/// Where the preview stage is: far below the world, out of everyone's way.
const STAGE: Vec3 = Vec3::new(0.0, -2000.0, 0.0);
/// How fast the preview turns, in radians per second.
const TURN_SPEED: f32 = 0.6;
/// Width of the looks' labels, and of a slider's track, in pixels.
const LABEL_WIDTH: f32 = 92.0;
const TRACK_WIDTH: f32 = 130.0;
/// Colour of a slider's track and of its filled part.
const TRACK_COLOUR: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
const FILL_COLOUR: Color = Color::srgb(0.75, 0.6, 0.32);
/// Size of a colour swatch, in pixels.
const SWATCH_SIZE: f32 = 22.0;

/// Lets new players make a character.
pub struct CharacterCreationPlugin;

impl Plugin for CharacterCreationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Offer>()
            .add_systems(PreUpdate, remember_offer.after(crate::network::NetworkSystems))
            // The stage first, so the prompt can show its picture.
            .add_systems(OnEnter(ClientState::ChoosingName), (build_stage, open_prompt).chain())
            .add_systems(OnExit(ClientState::ChoosingName), close_prompt)
            .add_systems(
                Update,
                (
                    choose_options,
                    show_choices,
                    (build_looks, choose_looks, show_looks).chain(),
                    (dress_preview, put_stage_on_its_layer, turn_preview).chain(),
                    submit.after(TextInputSystems),
                    show_rejections,
                )
                    .run_if(in_state(ClientState::ChoosingName)),
            );
    }
}

/// What the server offered (a name, and the classes), and what's chosen.
#[derive(Resource, Default)]
struct Offer {
    suggestion: String,
    classes: Vec<ClassChoice>,
    /// The chosen class's position in `classes`.
    class: usize,
    /// The chosen body type.
    body: BodyType,
    /// The chosen features.
    features: Features,
}

impl Offer {
    /// How the chosen options look.
    fn look(&self) -> PlayerLook {
        let class = self.classes.get(self.class);
        let worn = class.map(|class| class.worn.clone()).unwrap_or_default();
        let outfit = class.map(|class| class.outfit.clone()).unwrap_or_default();
        PlayerLook { body: self.body, features: self.features.clone(), worn, outfit }
    }
}

/// Everything on screen while making a character (removed afterwards).
#[derive(Component)]
struct CreationScreen;

/// The field the name is typed in.
#[derive(Component)]
struct NameField;

/// A class to choose, by its position in the offer.
#[derive(Component)]
struct ClassButton(usize);

/// A body type to choose.
#[derive(Component)]
struct BodyButton(BodyType);

/// A look that's chosen by steps: a slider, or one of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Feature {
    Height,
    Build,
    Head,
    Hair,
    Beard,
}

impl Feature {
    const SLIDERS: [(Feature, &'static str); 3] =
        [(Feature::Height, "Height"), (Feature::Build, "Build"), (Feature::Head, "Head")];
}

/// The column the looks are chosen in (filled once the content says what's offered).
#[derive(Component)]
struct LooksColumn;

/// A button stepping a look back (-1) or forward (1).
#[derive(Component)]
struct Stepper(Feature, i8);

/// A slider's track (clicked or dragged to set it), and its filled part.
#[derive(Component)]
struct Track(Feature);
#[derive(Component)]
struct TrackFill(Feature);

/// The name of the chosen hairstyle or beard.
#[derive(Component)]
struct ChoiceName(Feature);

/// A colour to choose: hair (`false`) or skin (`true`), and its place in the palette.
#[derive(Component)]
struct Swatch {
    skin: bool,
    index: u8,
}

/// The chosen class's description.
#[derive(Component)]
struct ClassDescription;

/// Where problems are shown.
#[derive(Component)]
struct ErrorText;

/// The preview character.
#[derive(Component)]
struct PreviewCharacter;

/// The preview's picture, which the camera draws into.
#[derive(Resource)]
struct PreviewImage(Handle<Image>);

fn remember_offer(mut events: MessageReader<FromServer>, mut offer: ResMut<Offer>) {
    for FromServer(message) in events.read() {
        if let ServerMessage::CharacterNeeded { suggestion, classes } = message {
            *offer = Offer { suggestion: suggestion.clone(), classes: classes.clone(), ..default() };
        }
    }
}

/// Sets up the preview stage: a camera drawing into an image, a light, and
/// the character.
fn build_stage(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    assets: Res<CharacterAssets>,
    offer: Res<Offer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // One plain sRGB format, with no second "view" format: WebGL2 (the
    // browser) doesn't support viewing a texture as another format.
    let image = Image::new_target_texture(PREVIEW_SIZE.0, PREVIEW_SIZE.1, TextureFormat::Rgba8UnormSrgb, None);
    let image = images.add(image);
    commands.insert_resource(PreviewImage(image.clone()));
    let layer = RenderLayers::layer(PREVIEW_LAYER);
    commands.spawn((
        CreationScreen,
        Camera3d::default(),
        Camera { clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.1, 0.08)), order: -1, ..default() },
        RenderTarget::Image(image.into()),
        AmbientLight { brightness: 1500.0, ..default() },
        Transform::from_translation(STAGE + Vec3::new(0.0, 1.15, 2.6))
            .looking_at(STAGE + Vec3::new(0.0, 0.92, 0.0), Vec3::Y),
        layer.clone(),
    ));
    // A key light from above one side, and a softer fill from the front.
    for (illuminance, direction) in [(12000.0, Vec3::new(-0.5, -0.7, -0.5)), (4000.0, Vec3::new(0.3, -0.2, -1.0))] {
        commands.spawn((
            CreationScreen,
            DirectionalLight { illuminance, ..default() },
            Transform::from_translation(STAGE).looking_to(direction, Vec3::Y),
            layer.clone(),
        ));
    }
    let look = assets.player_look(0, &offer.look(), &mut materials);
    // Turned a little, so the build shows.
    let transform = Transform::from_translation(STAGE).with_rotation(Quat::from_rotation_y(0.5));
    let character = spawn_character(&mut commands, &assets, transform, &look);
    commands.entity(character).insert((CreationScreen, PreviewCharacter, HumanLook::of_player(&offer.look()), layer));
}

fn open_prompt(
    mut commands: Commands,
    mut offer: ResMut<Offer>,
    dev: Res<DevSettings>,
    mut focus: ResMut<KeyboardFocus>,
    mut to_server: MessageWriter<ToServer>,
    preview: Option<Res<PreviewImage>>,
) {
    // Automated runs (such as screenshots) take the suggestion without asking,
    // as the class asked for (`VENTURE_CLASS`) or the first.
    if dev.automated && !dev.show_creation {
        let wanted = dev.class.as_deref();
        let class = offer.classes.iter().find(|class| Some(class.id.as_str()) == wanted).or(offer.classes.first());
        let class = class.map(|class| class.id.clone()).unwrap_or_default();
        let name = dev.name.clone().unwrap_or_else(|| offer.suggestion.clone());
        let (body, features) = (dev.body.unwrap_or_default(), dev.features.clone());
        to_server.write(ToServer(ClientMessage::CreateCharacter { name, class, body, features }));
        return;
    }
    // Development runs can ask for a class and body type to show.
    let wanted = dev.class.as_deref();
    offer.class = offer.classes.iter().position(|class| Some(class.id.as_str()) == wanted).unwrap_or(0);
    offer.body = dev.body.unwrap_or_default();
    offer.features = dev.features.clone();
    let title = TextFont { font_size: FontSize::Px(26.0), ..default() };
    let body = TextFont { font_size: FontSize::Px(16.0), ..default() };
    let small = TextFont { font_size: FontSize::Px(14.0), ..default() };
    let option = |label: String| {
        (
            Button,
            Node {
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(OPTION_COLOUR),
            BorderColor::all(BORDER_COLOUR),
            Themed(Piece::Button),
            children![(
                Text::new(label),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                TextColor(TITLE_COLOUR)
            )],
        )
    };
    let mut field = None;
    commands
        .spawn((
            CreationScreen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(12),
                        padding: UiRect::all(px(28)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.07, 0.05, 0.92)),
                    Themed(Piece::Window),
                ))
                .with_children(|panel| {
                    panel.spawn((Text::new("Welcome, traveller"), title, TextColor(TITLE_COLOUR)));
                    panel.spawn(Node { column_gap: px(20), ..default() }).with_children(|row| {
                        // The preview on the left...
                        if let Some(preview) = &preview {
                            row.spawn((
                                ImageNode::new(preview.0.clone()),
                                Node {
                                    width: px(PREVIEW_SIZE.0 as f32),
                                    height: px(PREVIEW_SIZE.1 as f32),
                                    border: UiRect::all(px(1)),
                                    ..default()
                                },
                                BorderColor::all(BORDER_COLOUR),
                            ));
                        }
                        // ...and the choices on the right.
                        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(10), ..default() })
                            .with_children(|choices| {
                                choices.spawn((Text::new("Name"), body.clone()));
                                let entity = choices
                                    .spawn((
                                        NameField,
                                        TextField {
                                            text: offer.suggestion.clone(),
                                            max_chars: MAX_NAME_CHARS,
                                            placeholder: String::new(),
                                        },
                                        Node { width: px(WIDTH), padding: UiRect::axes(px(8), px(6)), ..default() },
                                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                                        Themed(Piece::TextField),
                                    ))
                                    .with_child((FieldText, Text::default(), body.clone()))
                                    .id();
                                field = Some(entity);
                                choices.spawn((Text::new("Class"), body.clone()));
                                choices.spawn(Node { column_gap: px(8), ..default() }).with_children(|buttons| {
                                    for (index, class) in offer.classes.iter().enumerate() {
                                        buttons.spawn((ClassButton(index), option(class.name.clone())));
                                    }
                                });
                                choices.spawn((
                                    ClassDescription,
                                    Text::default(),
                                    small.clone(),
                                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                                    Node { width: px(WIDTH), min_height: px(56), ..default() },
                                ));
                                choices.spawn((Text::new("Body"), body.clone()));
                                choices.spawn(Node { column_gap: px(8), ..default() }).with_children(|buttons| {
                                    for body_type in BodyType::ALL {
                                        buttons.spawn((BodyButton(body_type), option(body_type.name().to_string())));
                                    }
                                });
                            });
                        // ...and their looks furthest right.
                        row.spawn((
                            LooksColumn,
                            Node { flex_direction: FlexDirection::Column, row_gap: px(8), ..default() },
                        ));
                    });
                    panel.spawn((
                        Text::new("Press Enter to begin"),
                        body.clone(),
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                    ));
                    panel.spawn((ErrorText, Text::default(), small, TextColor(ERROR_COLOUR)));
                });
        });
    focus.0 = field;
}

/// Fills the looks column with what the content offers, once it's here.
fn build_looks(
    mut commands: Commands,
    content: Res<ContentStore>,
    column: Single<(Entity, Option<&Children>), With<LooksColumn>>,
) {
    let (column, children) = column.into_inner();
    let Some(models) = &content.character_models else { return };
    if children.is_some_and(|children| !children.is_empty()) {
        return;
    }
    let body = TextFont { font_size: FontSize::Px(16.0), ..default() };
    let small = TextFont { font_size: FontSize::Px(15.0), ..default() };
    let label = |text: &str| (Text::new(text), small.clone(), Node { width: px(LABEL_WIDTH), ..default() });
    let stepper = |feature: Feature, step: i8| {
        (
            Stepper(feature, step),
            Button,
            Node {
                width: px(24),
                height: px(24),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(OPTION_COLOUR),
            BorderColor::all(BORDER_COLOUR),
            Themed(Piece::Button),
            children![(Text::new(if step < 0 { "<" } else { ">" }), small.clone(), TextColor(TITLE_COLOUR))],
        )
    };
    let row = || Node { align_items: AlignItems::Center, column_gap: px(6), ..default() };
    commands.entity(column).with_children(|looks| {
        looks.spawn((Text::new("Looks"), body.clone()));
        for (feature, name) in Feature::SLIDERS {
            looks.spawn(row()).with_children(|line| {
                line.spawn(label(name));
                line.spawn(stepper(feature, -1));
                line.spawn((
                    Track(feature),
                    Button,
                    RelativeCursorPosition::default(),
                    Node {
                        width: px(TRACK_WIDTH),
                        height: px(10),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(TRACK_COLOUR),
                ))
                .with_child((
                    TrackFill(feature),
                    Node { height: percent(100), border_radius: BorderRadius::all(px(5)), ..default() },
                    BackgroundColor(FILL_COLOUR),
                ));
                line.spawn(stepper(feature, 1));
            });
        }
        for (feature, name) in [(Feature::Hair, "Hair"), (Feature::Beard, "Beard")] {
            looks.spawn(row()).with_children(|line| {
                line.spawn(label(name));
                line.spawn(stepper(feature, -1));
                line.spawn((
                    ChoiceName(feature),
                    Text::default(),
                    small.clone(),
                    TextColor(TITLE_COLOUR),
                    TextLayout::justify(Justify::Center),
                    Node { width: px(TRACK_WIDTH), ..default() },
                ));
                line.spawn(stepper(feature, 1));
            });
        }
        for (skin, name, palette) in [(false, "Hair colour", &models.hair_colours), (true, "Skin", &models.skin_tones)]
        {
            looks.spawn((Text::new(name), small.clone()));
            looks.spawn(Node { column_gap: px(5), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|line| {
                for (index, &(r, g, b)) in palette.iter().enumerate().take(u8::MAX as usize) {
                    line.spawn((
                        Swatch { skin, index: index as u8 },
                        Button,
                        Node {
                            width: px(SWATCH_SIZE),
                            height: px(SWATCH_SIZE),
                            border: UiRect::all(px(2)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(r, g, b)),
                        BorderColor::all(BORDER_COLOUR),
                    ));
                }
            });
        }
    });
}

/// Clicking the looks' controls changes the chosen features. Sliders' tracks
/// can be clicked or dragged along.
fn choose_looks(
    content: Res<ContentStore>,
    steppers: Query<(&Interaction, &Stepper), Changed<Interaction>>,
    tracks: Query<(&Interaction, &Track, &RelativeCursorPosition)>,
    swatches: Query<(&Interaction, &Swatch), Changed<Interaction>>,
    mut offer: ResMut<Offer>,
) {
    let Some(models) = &content.character_models else { return };
    let mut features = offer.features.clone();
    for (_, Stepper(feature, step)) in steppers.iter().filter(|(interaction, _)| **interaction == Interaction::Pressed)
    {
        match feature {
            Feature::Hair => {
                let usual = &models.bodies.of(offer.body).hair;
                let current = features.hair.as_ref().unwrap_or(usual);
                features.hair = Some(step_through(&models.hairstyles, Some(current), *step)).flatten();
            }
            Feature::Beard => features.beard = step_through(&models.beards, features.beard.as_ref(), *step),
            _ => *slider(&mut features, *feature) += step,
        }
    }
    for (_, Track(feature), cursor) in tracks.iter().filter(|(interaction, ..)| **interaction == Interaction::Pressed) {
        // (The cursor's position runs from -0.5 at the left to 0.5 at the right.)
        if let Some(position) = cursor.normalized {
            let along = (position.x + 0.5).clamp(0.0, 1.0);
            *slider(&mut features, *feature) = (along * 2.0 * f32::from(FEATURE_RANGE)).round() as i8 - FEATURE_RANGE;
        }
    }
    for (_, swatch) in swatches.iter().filter(|(interaction, _)| **interaction == Interaction::Pressed) {
        if swatch.skin {
            features.skin = swatch.index;
        } else {
            features.hair_colour = swatch.index;
        }
    }
    let features = models.allowed(features);
    if features != offer.features {
        offer.features = features;
    }
}

/// One of a slider's features.
fn slider(features: &mut Features, feature: Feature) -> &mut i8 {
    match feature {
        Feature::Build => &mut features.build,
        Feature::Head => &mut features.head,
        _ => &mut features.height,
    }
}

/// The next (`step` 1) or previous (-1) of a list with "none" at its start,
/// going round.
fn step_through(list: &[String], current: Option<&String>, step: i8) -> Option<String> {
    let places = list.len() as i32 + 1;
    let place = current.and_then(|current| list.iter().position(|item| item == current)).map_or(0, |at| at as i32 + 1);
    let next = (place + i32::from(step)).rem_euclid(places);
    (next > 0).then(|| list[next as usize - 1].clone())
}

/// A hair or beard's name as shown: "long" becomes "Long".
fn shown_name(name: &str) -> String {
    let mut letters = name.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

/// Shows the chosen looks: sliders' fills, names, and the chosen swatches.
fn show_looks(
    offer: Res<Offer>,
    content: Res<ContentStore>,
    mut fills: Query<(&TrackFill, &mut Node)>,
    mut names: Query<(&ChoiceName, &mut Text)>,
    mut swatches: Query<(&Swatch, &mut BorderColor)>,
    new_controls: Query<(), Added<Swatch>>,
) {
    if !offer.is_changed() && new_controls.is_empty() {
        return;
    }
    let Some(models) = &content.character_models else { return };
    let mut features = offer.features.clone();
    for (TrackFill(feature), mut node) in &mut fills {
        let value = f32::from(*slider(&mut features, *feature) + FEATURE_RANGE);
        node.width = percent(100.0 * value / f32::from(2 * FEATURE_RANGE));
    }
    for (ChoiceName(feature), mut text) in &mut names {
        text.0 = match feature {
            Feature::Hair => shown_name(features.hair.as_ref().unwrap_or(&models.bodies.of(offer.body).hair)),
            _ => features.beard.as_deref().map_or("None".to_string(), shown_name),
        };
    }
    for (swatch, mut border) in &mut swatches {
        let chosen = if swatch.skin { features.skin } else { features.hair_colour } == swatch.index;
        *border = BorderColor::all(if chosen { TITLE_COLOUR } else { Color::srgba(0.0, 0.0, 0.0, 0.5) });
    }
}

fn close_prompt(mut commands: Commands, screen: Query<Entity, With<CreationScreen>>, mut focus: ResMut<KeyboardFocus>) {
    for entity in &screen {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<PreviewImage>();
    focus.0 = None;
}

/// Clicking a class or body type chooses it (and puts the typing back in
/// the name field).
fn choose_options(
    classes: Query<(&Interaction, &ClassButton), Changed<Interaction>>,
    bodies: Query<(&Interaction, &BodyButton), Changed<Interaction>>,
    field: Single<Entity, With<NameField>>,
    mut offer: ResMut<Offer>,
    mut focus: ResMut<KeyboardFocus>,
) {
    for (interaction, button) in &classes {
        if *interaction == Interaction::Pressed {
            offer.class = button.0;
            focus.0 = Some(*field);
        }
    }
    for (interaction, button) in &bodies {
        if *interaction == Interaction::Pressed {
            offer.body = button.0;
            focus.0 = Some(*field);
        }
    }
}

/// Highlights the chosen options and shows what the class is like.
fn show_choices(
    offer: Res<Offer>,
    mut classes: Query<(&ClassButton, &mut BackgroundColor, &mut Themed), Without<BodyButton>>,
    mut bodies: Query<(&BodyButton, &mut BackgroundColor, &mut Themed), Without<ClassButton>>,
    mut description: Single<&mut Text, With<ClassDescription>>,
    new_buttons: Query<(), Or<(Added<ClassButton>, Added<BodyButton>)>>,
) {
    if !offer.is_changed() && new_buttons.is_empty() {
        return;
    }
    let show = |chosen: bool, background: &mut BackgroundColor, themed: &mut Themed| {
        background.0 = if chosen { CHOSEN_COLOUR } else { OPTION_COLOUR };
        themed.0 = if chosen { Piece::ButtonChosen } else { Piece::Button };
    };
    for (button, mut background, mut themed) in &mut classes {
        show(button.0 == offer.class, &mut background, &mut themed);
    }
    for (button, mut background, mut themed) in &mut bodies {
        show(button.0 == offer.body, &mut background, &mut themed);
    }
    description.0 = offer.classes.get(offer.class).map(|class| class.description.clone()).unwrap_or_default();
}

/// Dresses the preview character in the chosen class, body type and looks.
/// A new class or body type rebuilds the placeholder body (shown until the
/// models load), and has them wave hello in their new look.
fn dress_preview(
    mut commands: Commands,
    offer: Res<Offer>,
    assets: Res<CharacterAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    preview: Single<(Entity, &mut Gait), With<PreviewCharacter>>,
    mut shown: Local<Option<(usize, BodyType)>>,
) {
    if !offer.is_changed() {
        return;
    }
    let (entity, mut gait) = preview.into_inner();
    if *shown != Some((offer.class, offer.body)) {
        *shown = Some((offer.class, offer.body));
        let look = assets.player_look(0, &offer.look(), &mut materials);
        rebuild_body(&mut commands, entity, &assets, &look);
        gait.wave();
    }
    commands.entity(entity).insert(HumanLook::of_player(&offer.look()));
}

/// Puts every part of the preview character on the preview's render layer
/// (parts are made without one, as for characters in the world).
fn put_stage_on_its_layer(
    mut commands: Commands,
    preview: Single<Entity, With<PreviewCharacter>>,
    children: Query<&Children>,
    unlayered: Query<(), (With<Mesh3d>, Without<RenderLayers>)>,
) {
    for part in children.iter_descendants(*preview) {
        if unlayered.contains(part) {
            commands.entity(part).insert(RenderLayers::layer(PREVIEW_LAYER));
        }
    }
}

fn turn_preview(time: Res<Time>, dev: Res<DevSettings>, mut preview: Single<&mut Transform, With<PreviewCharacter>>) {
    // (Held still for screenshots.)
    if !dev.show_creation {
        preview.rotate_y(TURN_SPEED * time.delta_secs());
    }
}

/// Checks the name as it's submitted, and sends it with the chosen options
/// if it looks fine.
fn submit(
    mut submitted: MessageReader<TextSubmitted>,
    offer: Res<Offer>,
    field: Single<Entity, With<NameField>>,
    mut error: Single<&mut Text, With<ErrorText>>,
    mut to_server: MessageWriter<ToServer>,
) {
    for submission in submitted.read().filter(|submission| submission.field == *field) {
        let Some(class) = offer.classes.get(offer.class) else { continue };
        match clean_name(&submission.text) {
            Ok(name) => {
                error.0.clear();
                let class = class.id.clone();
                let (body, features) = (offer.body, offer.features.clone());
                to_server.write(ToServer(ClientMessage::CreateCharacter { name, class, body, features }));
            }
            Err(reason) => error.0 = reason,
        }
    }
}

fn show_rejections(mut events: MessageReader<FromServer>, error: Option<Single<&mut Text, With<ErrorText>>>) {
    let Some(mut error) = error else { return };
    for FromServer(message) in events.read() {
        if let ServerMessage::CharacterRejected { reason } = message {
            error.0.clone_from(reason);
        }
    }
}
