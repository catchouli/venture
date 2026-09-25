//! A placeholder human character, 1.8 metres tall, built from simple shapes
//! with jointed limbs, and a procedural run and idle animation. Its build
//! follows the character's body type, and its clothes their outfit (a tunic,
//! armour plates over one, or a robe).
//!
//! The character faces +Z in its own space. Its origin is on the ground
//! between its feet. Proportions follow a real adult (about 7.5 heads tall).

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use venture_content::{AppearanceDef, Colour};
use venture_protocol::{BodyType, OutfitStyle, PlayerLook};

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
    linen: Handle<StandardMaterial>,
    leather: Handle<StandardMaterial>,
    boots: Handle<StandardMaterial>,
    belt: Handle<StandardMaterial>,
    /// Skin tones, so players look different from one another.
    skins: Vec<Handle<StandardMaterial>>,
    /// Hair colours, likewise.
    hairs: Vec<Handle<StandardMaterial>>,
}

struct CharacterMeshes {
    unit_sphere: Handle<Mesh>,
    unit_cube: Handle<Mesh>,
    torso: Handle<Mesh>,
    belt: Handle<Mesh>,
    skirt: Handle<Mesh>,
    robe: Handle<Mesh>,
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
            unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            torso: meshes.add(Capsule3d::new(0.15, 0.3)),
            belt: meshes.add(Cylinder::new(0.16, 0.06)),
            skirt: meshes.add(ConicalFrustum { radius_top: 0.19, radius_bottom: 0.24, height: 0.22 }),
            robe: meshes.add(ConicalFrustum { radius_top: 0.18, radius_bottom: 0.3, height: 0.78 }),
            neck: meshes.add(Cylinder::new(0.055, 0.1)),
            upper_arm: meshes.add(Capsule3d::new(0.052, 0.2)),
            forearm: meshes.add(Capsule3d::new(0.045, 0.18)),
            thigh: meshes.add(Capsule3d::new(0.075, 0.28)),
            shin: meshes.add(Capsule3d::new(0.06, 0.28)),
            boot_shaft: meshes.add(Cylinder::new(0.07, 0.22)),
            foot: meshes.add(Cuboid::new(0.11, 0.08, 0.25)),
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut material = |(r, g, b): Colour, roughness: f32| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: roughness,
                ..default()
            })
        };
        Self {
            meshes: character_meshes,
            linen: material((0.78, 0.73, 0.62), 0.9),
            leather: material((0.46, 0.31, 0.19), 0.7),
            boots: material((0.20, 0.14, 0.10), 0.6),
            belt: material((0.16, 0.10, 0.07), 0.5),
            skins: SKIN_TONES.iter().map(|&colour| material(colour, 0.7)).collect(),
            hairs: HAIR_COLOURS.iter().map(|&colour| material(colour, 0.6)).collect(),
        }
    }
}

/// Skin tones players are given (picked by their id).
const SKIN_TONES: [Colour; 5] =
    [(0.86, 0.66, 0.53), (0.93, 0.76, 0.64), (0.72, 0.52, 0.38), (0.55, 0.38, 0.26), (0.40, 0.27, 0.18)];
/// Hair colours players are given (picked by their id).
const HAIR_COLOURS: [Colour; 6] = [
    (0.30, 0.19, 0.11),
    (0.12, 0.09, 0.07),
    (0.62, 0.42, 0.20),
    (0.78, 0.66, 0.42),
    (0.50, 0.20, 0.10),
    (0.60, 0.58, 0.55),
];

/// How a character is drawn: their build, the cut of their clothes, and colours.
#[derive(Clone)]
pub struct Look {
    /// Build.
    pub body: BodyType,
    /// The cut of their clothes.
    pub outfit: OutfitStyle,
    /// Skin.
    pub skin: Handle<StandardMaterial>,
    /// Hair.
    pub hair: Handle<StandardMaterial>,
    /// Tunic or robe.
    pub tunic: Handle<StandardMaterial>,
    /// Trousers.
    pub trousers: Handle<StandardMaterial>,
    /// Armour plates and trim.
    pub trim: Handle<StandardMaterial>,
}

