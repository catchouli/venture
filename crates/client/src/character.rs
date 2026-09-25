//! A placeholder human character, 1.8 metres tall, built from simple shapes
//! with jointed limbs, and a procedural run and idle animation.
//!
//! The character faces +Z in its own space. Its origin is on the ground
//! between its feet. Proportions follow a real adult (about 7.5 heads tall).

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

/// Distance covered by one full running cycle (two steps), in metres.
const STRIDE_LENGTH: f32 = 2.6;
/// Ground speed at which the run animation is at full strength, in metres per second.
pub const RUN_SPEED: f32 = 5.0;
/// How quickly the animation blends between standing and running (per second).
const BLEND_RATE: f32 = 8.0;

/// Builds characters and animates them.
pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CharacterAssets>().add_systems(Update, animate_joints);
    }
}

/// How a character is currently moving, which drives its animation.
#[derive(Component, Default)]
pub struct Gait {
    /// Current ground speed, in metres per second. Set by whatever moves the character.
    pub speed: f32,
    /// How far through the running cycle the legs are, in radians.
    phase: f32,
    /// How much of the run animation is showing, from `0.0` (standing) to `1.0` (running).
    running: f32,
}

/// Which side of the body a limb is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

impl Side {
    /// The sign of the side's X position (the character's left is +X, as it faces +Z).
    fn sign(self) -> f32 {
        match self {
            Side::Left => 1.0,
            Side::Right => -1.0,
        }
    }

    /// The left leg is half a cycle ahead of the right.
    fn phase_offset(self) -> f32 {
        match self {
            Side::Left => 0.0,
            Side::Right => PI,
        }
    }
}

/// A pivot the animation rotates, belonging to the character entity `owner`.
#[derive(Component)]
struct Joint {
    owner: Entity,
    kind: JointKind,
}

#[derive(Clone, Copy)]
enum JointKind {
    /// Everything above the hips: bobs, leans and twists.
    Body,
    Shoulder(Side),
    Elbow,
    Hip(Side),
    Knee(Side),
}

/// Meshes and materials for characters, made once.
#[derive(Resource)]
pub struct CharacterAssets {
    meshes: CharacterMeshes,
    skin: Handle<StandardMaterial>,
    hair: Handle<StandardMaterial>,
    linen: Handle<StandardMaterial>,
    leather: Handle<StandardMaterial>,
    trousers: Handle<StandardMaterial>,
    boots: Handle<StandardMaterial>,
    belt: Handle<StandardMaterial>,
    /// Tunic colours, so characters are easier to tell apart.
    tunics: Vec<Handle<StandardMaterial>>,
}

struct CharacterMeshes {
    unit_sphere: Handle<Mesh>,
    torso: Handle<Mesh>,
    belt: Handle<Mesh>,
    skirt: Handle<Mesh>,
    neck: Handle<Mesh>,
    upper_arm: Handle<Mesh>,
    forearm: Handle<Mesh>,
    thigh: Handle<Mesh>,
    shin: Handle<Mesh>,
    boot_shaft: Handle<Mesh>,
    foot: Handle<Mesh>,
}

