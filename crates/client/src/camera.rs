//! The camera, which orbits a point it always looks at. It has two modes,
//! switched with F1:
//!
//! - **Follow** (default): orbits the player's character.
//! - **Free**: orbits a red marker sphere that flies freely over the world,
//!   for looking around.
//!
//! Controls in both modes:
//! - Right mouse drag: rotate (left-right) and tilt (up-down)
//! - Arrow keys, or Q / E and R / F: rotate and tilt
//! - Mouse wheel: zoom
//!
//! WASD runs the character (see [`crate::characters::player`]); in free mode it moves the
//! sphere instead, and Shift moves faster.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::Exposure;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal};

use crate::characters::player::{LocalPlayer, PlayerSystems};
use crate::common::coords::{from_bevy, to_bevy};
use crate::common::palette;
use crate::dev::debug_hud::DebugOverlay;
use crate::dev::settings::DevSettings;
use crate::ui::text_input::KeyboardFocus;
use crate::world::query::ground_height;
use crate::world::source::WorldInfo;
use crate::world::streaming::{ChunkStore, StreamingFocus};

/// Radius of the free camera's sphere, in metres.
const SPHERE_RADIUS: f32 = 0.5;
/// How high above the character's feet the camera looks: about chest height.
const FOLLOW_HEIGHT: f32 = 1.4;
/// Sphere movement speed at a zoom distance of 1 metre, in metres per second.
const MOVE_SPEED_PER_DISTANCE: f32 = 1.0;
/// Multiplier applied to sphere movement while Shift is held.
const FAST_MULTIPLIER: f32 = 3.0;
/// Mouse rotation speed, in radians per pixel.
const MOUSE_ROTATE_SPEED: f32 = 0.005;
/// Zoom change per scroll line, as a fraction of the current distance.
const ZOOM_STEP: f32 = 0.1;
/// Closest zoom distance, in metres.
const MIN_DISTANCE: f32 = 2.0;
/// Furthest zoom distance when following the character, in metres.
const MAX_FOLLOW_DISTANCE: f32 = 25.0;
/// Furthest zoom distance with the free camera, in metres.
const MAX_FREE_DISTANCE: f32 = 300.0;
/// The camera stays at least this far above the ground, in metres.
const CAMERA_CLEARANCE: f32 = 0.5;
/// Lowest and highest camera tilt above the horizon, in radians.
const PITCH_RANGE: (f32, f32) = (-0.2, FRAC_PI_2 - 0.01);
/// The starting view, behind and above the character like a third-person game.
const DEFAULT_VIEW: (f32, f32, f32) = (0.6, 0.4, 9.0);

/// Spawns the camera and moves it.
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>()
            .init_resource::<CameraHeading>()
            .init_resource::<CameraControls>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (toggle_mode, move_sphere, orbit_controls, zoom_controls, update_heading, place_camera)
                    .chain()
                    .after(PlayerSystems),
            );
    }
}

/// What the camera is following.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    /// The player's character.
    #[default]
    Follow,
    /// The free-flying sphere.
    Free,
}

/// How the camera responds to input. These will become player settings.
#[derive(Resource)]
pub struct CameraControls {
    /// How fast the keys rotate and tilt the camera, in radians per second.
    pub key_turn_speed: f32,
    /// Reverse which way the left/right keys (and Q/E) rotate the camera.
    pub invert_key_rotate: bool,
    /// Reverse which way the up/down keys (and R/F) tilt the camera.
    pub invert_key_tilt: bool,
    /// Reverse which way dragging left/right rotates the camera.
    pub invert_drag_rotate: bool,
    /// Reverse which way dragging up/down tilts the camera.
    pub invert_drag_tilt: bool,
}

impl Default for CameraControls {
    fn default() -> Self {
        Self {
            key_turn_speed: 1.0,
            invert_key_rotate: false,
            invert_key_tilt: false,
            invert_drag_rotate: false,
            invert_drag_tilt: false,
        }
    }
}

/// `-1.0` if `invert` is set, otherwise `1.0`.
fn sign(invert: bool) -> f32 {
    if invert { -1.0 } else { 1.0 }
}

/// The camera's heading on the ground: the horizontal directions it faces,
/// in world `(x, y)`. Used to move relative to the camera.
#[derive(Resource, Default)]
pub struct CameraHeading {
    /// Straight ahead, flattened onto the ground.
    pub forward: Vec2,
    /// To the right, flattened onto the ground.
    pub right: Vec2,
}

/// The free camera's sphere.
#[derive(Component)]
pub struct FreeCameraSphere;

/// Where the camera sits relative to what it looks at.
#[derive(Component)]
struct Orbit {
    /// Rotation around the vertical axis, in radians.
    yaw: f32,
    /// Tilt above the horizon, in radians.
    pitch: f32,
    /// Distance from the point looked at, in metres.
    distance: f32,
}

