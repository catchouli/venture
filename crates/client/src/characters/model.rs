//! A placeholder human character, 1.8 metres tall, built from simple shapes
//! with jointed limbs, and a procedural run and idle animation. Its build
//! follows the character's body type. Over plain clothes it wears what the
//! character has equipped (plates, a robe), with the weapon in the right hand.
//!
//! The character faces +Z in its own space. Its origin is on the ground
//! between its feet. Proportions follow a real adult (about 7.5 heads tall).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;
use venture_content::{AppearanceDef, Colour, OutfitStyle};
use venture_protocol::{BodyType, ItemLook, LookStyle, PlayerLook};
use venture_simulation::JUMP_SECONDS;

/// Distance covered by one full running cycle (two steps), in metres.
const STRIDE_LENGTH: f32 = 2.6;
/// Ground speed at which the run animation is at full strength, in metres per second.
pub const RUN_SPEED: f32 = 5.0;
/// How quickly the animation blends between standing and running (per second).
const BLEND_RATE: f32 = 8.0;
/// How long a swing of a weapon lasts, in seconds.
const SWING_SECONDS: f32 = 0.45;
/// How long a wave lasts, in seconds.
const WAVE_SECONDS: f32 = 1.8;
/// How high a jump goes, in metres.
const JUMP_HEIGHT: f32 = 0.45;
/// How quickly characters sit down and stand up (higher is quicker).
const SIT_RATE: f32 = 6.0;
/// How far the hips drop when sitting, in metres.
const SIT_DROP: f32 = 0.8;
/// How high the hip joints are when standing, in metres.
const HIP_HEIGHT: f32 = 0.92;
/// How long a wave takes to blend in and out, in seconds.
const WAVE_BLEND_SECONDS: f32 = 0.3;

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
    /// Seconds into a wave, if the character is waving.
    waving: Option<f32>,
    /// Seconds into a swing, if the character is swinging a weapon.
    swinging: Option<f32>,
    /// Whether the character is sitting down to rest. Set from snapshots.
    pub resting: bool,
    /// Seconds into a jump, if the character is in the air.
    jumping: Option<f32>,
    /// Whether the swing going on is casting a spell (rather than a blow).
    casting: bool,
    /// How far down they are, from `0.0` (standing) to `1.0` (sitting).
    sitting: f32,
}

impl Gait {
    /// Starts a friendly wave of the right hand (or starts it over).
    pub fn wave(&mut self) {
        self.waving = Some(0.0);
    }

    /// Jumps, unless already in the air. Returns whether it did.
    pub fn jump(&mut self) -> bool {
        let ready = self.jumping.is_none();
        if ready {
            self.jumping = Some(0.0);
        }
        ready
    }

    /// How high the jump has lifted the character (metres), and how far
    /// through it they are (0 to 1), while in the air.
    pub fn jump_amount(&self) -> (f32, f32) {
        match self.jumping {
            Some(time) => {
                let progress = (time / JUMP_SECONDS).clamp(0.0, 1.0);
                // A parabola: up quickly, a moment at the top, and down.
                (JUMP_HEIGHT * 4.0 * progress * (1.0 - progress), progress)
            }
            None => (0.0, 0.0),
        }
    }

    /// Starts a swing of the weapon (or fist), as in an attack.
    pub fn swing(&mut self) {
        self.swinging = Some(0.0);
        self.casting = false;
    }

    /// Starts casting a spell (a swing, for the simple bodies).
    pub fn cast(&mut self) {
        self.swinging = Some(0.0);
        self.casting = true;
    }

    /// Whether the swing going on is casting a spell.
    pub fn is_casting(&self) -> bool {
        self.casting && self.swinging.is_some()
    }

    /// Whether the character is waving.
    pub fn is_waving(&self) -> bool {
        self.waving.is_some()
    }