impl FromWorld for CharacterAssets {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let character_meshes = CharacterMeshes {
            unit_sphere: meshes.add(Sphere::new(1.0).mesh().ico(3).expect("valid subdivision")),
            torso: meshes.add(Capsule3d::new(0.15, 0.3)),
            belt: meshes.add(Cylinder::new(0.16, 0.06)),
            skirt: meshes.add(ConicalFrustum { radius_top: 0.19, radius_bottom: 0.24, height: 0.22 }),
            neck: meshes.add(Cylinder::new(0.055, 0.1)),
            upper_arm: meshes.add(Capsule3d::new(0.052, 0.2)),
            forearm: meshes.add(Capsule3d::new(0.045, 0.18)),
            thigh: meshes.add(Capsule3d::new(0.075, 0.28)),
            shin: meshes.add(Capsule3d::new(0.06, 0.28)),
            boot_shaft: meshes.add(Cylinder::new(0.07, 0.22)),
            foot: meshes.add(Cuboid::new(0.11, 0.08, 0.25)),
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut material = |r: f32, g: f32, b: f32, roughness: f32| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: roughness,
                ..default()
            })
        };
        Self {
            meshes: character_meshes,
            skin: material(0.86, 0.66, 0.53, 0.7),
            hair: material(0.30, 0.19, 0.11, 0.6),
            linen: material(0.78, 0.73, 0.62, 0.9),
            leather: material(0.46, 0.31, 0.19, 0.7),
            trousers: material(0.30, 0.26, 0.22, 0.9),
            boots: material(0.20, 0.14, 0.10, 0.6),
            belt: material(0.16, 0.10, 0.07, 0.5),
            tunics: vec![
                material(0.46, 0.31, 0.19, 0.7),
                material(0.25, 0.35, 0.22, 0.8),
                material(0.45, 0.18, 0.15, 0.8),
                material(0.22, 0.28, 0.42, 0.8),
                material(0.40, 0.36, 0.30, 0.8),
                material(0.35, 0.22, 0.38, 0.8),
            ],
        }
    }
}

/// Spawns a character standing at `transform` and returns its entity.
/// `variant` picks its tunic colour (any number; it wraps around).
pub fn spawn_character(
    commands: &mut Commands,
    assets: &CharacterAssets,
    transform: Transform,
    variant: u32,
) -> Entity {
    let tunic = &assets.tunics[variant as usize % assets.tunics.len()];
    let owner = commands.spawn((Name::new("Character"), transform, Visibility::default(), Gait::default())).id();
    let m = &assets.meshes;
    let joint = |kind| Joint { owner, kind };

    // A mesh part, placed relative to its parent.
    let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform| {
        (Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), transform)
    };
    let sphere = |material: &Handle<StandardMaterial>, position: Vec3, radii: Vec3| {
        part(&m.unit_sphere, material, Transform::from_translation(position).with_scale(radii))
    };

    let body = commands
        .spawn((joint(JointKind::Body), Transform::from_xyz(0.0, 0.95, 0.0), Visibility::default()))
        .with_children(|body| {
            // Hips, tunic and belt.
            body.spawn(sphere(&assets.trousers, Vec3::new(0.0, 0.02, 0.0), Vec3::new(0.17, 0.11, 0.12)));
            body.spawn(part(&m.skirt, tunic, scaled(0.0, -0.06, 0.0, Vec3::new(1.1, 1.0, 0.85))));
            body.spawn(part(&m.torso, tunic, scaled(0.0, 0.30, 0.0, Vec3::new(1.3, 1.0, 0.85))));
            body.spawn(part(&m.belt, &assets.belt, scaled(0.0, 0.06, 0.0, Vec3::new(1.25, 1.0, 0.85))));
            // Neck, head and hair. The top of the hair is 1.8 m above the ground.
            body.spawn(part(&m.neck, &assets.skin, Transform::from_xyz(0.0, 0.60, 0.0)));
            body.spawn(sphere(&assets.skin, Vec3::new(0.0, 0.72, 0.0), Vec3::new(0.106, 0.127, 0.115)));
            body.spawn(sphere(&assets.hair, Vec3::new(0.0, 0.745, -0.015), Vec3::new(0.117, 0.12, 0.128)));
        })
        .id();
    commands.entity(owner).add_child(body);

    for side in [Side::Left, Side::Right] {
        let x = side.sign();
        let shoulder = commands
            .spawn((joint(JointKind::Shoulder(side)), Transform::from_xyz(0.22 * x, 0.52, 0.0), Visibility::default()))
            .with_children(|shoulder| {
                shoulder.spawn(sphere(&assets.leather, Vec3::ZERO, Vec3::splat(0.075)));
                shoulder.spawn(part(&m.upper_arm, &assets.linen, Transform::from_xyz(0.0, -0.14, 0.0)));
                shoulder
                    .spawn((joint(JointKind::Elbow), Transform::from_xyz(0.0, -0.28, 0.0), Visibility::default()))
                    .with_children(|elbow| {
                        elbow.spawn(part(&m.forearm, &assets.linen, Transform::from_xyz(0.0, -0.13, 0.0)));
                        elbow.spawn(sphere(&assets.skin, Vec3::new(0.0, -0.28, 0.0), Vec3::new(0.04, 0.055, 0.03)));
                    });
            })
            .id();
        commands.entity(body).add_child(shoulder);

        let hip = commands
            .spawn((joint(JointKind::Hip(side)), Transform::from_xyz(0.095 * x, 0.92, 0.0), Visibility::default()))
            .with_children(|hip| {
                hip.spawn(part(&m.thigh, &assets.trousers, Transform::from_xyz(0.0, -0.21, 0.0)));
                hip.spawn((joint(JointKind::Knee(side)), Transform::from_xyz(0.0, -0.44, 0.0), Visibility::default()))
                    .with_children(|knee| {
                        knee.spawn(part(&m.shin, &assets.trousers, Transform::from_xyz(0.0, -0.2, 0.0)));
                        knee.spawn(part(&m.boot_shaft, &assets.boots, Transform::from_xyz(0.0, -0.32, 0.0)));
                        knee.spawn(part(&m.foot, &assets.boots, Transform::from_xyz(0.0, -0.44, 0.05)));
                    });
            })
            .id();
        commands.entity(owner).add_child(hip);
    }
    owner
}