impl CharacterAssets {
    /// A player's look: their build and outfit, with a skin tone and hair
    /// colour picked by `variant` (their id, say), so players look different.
    pub fn player_look(&self, variant: u32, look: &PlayerLook, materials: &mut Assets<StandardMaterial>) -> Look {
        let mut material = |(r, g, b): Colour, roughness: f32| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: roughness,
                ..default()
            })
        };
        let outfit = look.outfit;
        let hash = variant.wrapping_mul(2_654_435_761);
        Look {
            body: look.body,
            outfit: outfit.style,
            skin: self.skins[hash as usize % self.skins.len()].clone(),
            hair: self.hairs[(hash >> 8) as usize % self.hairs.len()].clone(),
            tunic: material(outfit.main, 0.8),
            trousers: material(darker(outfit.main), 0.9),
            trim: material(outfit.trim, 0.6),
        }
    }
}

/// A colour a little darker (for trousers under a tunic of that colour).
fn darker((r, g, b): Colour) -> Colour {
    (r * 0.6, g * 0.6, b * 0.6)
}

/// The look described by a content appearance, with new materials.
pub fn appearance_look(materials: &mut Assets<StandardMaterial>, appearance: &AppearanceDef) -> Look {
    let mut material = |(r, g, b): Colour, roughness: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: roughness,
            ..default()
        })
    };
    Look {
        body: appearance.body,
        outfit: appearance.outfit,
        skin: material(appearance.skin, 0.7),
        hair: material(appearance.hair, 0.6),
        tunic: material(appearance.tunic, 0.8),
        trousers: material(appearance.trousers, 0.9),
        // NPC armour is plain wood.
        trim: material((0.55, 0.40, 0.25), 0.6),
    }
}

/// The proportions of a build.
struct Build {
    /// How far out the shoulders are, in metres.
    shoulder_x: f32,
    /// How far apart the legs are (from the middle), in metres.
    hip_x: f32,
    /// Scale of the torso.
    torso: Vec3,
    /// Scale of the hips.
    hips: Vec3,
    /// Scale of the tunic's skirt.
    skirt: Vec3,
    /// Whether to shape a bust.
    bust: bool,
    /// Hair: short, to the jaw, or long down the back.
    hair: HairLength,
}

enum HairLength {
    Short,
    Medium,
    Long,
}

impl Build {
    fn of(body: BodyType) -> Self {
        match body {
            BodyType::Masculine => Self {
                shoulder_x: 0.23,
                hip_x: 0.095,
                torso: Vec3::new(1.4, 1.0, 0.9),
                hips: Vec3::new(0.16, 0.11, 0.12),
                skirt: Vec3::new(1.05, 1.0, 0.85),
                bust: false,
                hair: HairLength::Short,
            },
            BodyType::Feminine => Self {
                shoulder_x: 0.2,
                hip_x: 0.1,
                torso: Vec3::new(1.18, 1.0, 0.82),
                hips: Vec3::new(0.19, 0.12, 0.13),
                skirt: Vec3::new(1.22, 1.0, 0.92),
                bust: true,
                hair: HairLength::Long,
            },
            BodyType::Androgynous => Self {
                shoulder_x: 0.215,
                hip_x: 0.097,
                torso: Vec3::new(1.28, 1.0, 0.85),
                hips: Vec3::new(0.175, 0.11, 0.125),
                skirt: Vec3::new(1.12, 1.0, 0.88),
                bust: false,
                hair: HairLength::Medium,
            },
        }
    }
}