impl Orbit {
    /// Offset from the point looked at to the camera.
    fn offset(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vec3::new(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch) * self.distance
    }

    /// The horizontal direction the camera faces.
    fn forward(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    /// The horizontal direction to the camera's right.
    fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }
}

fn spawn_camera(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    dev: Res<DevSettings>,
) {
    commands.spawn((
        Name::new("Free camera sphere"),
        FreeCameraSphere,
        Mesh3d(meshes.add(Sphere::new(SPHERE_RADIUS))),
        MeshMaterial3d(materials.add(palette::TARGET_MARKER)),
        Transform::default(),
        Visibility::Hidden,
    ));

    // Placed properly by `place_camera` once there's something to look at.
    let (yaw, pitch, distance) = dev.camera_view.unwrap_or(DEFAULT_VIEW);
    let orbit = Orbit { yaw, pitch, distance };
    let focus = Vec3::ZERO;
    commands.spawn((
        Name::new("Camera"),
        Camera3d::default(),
        // The interface is drawn by this camera. (Said outright, because other
        // cameras, such as the character creation preview, draw into images;
        // left to choose, Bevy might pick one of those.)
        IsDefaultUiCamera,
        // (Set by the time of day: see `world::lighting`.)
        Exposure::default(),
        // A soft bloom, so glowing things (the move marker, lava) glow.
        // (Kept low: bloom without a threshold spreads a veil over everything
        // bright, such as the sun on water.)
        Bloom { intensity: 0.06, ..Bloom::NATURAL },
        // A touch more colour than the tonemapper leaves (it desaturates
        // bright colours).
        ColorGrading { global: ColorGradingGlobal { post_saturation: 1.1, ..default() }, ..default() },
        // Haze that thickens with distance and hides where chunks stop (it
        // takes the sky's horizon colour once the sky is up). It starts well
        // out, so the middle distance stays clear.
        DistanceFog {
            color: palette::HAZE,
            directional_light_color: Color::srgba(1.0, 0.9, 0.7, 0.4),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::Linear { start: 110.0, end: 290.0 },
        },
        Transform::from_translation(focus + orbit.offset()).looking_at(focus, Vec3::Y),
        orbit,
    ));
}

/// F1 switches between following the character and the free camera, while
/// the debug text is showing (F3); hiding it goes back to following.
fn toggle_mode(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<KeyboardFocus>,
    overlay: Res<DebugOverlay>,
    mut mode: ResMut<CameraMode>,
    camera: Single<&Transform, (With<Orbit>, Without<FreeCameraSphere>)>,
    sphere: Single<(&mut Transform, &mut Visibility), With<FreeCameraSphere>>,
    orbit: Single<&Orbit>,
) {
    let leave_free = !overlay.0 && *mode == CameraMode::Free;
    if !leave_free && (!overlay.0 || !keys.just_pressed(KeyCode::F1) || focus.is_typing()) {
        return;
    }
    let (mut sphere_transform, mut visibility) = sphere.into_inner();
    *mode = match *mode {
        CameraMode::Follow => {
            // Start flying from whatever the camera was looking at.
            let target = camera.translation - orbit.offset();
            sphere_transform.translation = target - Vec3::Y * (FOLLOW_HEIGHT - SPHERE_RADIUS);
            *visibility = Visibility::Visible;
            CameraMode::Free
        }
        CameraMode::Free => {
            *visibility = Visibility::Hidden;
            CameraMode::Follow
        }
    };
}

/// In free mode, moves the sphere across the ground with the keyboard.
fn move_sphere(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mode: Res<CameraMode>,
    focus: Res<KeyboardFocus>,
    world: Option<Res<WorldInfo>>,
    store: Res<ChunkStore>,
    orbit: Single<&Orbit>,
    mut sphere: Single<&mut Transform, With<FreeCameraSphere>>,
) {
    let Some(world) = world else { return };
    if *mode != CameraMode::Free || focus.is_typing() {
        return;
    }
    let input = wasd_direction(&keys);
    let direction = orbit.forward() * input.y + orbit.right() * input.x;

    let mut speed = MOVE_SPEED_PER_DISTANCE * orbit.distance;
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        speed *= FAST_MULTIPLIER;
    }
    let moved = sphere.translation + direction.normalize_or_zero() * speed * time.delta_secs();

    // Stay inside the world, resting on the ground.
    let size = world.0.world_size as f32;
    let (x, y) = from_bevy(moved);
    let (x, y) = (x.clamp(0.0, size), y.clamp(0.0, size));
    let height = ground_height(&store, x, y).map_or(sphere.translation.y, |ground| ground + SPHERE_RADIUS);
    sphere.translation = to_bevy(x, y, height);
}

