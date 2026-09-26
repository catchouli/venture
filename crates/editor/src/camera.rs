//! The editor's camera: orbiting a point on the ground.
//!
//! - Right mouse button: turn around the point.
//! - Middle mouse button (or Shift and the right button): slide over the ground.
//! - Scroll wheel: closer or further.
//! - W, A, S, D (or the arrow keys): move over the ground; faster with Shift.
//!
//! The world streams in around the point, as around the player in the game.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;
use venture_client::common::coords::to_bevy;
use venture_client::common::palette;
use venture_client::world::query::ground_height;
use venture_client::world::source::WorldInfo;
use venture_client::world::streaming::{ChunkStore, StreamingFocus};

use crate::shell::ViewRect;

/// Closest and furthest the camera goes from its point, in metres.
const DISTANCE: (f32, f32) = (3.0, 600.0);
/// How far the camera turns per pixel dragged, in radians.
const TURN_PER_PIXEL: f32 = 0.005;
/// How fast the keys move the point, in camera-distances per second.
const KEY_SPEED: f32 = 1.2;

/// Sets up and moves the editor's camera.
pub struct EditorCameraPlugin;

impl Plugin for EditorCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, (start_at_spawn_point, move_camera, follow_ground, place_camera).chain());
    }
}

/// The editor camera, orbiting `focus`.
#[derive(Component, Debug)]
pub struct EditorCamera {
    /// The point on the ground it looks at.
    pub focus: Vec3,
    /// Its turn about the vertical, and tilt down, in radians.
    pub yaw: f32,
    /// See `yaw`.
    pub pitch: f32,
    /// How far it is from `focus`, in metres.
    pub distance: f32,
    /// Whether it has been put at the world's spawn point yet.
    placed: bool,
}

impl EditorCamera {
    /// Looks at a point (x and z matter; it follows the ground).
    pub fn go_to(&mut self, focus: Vec3) {
        self.focus = focus;
        self.placed = true;
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("Editor camera"),
        Camera3d::default(),
        // The same haze as the game, but further out: authors look at more.
        DistanceFog {
            color: palette::HAZE,
            directional_light_color: Color::srgba(1.0, 0.9, 0.7, 0.4),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::Linear { start: 200.0, end: 900.0 },
        },
        EditorCamera { focus: Vec3::ZERO, yaw: 0.6, pitch: 0.7, distance: 60.0, placed: false },
        Transform::default(),
    ));
}

/// Once the world's description has loaded, looks at its spawn point.
fn start_at_spawn_point(world: Option<Res<WorldInfo>>, mut camera: Single<&mut EditorCamera>) {
    if let Some(world) = world
        && !camera.placed
    {
        let (x, y) = world.0.spawn_point;
        camera.focus = to_bevy(x, y, 0.0);
        camera.placed = true;
    }
}

fn move_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut EditorCamera>,
) {
    let camera = &mut **camera;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // Directions over the ground, as the camera faces.
    let forward = Vec3::new(-camera.yaw.sin(), 0.0, -camera.yaw.cos());
    let right = Vec3::new(camera.yaw.cos(), 0.0, -camera.yaw.sin());

    let in_view = window.cursor_position().is_some_and(|cursor| view.0.is_some_and(|view| view.contains(cursor)));
    if in_view && !egui.wants_pointer_input() {
        let delta = motion.delta;
        let panning = buttons.pressed(MouseButton::Middle) || (shift && buttons.pressed(MouseButton::Right));
        if panning {
            // (About one metre per pixel at 500 m away.)
            let scale = camera.distance * 0.002;
            camera.focus += (-right * delta.x + forward * delta.y) * scale;
        } else if buttons.pressed(MouseButton::Right) {
            camera.yaw -= delta.x * TURN_PER_PIXEL;
            camera.pitch = (camera.pitch + delta.y * TURN_PER_PIXEL).clamp(0.05, 1.5);
        }
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        if lines != 0.0 {
            camera.distance = (camera.distance * 0.88f32.powf(lines)).clamp(DISTANCE.0, DISTANCE.1);
        }
    }
    if view.0.is_some() && !egui.wants_keyboard_input() {
        let mut direction = Vec3::ZERO;
        for (key, other, towards) in [
            (KeyCode::KeyW, KeyCode::ArrowUp, forward),
            (KeyCode::KeyS, KeyCode::ArrowDown, -forward),
            (KeyCode::KeyD, KeyCode::ArrowRight, right),
            (KeyCode::KeyA, KeyCode::ArrowLeft, -right),
        ] {
            if keys.any_pressed([key, other]) {
                direction += towards;
            }
        }
        let speed = KEY_SPEED * camera.distance * if shift { 3.0 } else { 1.0 };
        camera.focus += direction.normalize_or_zero() * speed * time.delta_secs();
    }
}

/// Keeps the point on the ground, and the world streaming in around it.
fn follow_ground(store: Res<ChunkStore>, mut camera: Single<&mut EditorCamera>, mut focus: ResMut<StreamingFocus>) {
    if let Some(height) = ground_height(&store, camera.focus.x, camera.focus.z) {
        camera.focus.y = height;
    }
    focus.0 = camera.focus;
}

fn place_camera(mut camera: Single<(&EditorCamera, &mut Transform)>) {
    let (orbit, transform) = &mut *camera;
    let offset = Vec3::new(orbit.yaw.sin() * orbit.pitch.cos(), orbit.pitch.sin(), orbit.yaw.cos() * orbit.pitch.cos())
        * orbit.distance;
    **transform = Transform::from_translation(orbit.focus + offset).looking_at(orbit.focus, Vec3::Y);
}
