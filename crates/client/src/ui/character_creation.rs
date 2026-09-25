//! Making a character, the first time a player joins: choosing a name, a
//! class and a body type, with a turning preview of how they'll look. The
//! server offers the classes ([`ServerMessage::CharacterNeeded`]) and turns
//! away names that are taken ([`ServerMessage::CharacterRejected`]).
//!
//! The preview is a character model on a little stage of its own, drawn by
//! a second camera into an image that the window shows. The stage is on its
//! own render layer, so the world's camera never sees it.

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use venture_protocol::{BodyType, ClassChoice, ClientMessage, MAX_NAME_CHARS, PlayerLook, ServerMessage, clean_name};

use crate::characters::model::{CharacterAssets, rebuild_body, spawn_character};
use crate::dev::settings::DevSettings;
use crate::network::{ClientState, FromServer, ToServer};
use crate::ui::text_input::{FieldText, KeyboardFocus, TextField, TextInputSystems, TextSubmitted};
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
}

impl Offer {
    /// How the chosen options look.
    fn look(&self) -> PlayerLook {
        let outfit = self.classes.get(self.class).map(|class| class.outfit).unwrap_or_default();
        PlayerLook { body: self.body, outfit }
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
    let mut image = Image::new_target_texture(
        PREVIEW_SIZE.0,
        PREVIEW_SIZE.1,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    image.asset_usage = RenderAssetUsages::default();
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
    commands.entity(character).insert((CreationScreen, PreviewCharacter, layer));
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
        to_server.write(ToServer(ClientMessage::CreateCharacter { name, class, body: dev.body.unwrap_or_default() }));
        return;
    }
    // Development runs can ask for a class and body type to show.
    let wanted = dev.class.as_deref();
    offer.class = offer.classes.iter().position(|class| Some(class.id.as_str()) == wanted).unwrap_or(0);
    offer.body = dev.body.unwrap_or_default();
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
    mut classes: Query<(&ClassButton, &mut BackgroundColor), Without<BodyButton>>,
    mut bodies: Query<(&BodyButton, &mut BackgroundColor), Without<ClassButton>>,
    mut description: Single<&mut Text, With<ClassDescription>>,
    new_buttons: Query<(), Or<(Added<ClassButton>, Added<BodyButton>)>>,
) {
    if !offer.is_changed() && new_buttons.is_empty() {
        return;
    }
    let colour = |chosen: bool| if chosen { CHOSEN_COLOUR } else { OPTION_COLOUR };
    for (button, mut background) in &mut classes {
        background.0 = colour(button.0 == offer.class);
    }
    for (button, mut background) in &mut bodies {
        background.0 = colour(button.0 == offer.body);
    }
    description.0 = offer.classes.get(offer.class).map(|class| class.description.clone()).unwrap_or_default();
}

/// Dresses the preview character in the chosen class and body type.
fn dress_preview(
    mut commands: Commands,
    offer: Res<Offer>,
    assets: Res<CharacterAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    preview: Single<Entity, With<PreviewCharacter>>,
) {
    if offer.is_changed() {
        let look = assets.player_look(0, &offer.look(), &mut materials);
        rebuild_body(&mut commands, *preview, &assets, &look);
    }
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
                to_server.write(ToServer(ClientMessage::CreateCharacter { name, class, body: offer.body }));
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