/// Spawns a character standing at `transform` and returns its entity.
pub fn spawn_character(commands: &mut Commands, assets: &CharacterAssets, transform: Transform, look: &Look) -> Entity {
    let owner = commands.spawn((Name::new("Character"), transform, Visibility::default(), Gait::default())).id();
    build_body(commands, owner, assets, look);
    owner
}

/// Rebuilds a character's body with a new look (such as when its appearance
/// changes in the content). The character entity itself stays.
pub fn rebuild_body(commands: &mut Commands, owner: Entity, assets: &CharacterAssets, look: &Look) {
    commands.entity(owner).despawn_children();
    build_body(commands, owner, assets, look);
}

/// Builds the body parts and joints under a character entity.
fn build_body(commands: &mut Commands, owner: Entity, assets: &CharacterAssets, look: &Look) {
    let tunic = &look.tunic;
    let m = &assets.meshes;
    let build = Build::of(look.body);
    let robed = look.outfit == OutfitStyle::Robe;
    let armoured = look.outfit == OutfitStyle::Armour;
    let joint = |kind| Joint { owner, kind };

    // A mesh part, placed relative to its parent.
    let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform| {
        (Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), transform)
    };
    let sphere = |material: &Handle<StandardMaterial>, position: Vec3, radii: Vec3| {
        part(&m.unit_sphere, material, Transform::from_translation(position).with_scale(radii))
    };
    let block = |material: &Handle<StandardMaterial>, position: Vec3, size: Vec3| {
        part(&m.unit_cube, material, Transform::from_translation(position).with_scale(size))
    };
    // Sleeves match a robe; otherwise they're the linen shirt under the tunic.
    let sleeves = if robed { tunic } else { &assets.linen };
    let belt = if robed { &look.trim } else { &assets.belt };

    let body = commands
        .spawn((joint(JointKind::Body), Transform::from_xyz(0.0, 0.95, 0.0), Visibility::default()))
        .with_children(|body| {
            // Hips, then a tunic's skirt or a robe down to the ankles, and a belt.
            body.spawn(sphere(&look.trousers, Vec3::new(0.0, 0.02, 0.0), build.hips));
            if robed {
                let robe_scale = Vec3::new(build.skirt.x * 0.95, 1.0, build.skirt.z);
                body.spawn(part(&m.robe, tunic, scaled(0.0, -0.36, 0.0, robe_scale)));
            } else {
                body.spawn(part(&m.skirt, tunic, scaled(0.0, -0.06, 0.0, build.skirt)));
            }
            body.spawn(part(&m.torso, tunic, scaled(0.0, 0.30, 0.0, build.torso)));
            if build.bust {
                body.spawn(sphere(tunic, Vec3::new(0.0, 0.38, 0.07), Vec3::new(0.13, 0.06, 0.055)));
            }
            body.spawn(part(&m.belt, belt, scaled(0.0, 0.06, 0.0, Vec3::new(build.torso.x * 0.96, 1.0, 0.85))));
            // Armour: wooden plates strapped over the chest and back.
            if armoured {
                let width = 0.2 * build.torso.x;
                body.spawn(block(
                    &look.trim,
                    Vec3::new(0.0, 0.34, 0.125 + if build.bust { 0.03 } else { 0.0 }),
                    Vec3::new(width, 0.26, 0.035),
                ));
                body.spawn(block(&look.trim, Vec3::new(0.0, 0.34, -0.13), Vec3::new(width, 0.28, 0.035)));
                body.spawn(block(&assets.belt, Vec3::new(0.0, 0.28, 0.0), Vec3::new(width + 0.02, 0.03, 0.29)));
            }
            // Neck, head and hair. The top of the hair is 1.8 m above the ground.
            body.spawn(part(&m.neck, &look.skin, Transform::from_xyz(0.0, 0.60, 0.0)));
            body.spawn(sphere(&look.skin, Vec3::new(0.0, 0.72, 0.0), Vec3::new(0.106, 0.127, 0.115)));
            match build.hair {
                // Cropped close to the head.
                HairLength::Short => {
                    body.spawn(sphere(&look.hair, Vec3::new(0.0, 0.752, -0.018), Vec3::new(0.113, 0.11, 0.124)));
                }
                // A bob to the jaw, covering the sides of the head.
                HairLength::Medium => {
                    body.spawn(sphere(&look.hair, Vec3::new(0.0, 0.745, -0.015), Vec3::new(0.117, 0.12, 0.128)));
                    body.spawn(sphere(&look.hair, Vec3::new(0.0, 0.70, -0.04), Vec3::new(0.124, 0.1, 0.105)));
                    for x in [-0.098, 0.098] {
                        body.spawn(sphere(&look.hair, Vec3::new(x, 0.69, 0.0), Vec3::new(0.03, 0.075, 0.075)));
                    }
                }
                // Long: down the back, and locks either side of the face to the shoulders.
                HairLength::Long => {
                    body.spawn(sphere(&look.hair, Vec3::new(0.0, 0.745, -0.015), Vec3::new(0.117, 0.12, 0.128)));
                    body.spawn(sphere(&look.hair, Vec3::new(0.0, 0.70, -0.045), Vec3::new(0.124, 0.11, 0.105)));
                    body.spawn(sphere(&look.hair, Vec3::new(0.0, 0.55, -0.09), Vec3::new(0.1, 0.19, 0.055)));
                    for x in [-0.1, 0.1] {
                        body.spawn(sphere(&look.hair, Vec3::new(x, 0.63, 0.0), Vec3::new(0.032, 0.13, 0.06)));
                    }
                }
            }
        })
        .id();
    commands.entity(owner).add_child(body);

    for side in [Side::Left, Side::Right] {
        let x = side.sign();
        // Armour has bigger shoulder guards in its plates' colour.
        let (guard, guard_size) = if armoured { (&look.trim, 0.095) } else { (&assets.leather, 0.075) };
        let shoulder = commands
            .spawn((
                joint(JointKind::Shoulder(side)),
                Transform::from_xyz(build.shoulder_x * x, 0.52, 0.0),
                Visibility::default(),
            ))
            .with_children(|shoulder| {
                shoulder.spawn(sphere(guard, Vec3::ZERO, Vec3::splat(guard_size)));
                shoulder.spawn(part(&m.upper_arm, sleeves, Transform::from_xyz(0.0, -0.14, 0.0)));
                shoulder
                    .spawn((joint(JointKind::Elbow), Transform::from_xyz(0.0, -0.28, 0.0), Visibility::default()))
                    .with_children(|elbow| {
                        elbow.spawn(part(&m.forearm, sleeves, Transform::from_xyz(0.0, -0.13, 0.0)));
                        elbow.spawn(sphere(&look.skin, Vec3::new(0.0, -0.28, 0.0), Vec3::new(0.04, 0.055, 0.03)));
                    });
            })
            .id();
        commands.entity(body).add_child(shoulder);

        let hip = commands
            .spawn((
                joint(JointKind::Hip(side)),
                Transform::from_xyz(build.hip_x * x, 0.92, 0.0),
                Visibility::default(),
            ))
            .with_children(|hip| {
                hip.spawn(part(&m.thigh, &look.trousers, Transform::from_xyz(0.0, -0.21, 0.0)));
                hip.spawn((joint(JointKind::Knee(side)), Transform::from_xyz(0.0, -0.44, 0.0), Visibility::default()))
                    .with_children(|knee| {
                        knee.spawn(part(&m.shin, &look.trousers, Transform::from_xyz(0.0, -0.2, 0.0)));
                        knee.spawn(part(&m.boot_shaft, &assets.boots, Transform::from_xyz(0.0, -0.32, 0.0)));
                        knee.spawn(part(&m.foot, &assets.boots, Transform::from_xyz(0.0, -0.44, 0.05)));
                    });
            })
            .id();
        commands.entity(owner).add_child(hip);
    }
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
