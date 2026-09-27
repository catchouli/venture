//! Tuning how things are held: on the Items tab, an item with held parts (a
//! sword, a shield) shows a person holding it, idle or swinging, next to the
//! grip of each held part (its bone, position, rotation and size, from
//! `characters/characters.ron`), which can be changed while watching.
//!
//! The person stands on a little stage of their own, far from the world,
//! drawn by a camera of its own into a picture the panel shows.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy_egui::{EguiTextureHandle, EguiUserTextures, egui};
use venture_client::characters::humans::{HumanLook, Paint};
use venture_client::characters::model::Gait;
use venture_content::characters::CharacterModels;
use venture_content::{AppearanceDef, ItemDef};
use venture_protocol::BodyType;

use crate::document::Document;
use crate::fields::{Choices, Fields};

/// Size of the picture, in pixels.
const SIZE: (u32, u32) = (360, 440);
/// The render layer the stage is on (the world is on layer 0).
const LAYER: usize = 3;
/// Where the stage is: far below the world.
const STAGE: Vec3 = Vec3::new(0.0, -3000.0, 0.0);
/// How often the person swings, when swinging, in seconds.
const SWING_EVERY: f32 = 1.4;

/// The grip stage.
pub struct GripsPlugin;

impl Plugin for GripsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_stage).add_systems(Update, (dress_holder, swing));
    }
}

/// The stage: its picture, and what's shown.
#[derive(Resource)]
pub struct GripStage {
    image: Handle<Image>,
    /// The item being held, if one is showing.
    pub item: Option<String>,
    /// The appearance being worn, if one is showing (instead of an item).
    pub appearance: Option<String>,
    /// The body holding it.
    pub body: BodyType,
    /// Whether they swing (rather than stand).
    pub swinging: bool,
    /// Which way the stage is turned (radians).
    pub turn: f32,
}

/// The person on the stage.
#[derive(Component)]
struct Holder;

fn build_stage(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(Image::new_target_texture(SIZE.0, SIZE.1, TextureFormat::Rgba8UnormSrgb, None));
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        Name::new("Grip camera"),
        Camera3d::default(),
        Camera { clear_color: ClearColorConfig::Custom(Color::srgb(0.13, 0.12, 0.11)), order: -2, ..default() },
        RenderTarget::Image(image.clone().into()),
        Transform::from_translation(STAGE + Vec3::new(0.0, 1.1, 2.4)).looking_at(STAGE + Vec3::Y, Vec3::Y),
        layer.clone(),
    ));
    for (illuminance, direction) in [(12000.0, Vec3::new(-0.5, -0.7, -0.5)), (5000.0, Vec3::new(0.3, -0.2, -1.0))] {
        commands.spawn((
            DirectionalLight { illuminance, ..default() },
            Transform::from_translation(STAGE).looking_to(direction, Vec3::Y),
            layer.clone(),
        ));
    }
    commands.spawn((
        Name::new("Grip holder"),
        Holder,
        Transform::from_translation(STAGE),
        Visibility::default(),
        Gait::default(),
        layer,
    ));
    commands.insert_resource(GripStage {
        image,
        item: None,
        appearance: None,
        body: BodyType::Masculine,
        swinging: false,
        turn: 0.6,
    });
}

/// Dresses the person in the item (and plain clothes), on the chosen body.
fn dress_holder(
    mut commands: Commands,
    stage: Res<GripStage>,
    document: Res<Document>,
    mut holder: Single<(Entity, &mut Transform, Option<&HumanLook>), With<Holder>>,
) {
    let (entity, transform, current) = &mut *holder;
    transform.rotation = Quat::from_rotation_y(stage.turn);
    let parts = stage
        .item
        .as_ref()
        .and_then(|id| item(&document, id))
        .and_then(|item| item.equip.map(|equip| equip.parts))
        .unwrap_or_default();
    let look = HumanLook {
        body: stage.body,
        hair: None,
        beard: None,
        parts,
        skin: Paint::Palette(3),
        hair_colour: Paint::Palette(0),
        sizes: [0; 3],
        shapes: Vec::new(),
        monster: None,
    };
    // An appearance, if one's showing, is worn instead.
    let appearance = stage.appearance.as_ref().and_then(|id| {
        let (_, all) = document.find::<AppearanceDef>()?;
        all.files().iter().flat_map(|file| &file.entries).find(|appearance| &appearance.id == id).cloned()
    });
    let models = document.find::<CharacterModels>().and_then(|(_, all)| all.files().first()?.entries.first().cloned());
    let look = match &appearance {
        Some(appearance) => {
            let items = document
                .find::<ItemDef>()
                .map(|(_, items)| {
                    items.files().iter().flat_map(|file| &file.entries).map(|i| (i.id.clone(), i.clone())).collect()
                })
                .unwrap_or_default();
            HumanLook::of_appearance(Some(appearance), models.as_ref(), &items)
        }
        None => look,
    };
    if current.is_none_or(|current| *current != look) {
        commands.entity(*entity).insert(look);
    }
}

