//! What a fight looks like: attackers swing, damage numbers (or "Miss") float
//! up from whatever was hit, hits flash, and the dead fall over.
//!
//! All placeholder effects made of simple shapes and text, until real
//! animations and effects are made. The server decides everything
//! ([`ServerMessage::Attacked`], [`ServerMessage::Died`]); this only shows it.

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use venture_protocol::{AttackResult, EntityId, ServerMessage};

use crate::characters::model::Gait;
use crate::characters::nameplates::NameplateHeight;
use crate::characters::player::LocalPlayer;
use crate::interaction::targeting::{Target, Targetable};
use crate::network::FromServer;

/// How long a damage number floats, in seconds.
const NUMBER_SECONDS: f32 = 1.2;
/// How far it rises, in pixels.
const NUMBER_RISE: f32 = 40.0;
/// How long a hit's flash lasts, in seconds.
const FLASH_SECONDS: f32 = 0.25;
/// How long the dead take to fall over, in seconds.
const FALL_SECONDS: f32 = 0.4;
/// Colour of damage done to others.
const DAMAGE_COLOUR: Color = Color::srgb(1.0, 0.95, 0.85);
/// Colour of damage done to the player.
const HURT_COLOUR: Color = Color::srgb(1.0, 0.35, 0.3);
/// Colour of critical hits.
const CRITICAL_COLOUR: Color = Color::srgb(1.0, 0.8, 0.25);
/// Colour of misses.
const MISS_COLOUR: Color = Color::srgb(0.75, 0.75, 0.72);
/// Colour of a hit's flash.
const FLASH_COLOUR: Color = Color::srgb(1.0, 0.85, 0.6);

/// Shows fights.
pub struct CombatEffectsPlugin;

impl Plugin for CombatEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_flash_mesh)
            .add_systems(Update, (show_fights, fade_flashes, fall_over))
            .add_systems(PostUpdate, float_numbers.before(TransformSystems::Propagate));
    }
}

/// A damage number floating up from where something was hit.
#[derive(Component)]
struct DamageNumber {
    /// Where it started, in the world.
    at: Vec3,
    age: f32,
}

/// A hit's flash.
#[derive(Component)]
struct Flash {
    age: f32,
}

/// Something that has died, falling over.
#[derive(Component)]
struct Fallen {
    age: f32,
    /// How it was facing when it died.
    facing: Quat,
}

/// The flash's shape.
#[derive(Resource)]
struct FlashMesh(Handle<Mesh>);

fn make_flash_mesh(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(FlashMesh(meshes.add(Sphere::new(1.0).mesh().ico(2).expect("valid subdivision"))));
}

/// Plays each attack and death the server reports.
fn show_fights(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    flash_mesh: Res<FlashMesh>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut target: ResMut<Target>,
    me: Option<Single<(Entity, &LocalPlayer)>>,
    others: Query<(Entity, &Targetable)>,
    mut gaits: Query<&mut Gait>,
    places: Query<(&GlobalTransform, Option<&NameplateHeight>)>,
) {
    let my_id = me.as_ref().map(|me| me.1.id);
    let find = |id: EntityId| match me.as_ref().filter(|me| me.1.id == id) {
        Some(me) => Some(me.0),
        None => others.iter().find(|(_, targetable)| targetable.id == id).map(|(entity, _)| entity),
    };
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::Attacked { attacker, target: hit, result } => {
                if let Some(mut gait) = find(*attacker).and_then(|entity| gaits.get_mut(entity).ok()) {
                    gait.swing();
                }
                let Some((place, height)) = find(*hit).and_then(|entity| places.get(entity).ok()) else { continue };
                // Just above the name, so the number doesn't cross it as it rises.
                let top = place.translation() + Vec3::Y * (height.map_or(2.2, |height| height.0) + 0.35);
                let (text, colour, size) = match result {
                    AttackResult::Miss => ("Miss".to_string(), MISS_COLOUR, 15.0),
                    AttackResult::Hit(amount) if Some(*hit) == my_id => (amount.to_string(), HURT_COLOUR, 18.0),
                    AttackResult::Hit(amount) => (amount.to_string(), DAMAGE_COLOUR, 18.0),
                    AttackResult::Critical(amount) => (format!("{amount}!"), CRITICAL_COLOUR, 24.0),
                };
                commands.spawn((
                    DamageNumber { at: top, age: 0.0 },
                    Text::new(text),
                    TextFont { font_size: FontSize::Px(size), ..default() },
                    TextColor(colour),
                    TextShadow { offset: Vec2::splat(1.5), ..default() },
                    Node { position_type: PositionType::Absolute, ..default() },
                    Visibility::Hidden,
                ));
                if *result != AttackResult::Miss {
                    let material = materials.add(StandardMaterial {
                        base_color: FLASH_COLOUR.with_alpha(0.8),
                        alpha_mode: AlphaMode::Add,
                        unlit: true,
                        ..default()
                    });
                    let chest = place.translation() + Vec3::Y * height.map_or(1.2, |height| height.0 * 0.5);
                    commands.spawn((
                        Flash { age: 0.0 },
                        Mesh3d(flash_mesh.0.clone()),
                        MeshMaterial3d(material),
                        Transform::from_translation(chest).with_scale(Vec3::splat(0.05)),
                        NotShadowCaster,
                    ));
                }
            }
            ServerMessage::Died { id } => {
                // The player's own character isn't knocked over: they wake up at once.
                if Some(*id) == my_id {
                    continue;
                }
                let Some(dead) = find(*id) else { continue };
                let facing = places.get(dead).map(|(place, _)| place.rotation()).unwrap_or_default();
                commands.entity(dead).insert(Fallen { age: 0.0, facing }).remove::<Targetable>();
                if target.0 == Some(dead) {
                    target.0 = None;
                }
            }
            _ => {}
        }
    }
}

/// Floats damage numbers up from where they started, fading them out.
fn float_numbers(
    mut commands: Commands,
    time: Res<Time>,
    camera: Single<(&Camera, &GlobalTransform), With<IsDefaultUiCamera>>,
    mut numbers: Query<(Entity, &mut DamageNumber, &mut Node, &mut TextColor, &mut Visibility, &ComputedNode)>,
) {
    let (camera, camera_transform) = *camera;
    for (entity, mut number, mut node, mut colour, mut visibility, computed) in &mut numbers {
        number.age += time.delta_secs();
        let progress = number.age / NUMBER_SECONDS;
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let Ok(screen) = camera.world_to_viewport(camera_transform, number.at) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = px(screen.x - size.x / 2.0);
        node.top = px(screen.y - size.y - NUMBER_RISE * progress);
        colour.0.set_alpha((1.0 - progress).powf(0.6));
        *visibility = Visibility::Inherited;
    }
}

/// Grows and fades hits' flashes.
fn fade_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(Entity, &mut Flash, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (entity, mut flash, mut transform, material) in &mut flashes {
        flash.age += time.delta_secs();
        let progress = flash.age / FLASH_SECONDS;
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(0.08 + 0.3 * progress);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = FLASH_COLOUR.with_alpha(0.8 * (1.0 - progress));
        }
    }
}

/// Tips the dead over onto their side.
fn fall_over(time: Res<Time>, mut fallen: Query<(&mut Fallen, &mut Transform)>) {
    for (mut fallen, mut transform) in &mut fallen {
        fallen.age = (fallen.age + time.delta_secs()).min(FALL_SECONDS);
        let progress = fallen.age / FALL_SECONDS;
        transform.rotation = fallen.facing * Quat::from_rotation_z(FRAC_PI_2 * progress * progress);
    }
}