/// Rotates and tilts the camera.
fn orbit_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    controls: Res<CameraControls>,
    focus: Res<KeyboardFocus>,
    mut orbit: Single<&mut Orbit>,
) {
    // Left/right keys orbit the camera around the target, up/down tilt it.
    let key_turn = if focus.is_typing() { 0.0 } else { controls.key_turn_speed * time.delta_secs() };
    let axis = |positive: [KeyCode; 2], negative: [KeyCode; 2]| {
        f32::from(keys.any_pressed(positive)) - f32::from(keys.any_pressed(negative))
    };
    let key_rotate = axis([KeyCode::KeyQ, KeyCode::ArrowLeft], [KeyCode::KeyE, KeyCode::ArrowRight]);
    let key_tilt = axis([KeyCode::KeyF, KeyCode::ArrowDown], [KeyCode::KeyR, KeyCode::ArrowUp]);
    orbit.yaw += key_rotate * key_turn * sign(controls.invert_key_rotate);
    orbit.pitch += key_tilt * key_turn * sign(controls.invert_key_tilt);

    if mouse_buttons.pressed(MouseButton::Right) {
        let drag = mouse_motion.delta * MOUSE_ROTATE_SPEED;
        orbit.yaw -= drag.x * sign(controls.invert_drag_rotate);
        orbit.pitch += drag.y * sign(controls.invert_drag_tilt);
    }
    orbit.pitch = orbit.pitch.clamp(PITCH_RANGE.0, PITCH_RANGE.1);
}

/// Zooms the camera with the mouse wheel. Following the character, it can't
/// zoom out as far as the free camera.
fn zoom_controls(
    mouse_scroll: Res<AccumulatedMouseScroll>,
    mode: Res<CameraMode>,
    ui: Query<&Interaction>,
    mut orbit: Single<&mut Orbit>,
) {
    // The wheel over a window (such as the inventory) scrolls the window instead.
    if ui.iter().any(|interaction| *interaction != Interaction::None) {
        return;
    }
    // Browsers report scrolling in pixels, most desktops in lines.
    let scroll_lines = match mouse_scroll.unit {
        MouseScrollUnit::Line => mouse_scroll.delta.y,
        MouseScrollUnit::Pixel => mouse_scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    let zoom = (1.0 - ZOOM_STEP).powf(scroll_lines);
    let max_distance = match *mode {
        CameraMode::Follow => MAX_FOLLOW_DISTANCE,
        CameraMode::Free => MAX_FREE_DISTANCE,
    };
    orbit.distance = (orbit.distance * zoom).clamp(MIN_DISTANCE, max_distance);
}

/// Places the camera at its orbit around what it follows (the character,
/// the free sphere, or the world's spawn point before the character
/// arrives), never below the ground, and loads chunks around that point.
fn place_camera(
    mode: Res<CameraMode>,
    world: Option<Res<WorldInfo>>,
    store: Res<ChunkStore>,
    mut focus: ResMut<StreamingFocus>,
    player: Option<Single<&Transform, (With<LocalPlayer>, Without<Orbit>, Without<FreeCameraSphere>)>>,
    sphere: Single<&Transform, (With<FreeCameraSphere>, Without<Orbit>)>,
    camera: Single<(&mut Transform, &Orbit)>,
) {
    let target = match (*mode, player) {
        (CameraMode::Free, _) => sphere.translation,
        (CameraMode::Follow, Some(player)) => player.translation + Vec3::Y * FOLLOW_HEIGHT,
        (CameraMode::Follow, None) => {
            let Some(world) = world else { return };
            let (x, y) = world.0.spawn_point;
            to_bevy(x, y, ground_height(&store, x, y).unwrap_or(0.0) + FOLLOW_HEIGHT)
        }
    };
    let (mut transform, orbit) = camera.into_inner();
    let mut position = target + orbit.offset();
    let (x, y) = from_bevy(position);
    if let Some(ground) = ground_height(&store, x, y) {
        position.y = position.y.max(ground + CAMERA_CLEARANCE);
    }
    *transform = Transform::from_translation(position).looking_at(target, Vec3::Y);
    focus.0 = target;
}

/// Publishes the camera's heading on the ground, for moving relative to it.
fn update_heading(orbit: Single<&Orbit>, mut heading: ResMut<CameraHeading>) {
    heading.forward = Vec2::from(from_bevy(orbit.forward()));
    heading.right = Vec2::from(from_bevy(orbit.right()));
}

/// The WASD keys as a direction: `x` is right (D) minus left (A), `y` is
/// forward (W) minus back (S). Diagonals are normalised, so they're no faster.
pub fn wasd_direction(keys: &ButtonInput<KeyCode>) -> Vec2 {
    let axis =
        |positive: KeyCode, negative: KeyCode| f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative));
    Vec2::new(axis(KeyCode::KeyD, KeyCode::KeyA), axis(KeyCode::KeyW, KeyCode::KeyS)).normalize_or_zero()
}