fn swing(time: Res<Time>, stage: Res<GripStage>, mut gait: Single<&mut Gait, With<Holder>>, mut since: Local<f32>) {
    *since += time.delta_secs();
    if stage.swinging && *since > SWING_EVERY {
        gait.swing();
        *since = 0.0;
    }
}

/// An item, by id.
fn item(document: &Document, id: &str) -> Option<ItemDef> {
    let (_, items) = document.find::<ItemDef>()?;
    items.files().iter().flat_map(|file| &file.entries).find(|item| item.id == id).cloned()
}

/// The grip panel, for an item with held parts: the stage's picture, what
/// it shows, and each held part's grip. Returns whether a grip changed (in
/// the character models' file, which it tells the document about).
pub fn grip_panel(root: &mut egui::Ui, world: &mut World, document: &mut Document, item_id: &str, now: f64) {
    let Some(item) = item(document, item_id) else { return };
    let parts = item.equip.map(|equip| equip.parts).unwrap_or_default();
    let Some((models_index, models)) = document.find::<CharacterModels>() else { return };
    let held: Vec<String> = parts
        .iter()
        .filter(|part| {
            models
                .files()
                .first()
                .is_some_and(|file| file.entries[0].parts.get(*part).is_some_and(|p| p.held.is_some()))
        })
        .cloned()
        .collect();
    let Some(mut stage) = world.remove_resource::<GripStage>() else { return };
    stage.item = (!held.is_empty()).then(|| item_id.to_string());
    stage.appearance = None;
    if held.is_empty() {
        world.insert_resource(stage);
        return;
    }
    let texture = world.resource_mut::<EguiUserTextures>().add_image(EguiTextureHandle::Weak(stage.image.id()));
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let choices = world.resource::<Choices>().clone();
    let fields = Fields { registry: &registry, choices: &choices };
    let mut changed = false;
    egui::Panel::right("grips").resizable(true).default_size(400.0).show_inside(root, |ui| {
        ui.heading("How it's held");
        ui.add(egui::Image::new((texture, egui::vec2(SIZE.0 as f32, SIZE.1 as f32))));
        ui.horizontal(|ui| {
            for body in BodyType::ALL {
                ui.selectable_value(&mut stage.body, body, body.name());
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut stage.swinging, "Swing");
            ui.add(egui::Slider::new(&mut stage.turn, -3.1..=3.1).text("Turn"));
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for part in &held {
                ui.strong(part);
                let Some(value) = document.find_mut::<CharacterModels>().and_then(|(_, models)| {
                    models.files_mut().first_mut()?.entries.first_mut()?.parts.get_mut(part)?.held.as_mut()
                }) else {
                    continue;
                };
                ui.push_id(part, |ui| changed |= fields.edit(ui, value));
                ui.add_space(8.0);
            }
            ui.weak(
                "Position is in metres along the bone's own directions; rotation in degrees about X, then Y, then Z.",
            );
        });
    });
    world.insert_resource(stage);
    if changed {
        document.changed(models_index, 0, now);
    }
}

/// The appearance panel: a person in an appearance, as NPCs wearing it look.
pub fn appearance_panel(root: &mut egui::Ui, world: &mut World, appearance: &str) {
    let Some(mut stage) = world.remove_resource::<GripStage>() else { return };
    stage.item = None;
    stage.appearance = Some(appearance.to_string());
    let texture = world.resource_mut::<EguiUserTextures>().add_image(EguiTextureHandle::Weak(stage.image.id()));
    egui::Panel::right("appearance preview").resizable(true).default_size(400.0).show_inside(root, |ui| {
        ui.heading("How it looks");
        ui.add(egui::Image::new((texture, egui::vec2(SIZE.0 as f32, SIZE.1 as f32))));
        ui.horizontal(|ui| {
            ui.checkbox(&mut stage.swinging, "Swing");
            ui.add(egui::Slider::new(&mut stage.turn, -3.1..=3.1).text("Turn"));
        });
        ui.weak("Sizes left empty are picked from the appearance's id, as in the game.");
    });
    world.insert_resource(stage);
}
