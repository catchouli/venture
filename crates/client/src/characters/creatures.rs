//! Creature bodies (placeholder models, until real ones): a rabbit that hops
//! as it goes, and a goblin (a small, green-skinned person with big ears and
//! a club, built from the ordinary character model).

use std::f32::consts::PI;

use bevy::prelude::*;
use venture_protocol::{BodyType, CreatureLook, CreatureShape, ItemLook, LookStyle};

use crate::characters::humans::HumanLook;
use crate::characters::model::{CharacterAssets, Gait, Look, Worn, spawn_character};

/// How big a goblin is, next to a person.
const GOBLIN_SCALE: f32 = 0.72;
/// How high a rabbit hops, in metres.
const HOP_HEIGHT: f32 = 0.12;
/// Hops per second while running.
const HOPS_PER_SECOND: f32 = 2.2;

/// Animates creatures.
pub struct CreaturesPlugin;

impl Plugin for CreaturesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, hop);
    }
}

/// A rabbit's body, which hops (and lunges when it bites).
#[derive(Component)]
struct RabbitBody {
    owner: Entity,
}

/// How high above a creature's feet its name floats, in metres.
pub fn name_height(look: &CreatureLook) -> f32 {
    match look.shape {
        CreatureShape::Rabbit => 0.8,
        CreatureShape::Goblin => 1.7,
    }
}

/// Spawns a creature standing at `transform` and returns its entity.
pub fn spawn_creature(
    commands: &mut Commands,
    assets: &CharacterAssets,
    materials: &mut Assets<StandardMaterial>,
    transform: Transform,
    look: &CreatureLook,
) -> Entity {
    // A creature with a model is dressed in it (see `humans`), once the
    // models have loaded; until then it has its shape.
    if let Some(model) = &look.model {
        let entity = spawn_shape(commands, assets, materials, transform, look, false);
        commands.entity(entity).insert(HumanLook::of_creature(model));
        return entity;
    }
    spawn_shape(commands, assets, materials, transform, look, true)
}

/// A creature's placeholder shape. (`scaled` shrinks goblins to goblin size;
/// a model has its own size.)
fn spawn_shape(
    commands: &mut Commands,
    assets: &CharacterAssets,
    materials: &mut Assets<StandardMaterial>,
    transform: Transform,
    look: &CreatureLook,
    scaled: bool,
) -> Entity {
    let mut material = |(r, g, b): (f32, f32, f32), roughness: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: roughness,
            ..default()
        })
    };
    match look.shape {
        CreatureShape::Goblin => {
            let clothes = material(look.trim, 0.9);
            let club = ItemLook { style: LookStyle::Club, main: (0.42, 0.30, 0.18), trim: (0.30, 0.20, 0.12) };
            let goblin = Look {
                body: BodyType::Masculine,
                skin: material(look.main, 0.7),
                hair: material((0.12, 0.1, 0.08), 0.6),
                tunic: clothes.clone(),
                trousers: material((look.trim.0 * 0.6, look.trim.1 * 0.6, look.trim.2 * 0.6), 0.9),
                worn: vec![Worn { style: club.style, main: material(club.main, 0.8), trim: material(club.trim, 0.8) }],
                pointy_ears: true,
            };
            let scale = if scaled { GOBLIN_SCALE } else { 1.0 };
            spawn_character(commands, assets, transform.with_scale(Vec3::splat(scale)), &goblin)
        }
        CreatureShape::Rabbit => {
            let fur = material(look.main, 0.9);
            let pale = material(look.trim, 0.9);
            let eyes = material((0.05, 0.04, 0.04), 0.3);
            let sphere = assets.unit_sphere();
            let owner = commands.spawn((Name::new("Rabbit"), transform, Visibility::default(), Gait::default())).id();
            let part = |material: &Handle<StandardMaterial>, position: Vec3, radii: Vec3, tilt: f32| {
                let transform =
                    Transform::from_translation(position).with_rotation(Quat::from_rotation_x(tilt)).with_scale(radii);
                (Mesh3d(sphere.clone()), MeshMaterial3d(material.clone()), transform)
            };
            let body = commands
                .spawn((RabbitBody { owner }, Transform::default(), Visibility::default()))
                .with_children(|body| {
                    body.spawn(part(&fur, Vec3::new(0.0, 0.17, -0.02), Vec3::new(0.15, 0.14, 0.22), 0.0));
                    body.spawn(part(&pale, Vec3::new(0.0, 0.13, 0.04), Vec3::new(0.1, 0.09, 0.15), 0.0));
                    body.spawn(part(&fur, Vec3::new(0.0, 0.3, 0.17), Vec3::new(0.09, 0.09, 0.1), 0.0));
                    for x in [-0.04, 0.04] {
                        // Long ears, laid back a little.
                        body.spawn(part(&fur, Vec3::new(x, 0.44, 0.12), Vec3::new(0.025, 0.1, 0.018), -0.35));
                        body.spawn(part(&eyes, Vec3::new(x * 1.6, 0.32, 0.25), Vec3::splat(0.014), 0.0));
                    }
                    body.spawn(part(&pale, Vec3::new(0.0, 0.2, -0.24), Vec3::splat(0.05), 0.0));
                    for x in [-0.07, 0.07] {
                        body.spawn(part(&fur, Vec3::new(x, 0.05, -0.08), Vec3::new(0.05, 0.05, 0.1), 0.0));
                    }
                })
                .id();
            commands.entity(owner).add_child(body);
            owner
        }
    }
}

/// Rabbits hop while they run, and lunge forward when they bite.
fn hop(time: Res<Time>, gaits: Query<&Gait>, mut bodies: Query<(&RabbitBody, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (body, mut transform) in &mut bodies {
        let Ok(gait) = gaits.get(body.owner) else { continue };
        let running = (gait.speed / 2.0).clamp(0.0, 1.0);
        let hop = (t * HOPS_PER_SECOND * PI).sin().abs() * HOP_HEIGHT * running;
        let lunge = gait.swing_progress().map_or(0.0, |progress| (progress * PI).sin() * 0.15);
        transform.translation = Vec3::new(0.0, hop, lunge);
        transform.rotation = Quat::from_rotation_x(-0.3 * lunge);
    }
}