    /// Whether the character is in the middle of a swing (for monsters'
    /// own attack animations), and how far through it, from 0 to 1.
    pub fn swing_progress(&self) -> Option<f32> {
        self.swinging.map(|time| time / SWING_SECONDS)
    }

    /// The right arm's angle for a swing, if swinging: raised up and forward,
    /// then brought down hard.
    fn swing_angle(&self) -> Option<f32> {
        let progress = self.swing_progress()?;
        Some(if progress < 0.35 {
            -2.6 * (progress / 0.35)
        } else {
            -2.6 + 2.4 * ((progress - 0.35) / 0.65).min(1.0).powf(0.5)
        })
    }

    /// How much of the wave is showing, from `0.0` to `1.0`, and how far into it.
    fn wave_amount(&self) -> (f32, f32) {
        let Some(time) = self.waving else { return (0.0, 0.0) };
        let blend_in = (time / WAVE_BLEND_SECONDS).min(1.0);
        let blend_out = ((WAVE_SECONDS - time) / WAVE_BLEND_SECONDS).clamp(0.0, 1.0);
        let amount = blend_in.min(blend_out);
        // Smoothstep, so the arm eases up and down.
        (amount * amount * (3.0 - 2.0 * amount), time)
    }
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

/// Where the simple shapes' "elbow" space (the forearm hanging down its -Y,
/// the hand at 0.29 m) sits on a model's right hand bone (which points along
/// its +Y, towards the fingers).
const HAND_FRAME: Transform = Transform::from_xyz(0.0, -0.29, 0.0).with_rotation(Quat::from_xyzw(0.0, 0.0, 1.0, 0.0));
/// Where the simple shapes' elbow space sits on a model's left forearm bone.
const FOREARM_FRAME: Transform = Transform::from_xyz(0.0, 0.0, 0.0).with_rotation(Quat::from_xyzw(0.0, 0.0, 1.0, 0.0));

/// A top-level part of a placeholder body (made of simple shapes), which a
/// character model replaces once the models have loaded (see
/// [`crate::characters::humans`]).
#[derive(Component)]
pub struct PlaceholderPart;

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
    Elbow(Side),
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
    unit_cylinder: Handle<Mesh>,
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
            unit_cylinder: meshes.add(Cylinder::new(1.0, 1.0)),
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

/// How a character is drawn: their build, their plain clothes, and what
/// they wear over them.
#[derive(Clone)]
pub struct Look {
    /// Build.
    pub body: BodyType,
    /// Skin.
    pub skin: Handle<StandardMaterial>,
    /// Hair.
    pub hair: Handle<StandardMaterial>,
    /// Their plain tunic.
    pub tunic: Handle<StandardMaterial>,
    /// Their plain trousers.
    pub trousers: Handle<StandardMaterial>,
    /// Each worn item: its shape, and its main and trim colours.
    pub worn: Vec<Worn>,
    /// Whether they have big pointed ears (goblins do).
    pub pointy_ears: bool,
}

/// A worn item, ready to draw.
#[derive(Clone)]
pub struct Worn {
    /// Its shape.
    pub style: LookStyle,
    /// Its main colour.
    pub main: Handle<StandardMaterial>,
    /// Its second colour.
    pub trim: Handle<StandardMaterial>,
}

/// Colour of everyone's plain tunic, under what they wear.
const PLAIN_TUNIC: Colour = (0.52, 0.47, 0.38);
/// Colour of everyone's plain trousers.
const PLAIN_TROUSERS: Colour = (0.30, 0.26, 0.22);

impl CharacterAssets {
    /// A player's look: their build and what they wear, with a skin tone and
    /// hair colour picked by `variant` (their id, say), so players look different.
    pub fn player_look(&self, variant: u32, look: &PlayerLook, materials: &mut Assets<StandardMaterial>) -> Look {
        let hash = variant.wrapping_mul(2_654_435_761);
        Look {
            body: look.body,
            skin: self.skins[hash as usize % self.skins.len()].clone(),
            hair: self.hairs[(hash >> 8) as usize % self.hairs.len()].clone(),
            tunic: material(materials, PLAIN_TUNIC, 0.9),
            trousers: material(materials, PLAIN_TROUSERS, 0.9),
            worn: look.worn.iter().map(|item| worn(materials, item)).collect(),
            pointy_ears: false,
        }
    }

