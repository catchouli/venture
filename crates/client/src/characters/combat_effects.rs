//! What a fight looks like: attackers swing, damage numbers (or "Miss") float
//! up from whatever was hit, hits flash, and the dead fall over. Abilities add
//! their own looks (from their `visual` in the content): a weapon ability's
//! bigger, coloured flash; a spell's bolt flying to its target; a healing
//! glow and a green number.
//!
//! All placeholder effects made of simple shapes and text, until real
//! animations and effects are made. The server decides everything
//! ([`ServerMessage::Attacked`], [`ServerMessage::Died`]); this only shows it.

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use venture_content::{AbilityDef, AbilityEffect, AbilityVisual};
use venture_protocol::{AttackResult, EntityId, ServerMessage};

use crate::characters::model::Gait;
use crate::characters::nameplates::NameplateHeight;
use crate::characters::player::LocalPlayer;
use crate::content::ContentStore;
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
/// Colour of healing numbers.
const HEAL_COLOUR: Color = Color::srgb(0.45, 1.0, 0.45);
/// How big a bolt is, in metres across (times its visual's size).
const BOLT_SIZE: f32 = 0.22;
/// How long a healing glow lasts, in seconds.
const GLOW_SECONDS: f32 = 0.8;

/// Shows fights.
pub struct CombatEffectsPlugin;

impl Plugin for CombatEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_flash_mesh)
            .add_systems(Update, (show_fights, fade_flashes, fly_bolts, fade_glows, fall_over))
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
    colour: Color,
    size: f32,
}

/// A spell flying from its caster to its target (it lands when the
/// server's [`ServerMessage::Attacked`] comes; this only shows it going).
#[derive(Component)]
struct Bolt {
    target: Entity,
    speed: f32,
}