/// A transform at `(x, y, z)` with a non-uniform `scale`.
fn scaled(x: f32, y: f32, z: f32, scale: Vec3) -> Transform {
    Transform::from_xyz(x, y, z).with_scale(scale)
}

/// Advances each character's gait, then poses its joints.
///
/// Rotations are about the X axis (the character's left-right axis). A
/// negative angle swings a hanging limb forward (+Z); a positive one swings it back.
fn animate_joints(time: Res<Time>, mut gaits: Query<&mut Gait>, mut joints: Query<(&Joint, &mut Transform)>) {
    let dt = time.delta_secs();
    for mut gait in &mut gaits {
        let target = (gait.speed / RUN_SPEED).clamp(0.0, 1.0);
        gait.running += (target - gait.running) * (BLEND_RATE * dt).min(1.0);
        gait.phase = (gait.phase + gait.speed * dt / STRIDE_LENGTH * TAU) % TAU;
    }

    let breathing = (time.elapsed_secs() * 2.0).sin();
    for (joint, mut transform) in &mut joints {
        let Ok(gait) = gaits.get(joint.owner) else { continue };
        let (run, phase) = (gait.running, gait.phase);
        transform.rotation = match joint.kind {
            JointKind::Body => {
                // Bob up at each footfall, lean into the run and twist with the stride.
                transform.translation.y = 0.95 + run * (0.04 * phase.cos().abs() - 0.03) + 0.004 * breathing;
                Quat::from_rotation_x(0.16 * run) * Quat::from_rotation_y(0.12 * phase.sin() * run)
            }
            JointKind::Hip(side) => {
                let leg = phase + side.phase_offset();
                Quat::from_rotation_x(-0.75 * leg.sin() * run)
            }
            JointKind::Knee(side) => {
                // Bends most while the leg swings forward, and slightly when standing.
                let leg = phase + side.phase_offset();
                Quat::from_rotation_x(run * (0.2 + 1.1 * leg.cos().max(0.0)) + 0.03)
            }
            JointKind::Shoulder(side) => {
                // Arms swing against the leg on the same side, and hang slightly
                // away from the body.
                let leg = phase + side.phase_offset();
                Quat::from_rotation_z(side.sign() * 0.08) * Quat::from_rotation_x(0.6 * leg.sin() * run)
            }
            JointKind::Elbow => Quat::from_rotation_x(-(0.15 + 1.1 * run)),
        };
    }
}