    /// For character models: puts simple shapes on a model's bones for held
    /// things that have no model: weapons in the right hand, a buckler on
    /// the left forearm.
    pub fn attach_shapes(
        &self,
        commands: &mut Commands,
        materials: &mut Assets<StandardMaterial>,
        looks: &[ItemLook],
        hand: Option<Entity>,
        forearm: Option<Entity>,
    ) {
        for look in looks {
            let shape = worn(materials, look);
            match (look.style, hand, forearm) {
                (LookStyle::Sword | LookStyle::Staff | LookStyle::Club | LookStyle::Rod, Some(hand), _) => {
                    commands
                        .spawn((HAND_FRAME, Visibility::default(), ChildOf(hand)))
                        .with_children(|frame| spawn_weapon(frame, &self.meshes, &shape));
                }
                (LookStyle::Buckler, _, Some(forearm)) => {
                    commands
                        .spawn((FOREARM_FRAME, Visibility::default(), ChildOf(forearm)))
                        .with_children(|frame| spawn_buckler(frame, &self.meshes, &shape, 1.0));
                }
                _ => {}
            }
        }
    }

    /// A sphere of radius 1, for building other bodies (such as monsters').
    pub fn unit_sphere(&self) -> Handle<Mesh> {
        self.meshes.unit_sphere.clone()
    }
}

fn material(materials: &mut Assets<StandardMaterial>, (r, g, b): Colour, roughness: f32) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, ..default() })
}

fn worn(materials: &mut Assets<StandardMaterial>, item: &ItemLook) -> Worn {
    // Weapons have a little shine to them; clothes and wood don't.
    let roughness = if matches!(item.style, LookStyle::Sword) { 0.35 } else { 0.7 };
    Worn {
        style: item.style,
        main: material(materials, item.main, roughness),
        trim: material(materials, item.trim, 0.6),
    }
}

/// The look described by a content appearance, with new materials. An NPC's
/// outfit becomes the same worn shapes players' equipment uses.
pub fn appearance_look(materials: &mut Assets<StandardMaterial>, appearance: &AppearanceDef) -> Look {
    let outfit = match appearance.outfit {
        OutfitStyle::Tunic => Vec::new(),
        OutfitStyle::Armour => {
            let wood = ItemLook { style: LookStyle::Plates, main: (0.55, 0.40, 0.25), trim: (0.28, 0.18, 0.10) };
            vec![wood, ItemLook { style: LookStyle::LegPlates, ..wood }]
        }
        OutfitStyle::Robe => {
            vec![ItemLook { style: LookStyle::Robe, main: appearance.tunic, trim: appearance.trousers }]
        }
    };
    Look {
        body: appearance.body,
        skin: material(materials, appearance.skin, 0.7),
        hair: material(materials, appearance.hair, 0.6),
        tunic: material(materials, appearance.tunic, 0.8),
        trousers: material(materials, appearance.trousers, 0.9),
        worn: outfit.iter().map(|item| worn(materials, item)).collect(),
        pointy_ears: false,
    }
}