/// A healing glow rising around someone.
#[derive(Component)]
struct Glow {
    age: f32,
    colour: Color,
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

/// Plays each attack, ability, heal and death the server reports.
fn show_fights(
    mut commands: Commands,
    mut events: MessageReader<FromServer>,
    content: Res<ContentStore>,
    flash_mesh: Res<FlashMesh>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut target: ResMut<Target>,
    me: Option<Single<(Entity, &LocalPlayer)>>,
    others: Query<(Entity, &Targetable)>,
    mut gaits: Query<&mut Gait>,
    places: Query<(&GlobalTransform, Option<&NameplateHeight>)>,
    mut facings: Query<&mut Transform, With<Gait>>,
) {
    let my_id = me.as_ref().map(|me| me.1.id);
    let find = |id: EntityId| match me.as_ref().filter(|me| me.1.id == id) {
        Some(me) => Some(me.0),
        None => others.iter().find(|(_, targetable)| targetable.id == id).map(|(entity, _)| entity),
    };
    // Where to show things on someone: just above their name, and their chest.
    let spots = |entity: Entity| {
        let (place, height) = places.get(entity).ok()?;
        let height = height.map_or(2.2, |height| height.0);
        Some((place.translation() + Vec3::Y * (height + 0.35), place.translation() + Vec3::Y * height * 0.5))
    };
    for FromServer(message) in events.read() {
        match message {
            ServerMessage::Attacked { attacker, target: hit, result, ability } => {
                let def = ability.as_ref().and_then(|ability| content.abilities.get(ability));
                // Spells were cast when they set off; blows swing as they land.
                let swings = def.is_none_or(|def| matches!(def.effect, AbilityEffect::Strike { .. }));
                if swings && let Some(mut gait) = find(*attacker).and_then(|entity| gaits.get_mut(entity).ok()) {
                    gait.swing();
                }
                // Attackers turn to face what they're attacking.
                if let (Some(from), Some(to)) = (find(*attacker), find(*hit)) {
                    face(&mut facings, &places, from, to);
                }
                let Some((top, chest)) = find(*hit).and_then(spots) else { continue };
                let (text, colour, size) = match result {
                    AttackResult::Miss => ("Miss".to_string(), MISS_COLOUR, 15.0),
                    AttackResult::Hit(amount) if Some(*hit) == my_id => (amount.to_string(), HURT_COLOUR, 18.0),
                    AttackResult::Hit(amount) => (amount.to_string(), DAMAGE_COLOUR, 18.0),
                    AttackResult::Critical(amount) => (format!("{amount}!"), CRITICAL_COLOUR, 24.0),
                };
                // Abilities' numbers are a little bigger.
                let size = if def.is_some() { size + 3.0 } else { size };
                spawn_number(&mut commands, top, text, colour, size);
                if *result != AttackResult::Miss {
                    let (colour, size) = match def {
                        Some(AbilityDef { visual: AbilityVisual { colour: (r, g, b), size }, .. }) => {
                            (Color::srgb(*r, *g, *b), *size)
                        }
                        None => (FLASH_COLOUR, 1.0),
                    };
                    let flash = spawn_glow(&mut commands, &flash_mesh, &mut materials, chest, colour, size);
                    commands.entity(flash).insert(Flash { age: 0.0, colour, size });
                }
            }
            ServerMessage::AbilityUsed { user, ability, target: aimed } => {
                let (Some(def), Some(user)) = (content.abilities.get(ability), find(*user)) else { continue };
                if let Some(aimed_at) = aimed.and_then(find) {
                    face(&mut facings, &places, user, aimed_at);
                }
                let (r, g, b) = def.visual.colour;
                let colour = Color::srgb(r, g, b);
                match def.effect {
                    AbilityEffect::Bolt { speed, .. } => {
                        if let Ok(mut gait) = gaits.get_mut(user) {
                            gait.cast();
                        }
                        let (Some(target), Some((_, from))) = (aimed.and_then(find), spots(user)) else { continue };
                        let material = materials.add(StandardMaterial {
                            base_color: colour,
                            emissive: LinearRgba::from(colour) * 2.0,
                            ..default()
                        });
                        let transform = Transform::from_translation(from + Vec3::Y * 0.2)
                            .with_scale(Vec3::splat(BOLT_SIZE * def.visual.size));
                        commands.spawn((
                            Bolt { target, speed },
                            Mesh3d(flash_mesh.0.clone()),
                            MeshMaterial3d(material),
                            transform,
                            NotShadowCaster,
                        ));
                    }
                    AbilityEffect::Heal { .. } => {
                        let Some((_, chest)) = spots(user) else { continue };
                        let glow =
                            spawn_glow(&mut commands, &flash_mesh, &mut materials, chest, colour, def.visual.size);
                        commands.entity(glow).insert(Glow { age: 0.0, colour });
                    }
                    AbilityEffect::Strike { .. } | AbilityEffect::Rest => {}
                }
            }
            ServerMessage::Healed { id, amount } => {
                let Some((top, _)) = find(*id).and_then(spots) else { continue };
                spawn_number(&mut commands, top, format!("+{amount}"), HEAL_COLOUR, 18.0);
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

/// A glowing ball (a flash or healing glow, animated by what's added to it).
fn spawn_glow(
    commands: &mut Commands,
    mesh: &FlashMesh,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    colour: Color,
    size: f32,
) -> Entity {
    let material = materials.add(StandardMaterial {
        base_color: colour.with_alpha(0.8),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    let transform = Transform::from_translation(at).with_scale(Vec3::splat(0.05 * size));
    commands.spawn((Mesh3d(mesh.0.clone()), MeshMaterial3d(material), transform, NotShadowCaster)).id()
}

/// Turns a character to face another (about the vertical, as characters face +Z).
fn face(
    facings: &mut Query<&mut Transform, With<Gait>>,
    places: &Query<(&GlobalTransform, Option<&NameplateHeight>)>,
    from: Entity,
    to: Entity,
) {
    let (Ok((from_place, _)), Ok((to_place, _))) = (places.get(from), places.get(to)) else { return };
    let direction = to_place.translation() - from_place.translation();
    if direction.x.abs() + direction.z.abs() < 0.01 {
        return;
    }
    if let Ok(mut transform) = facings.get_mut(from) {
        transform.rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
    }
}

/// A number (damage, "Miss", healing) floating up from a spot.
fn spawn_number(commands: &mut Commands, at: Vec3, text: String, colour: Color, size: f32) {
    commands.spawn((
        DamageNumber { at, age: 0.0 },
        Text::new(text),
        TextFont { font_size: FontSize::Px(size), ..default() },
        TextColor(colour),
        TextShadow { offset: Vec2::splat(1.5), ..default() },
        Node { position_type: PositionType::Absolute, ..default() },
        Visibility::Hidden,
    ));
}

/// Flies bolts towards their targets (following them if they move), and
/// removes them when they arrive.
fn fly_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut bolts: Query<(Entity, &Bolt, &mut Transform)>,
    places: Query<(&GlobalTransform, Option<&NameplateHeight>)>,
) {
    for (entity, bolt, mut transform) in &mut bolts {
        let Ok((place, height)) = places.get(bolt.target) else {
            commands.entity(entity).despawn();
            continue;
        };
        let aim = place.translation() + Vec3::Y * height.map_or(1.1, |height| height.0 * 0.5);
        let to_go = aim - transform.translation;
        let step = bolt.speed * time.delta_secs();
        if to_go.length() <= step {
            commands.entity(entity).despawn();
        } else {
            transform.translation += to_go.normalize() * step;
        }
    }
}

/// Grows healing glows as they rise and fade.
fn fade_glows(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut glows: Query<(Entity, &mut Glow, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (entity, mut glow, mut transform, material) in &mut glows {
        glow.age += time.delta_secs();
        let progress = glow.age / GLOW_SECONDS;
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(0.3 + 0.5 * progress);
        transform.translation.y += 0.8 * time.delta_secs();
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = glow.colour.with_alpha(0.5 * (1.0 - progress));
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
        transform.scale = Vec3::splat((0.08 + 0.3 * progress) * flash.size);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = flash.colour.with_alpha(0.8 * (1.0 - progress));
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