impl Look {
    /// The first worn item of a shape, if any.
    fn wearing(&self, style: LookStyle) -> Option<&Worn> {
        self.worn.iter().find(|item| item.style == style)
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
    // Out with the old placeholder parts (only those: other children, such
    // as dressed models, stay).
    commands.queue(move |world: &mut World| {
        let Some(children) = world.get::<Children>(owner) else { return };
        let old: Vec<Entity> = children.iter().filter(|child| world.get::<PlaceholderPart>(*child).is_some()).collect();
        for part in old {
            world.despawn(part);
        }
    });
    build_body(commands, owner, assets, look);
}

/// Builds the body parts and joints under a character entity.
fn build_body(commands: &mut Commands, owner: Entity, assets: &CharacterAssets, look: &Look) {
    let tunic = &look.tunic;
    let m = &assets.meshes;
    let build = Build::of(look.body);
    let robe = look.wearing(LookStyle::Robe);
    let plates = look.wearing(LookStyle::Plates);
    let leg_plates = look.wearing(LookStyle::LegPlates);
    let weapon = look
        .worn
        .iter()
        .find(|item| matches!(item.style, LookStyle::Sword | LookStyle::Staff | LookStyle::Club | LookStyle::Rod));
    let shield = look.wearing(LookStyle::Buckler);
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
    let sleeves = robe.map_or(&assets.linen, |robe| &robe.main);
    let belt = robe.map_or(&assets.belt, |robe| &robe.trim);

    let body = commands
        .spawn((joint(JointKind::Body), Transform::from_xyz(0.0, 0.95, 0.0), Visibility::default()))
        .with_children(|body| {
            // Hips, then a tunic's skirt or a robe down to the ankles, and a belt.
            body.spawn(sphere(&look.trousers, Vec3::new(0.0, 0.02, 0.0), build.hips));
            // A robe covers the tunic, top to bottom.
            let top = robe.map_or(tunic, |robe| &robe.main);
            if let Some(robe) = robe {
                let robe_scale = Vec3::new(build.skirt.x * 0.95, 1.0, build.skirt.z);
                body.spawn(part(&m.robe, &robe.main, scaled(0.0, -0.36, 0.0, robe_scale)));
            } else {
                body.spawn(part(&m.skirt, tunic, scaled(0.0, -0.06, 0.0, build.skirt)));
            }
            body.spawn(part(&m.torso, top, scaled(0.0, 0.30, 0.0, build.torso)));
            if build.bust {
                body.spawn(sphere(top, Vec3::new(0.0, 0.38, 0.07), Vec3::new(0.13, 0.06, 0.055)));
            }
            body.spawn(part(&m.belt, belt, scaled(0.0, 0.06, 0.0, Vec3::new(build.torso.x * 0.96, 1.0, 0.85))));
            // Armour: wooden plates strapped over the chest and back.
            if let Some(plates) = plates {
                let width = 0.2 * build.torso.x;
                body.spawn(block(
                    &plates.main,
                    Vec3::new(0.0, 0.34, 0.125 + if build.bust { 0.03 } else { 0.0 }),
                    Vec3::new(width, 0.26, 0.035),
                ));
                body.spawn(block(&plates.main, Vec3::new(0.0, 0.34, -0.13), Vec3::new(width, 0.28, 0.035)));
                body.spawn(block(&plates.trim, Vec3::new(0.0, 0.28, 0.0), Vec3::new(width + 0.02, 0.03, 0.29)));
            }
            // Neck, head and hair. The top of the hair is 1.8 m above the ground.
            body.spawn(part(&m.neck, &look.skin, Transform::from_xyz(0.0, 0.60, 0.0)));
            body.spawn(sphere(&look.skin, Vec3::new(0.0, 0.72, 0.0), Vec3::new(0.106, 0.127, 0.115)));
            if look.pointy_ears {
                for x in [-1.0, 1.0] {
                    let ear = Transform::from_xyz(0.12 * x, 0.73, -0.01)
                        .with_rotation(Quat::from_rotation_z(-0.9 * x))
                        .with_scale(Vec3::new(0.025, 0.09, 0.035));
                    body.spawn((Mesh3d(m.unit_sphere.clone()), MeshMaterial3d(look.skin.clone()), ear));
                }
            }
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
    commands.entity(body).insert(PlaceholderPart);
    commands.entity(owner).add_child(body);

    for side in [Side::Left, Side::Right] {
        let x = side.sign();
        // Armour has bigger shoulder guards in its plates' colour.
        let (guard, guard_size) = plates.map_or((&assets.leather, 0.075), |plates| (&plates.main, 0.095));
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
                    .spawn((joint(JointKind::Elbow(side)), Transform::from_xyz(0.0, -0.28, 0.0), Visibility::default()))
                    .with_children(|elbow| {
                        elbow.spawn(part(&m.forearm, sleeves, Transform::from_xyz(0.0, -0.13, 0.0)));
                        elbow.spawn(sphere(&look.skin, Vec3::new(0.0, -0.28, 0.0), Vec3::new(0.04, 0.055, 0.03)));
                        // The weapon, in the right hand; a shield on the left forearm.
                        if let (Some(weapon), Side::Right) = (weapon, side) {
                            spawn_weapon(elbow, m, weapon);
                        }
                        if let (Some(shield), Side::Left) = (shield, side) {
                            spawn_buckler(elbow, m, shield, x);
                        }
                    });
            })
            .id();
        commands.entity(body).add_child(shoulder);

        let hip = commands
            .spawn((
                joint(JointKind::Hip(side)),
                Transform::from_xyz(build.hip_x * x, HIP_HEIGHT, 0.0),
                Visibility::default(),
            ))
            .with_children(|hip| {
                hip.spawn(part(&m.thigh, &look.trousers, Transform::from_xyz(0.0, -0.21, 0.0)));
                // Leg plates: one strapped over the front of the thigh, one over the shin.
                if let Some(plates) = leg_plates {
                    hip.spawn(block(&plates.main, Vec3::new(0.0, -0.2, 0.072), Vec3::new(0.14, 0.3, 0.03)));
                    hip.spawn(block(&plates.trim, Vec3::new(0.0, -0.2, 0.0), Vec3::new(0.165, 0.035, 0.17)));
                }
                hip.spawn((joint(JointKind::Knee(side)), Transform::from_xyz(0.0, -0.44, 0.0), Visibility::default()))
                    .with_children(|knee| {
                        knee.spawn(part(&m.shin, &look.trousers, Transform::from_xyz(0.0, -0.2, 0.0)));
                        if let Some(plates) = leg_plates {
                            knee.spawn(block(&plates.main, Vec3::new(0.0, -0.17, 0.066), Vec3::new(0.12, 0.24, 0.028)));
                        }
                        knee.spawn(part(&m.boot_shaft, &assets.boots, Transform::from_xyz(0.0, -0.32, 0.0)));
                        knee.spawn(part(&m.foot, &assets.boots, Transform::from_xyz(0.0, -0.44, 0.05)));
                    });
            })
            .id();
        commands.entity(hip).insert(PlaceholderPart);
        commands.entity(owner).add_child(hip);
    }
}

/// Straps a small round shield to the outside of a forearm (the elbow's
/// space; `outward` is +1 or -1, the side the arm is on).
fn spawn_buckler(elbow: &mut ChildSpawnerCommands, m: &CharacterMeshes, shield: &Worn, outward: f32) {
    // A flat disc facing out to the side, with a boss in the middle.
    let face = Quat::from_rotation_z(FRAC_PI_2);
    let centre = Vec3::new(0.075 * outward, -0.16, 0.0);
    elbow.spawn((
        Mesh3d(m.unit_cylinder.clone()),
        MeshMaterial3d(shield.main.clone()),
        Transform::from_translation(centre).with_rotation(face).with_scale(Vec3::new(0.17, 0.03, 0.17)),
    ));
    elbow.spawn((
        Mesh3d(m.unit_cylinder.clone()),
        MeshMaterial3d(shield.trim.clone()),
        Transform::from_translation(centre - Vec3::X * 0.004 * outward)
            .with_rotation(face)
            .with_scale(Vec3::new(0.18, 0.02, 0.18)),
    ));
    elbow.spawn((
        Mesh3d(m.unit_sphere.clone()),
        MeshMaterial3d(shield.trim.clone()),
        Transform::from_translation(centre + Vec3::X * 0.02 * outward).with_scale(Vec3::new(0.02, 0.04, 0.04)),
    ));
}

/// How far a held fishing rod leans from upright towards the front, in radians.
pub const ROD_ANGLE: f32 = 1.3;

/// The tip of a held fishing rod, where a line leaves it.
#[derive(Component)]
pub struct RodTip;

/// Puts a weapon in the hand at the end of a forearm (the elbow's space).
/// Swords point forward, as if ready; staves stand upright; rods reach up
/// and out, over the water.
fn spawn_weapon(elbow: &mut ChildSpawnerCommands, m: &CharacterMeshes, weapon: &Worn) {
    let hand = Vec3::new(0.0, -0.29, 0.0);
    let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, position: Vec3, size: Vec3| {
        (Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(position).with_scale(size))
    };
    match weapon.style {
        LookStyle::Sword => {
            // Held with the blade forward and a little up: the weapon's own Y
            // axis runs along the blade.
            let grip = Transform::from_translation(hand).with_rotation(Quat::from_rotation_x(1.35));
            elbow.spawn((grip, Visibility::default())).with_children(|sword| {
                sword.spawn(part(&m.unit_cylinder, &weapon.trim, Vec3::ZERO, Vec3::new(0.022, 0.14, 0.022)));
                sword.spawn(part(&m.unit_cube, &weapon.trim, Vec3::new(0.0, 0.08, 0.0), Vec3::new(0.16, 0.025, 0.035)));
                sword.spawn(part(&m.unit_cube, &weapon.main, Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.05, 0.64, 0.012)));
            });
        }
        LookStyle::Club => {
            // Thick at the end, held out forward like the sword.
            let grip = Transform::from_translation(hand).with_rotation(Quat::from_rotation_x(1.35));
            elbow.spawn((grip, Visibility::default())).with_children(|club| {
                club.spawn(part(
                    &m.unit_cylinder,
                    &weapon.trim,
                    Vec3::new(0.0, 0.12, 0.0),
                    Vec3::new(0.025, 0.3, 0.025),
                ));
                club.spawn(part(&m.unit_sphere, &weapon.main, Vec3::new(0.0, 0.33, 0.0), Vec3::new(0.06, 0.11, 0.06)));
            });
        }
        LookStyle::Rod => {
            // A long, thin, tapering pole, angled up and forward, with a cork
            // grip (the trim) and a reel just above the hand.
            let grip = Transform::from_translation(hand).with_rotation(Quat::from_rotation_x(ROD_ANGLE));
            elbow.spawn((grip, Visibility::default())).with_children(|rod| {
                rod.spawn(part(&m.unit_cylinder, &weapon.trim, Vec3::new(0.0, 0.05, 0.0), Vec3::new(0.02, 0.3, 0.02)));
                rod.spawn(part(&m.unit_cylinder, &weapon.main, Vec3::new(0.0, 0.8, 0.0), Vec3::new(0.013, 1.2, 0.013)));
                rod.spawn(part(
                    &m.unit_cylinder,
                    &weapon.main,
                    Vec3::new(0.0, 1.95, 0.0),
                    Vec3::new(0.007, 1.1, 0.007),
                ));
                rod.spawn(part(
                    &m.unit_sphere,
                    &weapon.trim,
                    Vec3::new(0.0, 0.25, 0.04),
                    Vec3::new(0.02, 0.045, 0.045),
                ));
                // Where a fishing line leaves it.
                rod.spawn((RodTip, Transform::from_xyz(0.0, 2.5, 0.0), Visibility::default()));
            });
        }
        LookStyle::Staff => {
            elbow.spawn((Transform::from_translation(hand), Visibility::default())).with_children(|staff| {
                staff.spawn(part(
                    &m.unit_cylinder,
                    &weapon.main,
                    Vec3::new(0.0, 0.3, 0.0),
                    Vec3::new(0.022, 1.5, 0.022),
                ));
                staff.spawn(part(&m.unit_sphere, &weapon.main, Vec3::new(0.0, 1.06, 0.0), Vec3::splat(0.045)));
                staff.spawn(part(
                    &m.unit_cylinder,
                    &weapon.trim,
                    Vec3::new(0.0, 0.02, 0.0),
                    Vec3::new(0.027, 0.12, 0.027),
                ));
            });
        }
        _ => {}
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
        if let Some(time) = gait.waving {
            gait.waving = Some(time + dt).filter(|&time| time < WAVE_SECONDS);
        }
        if let Some(time) = gait.swinging {
            gait.swinging = Some(time + dt).filter(|&time| time < SWING_SECONDS);
        }
        if let Some(time) = gait.jumping {
            gait.jumping = Some(time + dt).filter(|&time| time < JUMP_SECONDS);
        }
        let target = if gait.resting { 1.0 } else { 0.0 };
        gait.sitting += (target - gait.sitting) * (SIT_RATE * dt).min(1.0);
    }

    let breathing = (time.elapsed_secs() * 2.0).sin();
    for (joint, mut transform) in &mut joints {
        let Ok(gait) = gaits.get(joint.owner) else { continue };
        let (run, phase) = (gait.running, gait.phase);
        let (wave, wave_time) = gait.wave_amount();
        // Sitting lowers the body to the ground and lays the legs out in front.
        let sit = gait.sitting;
        // Jumping lifts the whole body, tucking the legs up in the middle of it.
        let (lift, jump) = gait.jump_amount();
        let tuck = if jump > 0.0 { (jump * PI).sin() } else { 0.0 };
        let drop = SIT_DROP * sit - lift;
        transform.rotation = match joint.kind {
            JointKind::Body => {
                // Bob up at each footfall, lean into the run and twist with the stride.
                transform.translation.y = 0.95 + run * (0.04 * phase.cos().abs() - 0.03) + 0.004 * breathing - drop;
                Quat::from_rotation_x(0.16 * run - 0.08 * sit) * Quat::from_rotation_y(0.12 * phase.sin() * run)
            }
            JointKind::Hip(side) => {
                transform.translation.y = HIP_HEIGHT - drop;
                let leg = phase + side.phase_offset();
                Quat::from_rotation_x(-0.75 * leg.sin() * run - FRAC_PI_2 * sit - 0.5 * tuck)
                    * Quat::from_rotation_z(side.sign() * 0.12 * sit)
            }
            JointKind::Knee(side) => {
                // Bends most while the leg swings forward, and slightly when standing.
                let leg = phase + side.phase_offset();
                Quat::from_rotation_x(run * (0.2 + 1.1 * leg.cos().max(0.0)) + 0.03 + 0.25 * sit + 0.9 * tuck)
            }
            JointKind::Shoulder(side) => {
                // Arms swing against the leg on the same side, and hang slightly
                // away from the body. Waving raises the right arm out and up.
                let leg = phase + side.phase_offset();
                let raise = if side == Side::Right { 1.9 * wave } else { 0.0 };
                // A swing takes over the right arm.
                let swing = gait.swing_angle().filter(|_| side == Side::Right);
                let forward = swing.unwrap_or(0.6 * leg.sin() * run - 0.35 * sit);
                Quat::from_rotation_z(side.sign() * (0.08 + raise)) * Quat::from_rotation_x(forward)
            }
            JointKind::Elbow(side) => {
                // Waving bends the raised forearm and swings it from side to side.
                let swing = if side == Side::Right { wave * (0.9 + 0.45 * (wave_time * 11.0).sin()) } else { 0.0 };
                Quat::from_rotation_z(side.sign() * swing) * Quat::from_rotation_x(-(0.15 + 1.1 * run) * (1.0 - wave))
            }
        };
    }
}
