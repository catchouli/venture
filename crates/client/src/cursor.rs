//! The mouse cursor: an ornate gold pointer normally, and a sword when a
//! click would attack (for when targets exist).
//!
//! Both are original placeholder art drawn in code. On the web they become
//! CSS cursors, so they move as smoothly as the system cursor.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage};

use crate::procedural_image::{distance_to_polygon_edge, distance_to_segment, inside_polygon, paint_image};

/// Cursor images are this many pixels square.
const SIZE: u32 = 32;
/// Samples per pixel along each axis, for smooth edges.
const SUPERSAMPLING: u32 = 4;

/// Sets the game's cursors.
pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorStyle>().add_systems(Startup, create_cursors).add_systems(Update, apply_cursor_style);
    }
}

/// Which cursor to show.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)] // `Attack` is waiting for targeting.
pub enum CursorStyle {
    /// The normal pointer.
    #[default]
    Pointer,
    /// A sword: clicking again will attack the selected target.
    Attack,
}

#[derive(Resource)]
struct CursorImages {
    pointer: Handle<Image>,
    attack: Handle<Image>,
}

fn create_cursors(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(CursorImages {
        pointer: images.add(draw(pointer_pixel)),
        attack: images.add(draw(sword_pixel)),
    });
}

fn apply_cursor_style(
    mut commands: Commands,
    style: Res<CursorStyle>,
    images: Option<Res<CursorImages>>,
    window: Single<Entity, With<Window>>,
    mut applied: Local<Option<CursorStyle>>,
) {
    let Some(images) = images else { return };
    if *applied == Some(*style) {
        return;
    }
    let handle = match *style {
        CursorStyle::Pointer => images.pointer.clone(),
        CursorStyle::Attack => images.attack.clone(),
    };
    // Both cursors point from their top-left corner.
    let cursor = CustomCursorImage { handle, hotspot: (1, 1), ..default() };
    commands.entity(*window).insert(CursorIcon::Custom(CustomCursor::Image(cursor)));
    *applied = Some(*style);
}

/// Draws a cursor, averaging several samples per pixel for smooth edges.
/// `pixel(p)` gives the colour at point `p`, in pixels from the top-left.
fn draw(pixel: fn(Vec2) -> [f32; 4]) -> Image {
    // The cursor code reads the image on the CPU, so keep it in the main world too.
    let usage = RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD;
    paint_image(SIZE, usage, |u, v| {
        let corner = Vec2::new((u + 1.0) / 2.0, (v + 1.0) / 2.0) * SIZE as f32 - Vec2::splat(0.5);
        let mut sum = [0.0; 4];
        for sy in 0..SUPERSAMPLING {
            for sx in 0..SUPERSAMPLING {
                let offset = (Vec2::new(sx as f32, sy as f32) + 0.5) / SUPERSAMPLING as f32;
                let [r, g, b, a] = pixel(corner + offset);
                // Average premultiplied, so edges don't pick up dark fringes.
                sum = [sum[0] + r * a, sum[1] + g * a, sum[2] + b * a, sum[3] + a];
            }
        }
        let alpha = sum[3] / (SUPERSAMPLING * SUPERSAMPLING) as f32;
        let unpremultiply = |c: f32| if sum[3] > 0.0 { c / sum[3] } else { 0.0 };
        [unpremultiply(sum[0]), unpremultiply(sum[1]), unpremultiply(sum[2]), alpha]
    })
}

const OUTLINE: [f32; 4] = [0.16, 0.09, 0.04, 1.0];
const CLEAR: [f32; 4] = [0.0; 4];

/// Gold that darkens from the top-left (`t = 0`) to the bottom-right (`t = 1`).
fn gold(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [1.0 - 0.45 * t, 0.86 - 0.5 * t, 0.45 - 0.35 * t, 1.0]
}

/// The pointer: an arrow with a gold body and a dark outline.
fn pointer_pixel(p: Vec2) -> [f32; 4] {
    let arrow = [
        Vec2::new(1.5, 1.5),
        Vec2::new(1.5, 24.0),
        Vec2::new(7.0, 18.5),
        Vec2::new(11.0, 27.5),
        Vec2::new(14.5, 26.0),
        Vec2::new(10.5, 17.0),
        Vec2::new(18.5, 17.0),
    ];
    let edge = distance_to_polygon_edge(p, &arrow);
    if inside_polygon(p, &arrow) {
        if edge < 1.3 {
            return OUTLINE;
        }
        // A bright bevel just inside the top-left edges.
        let bevel = if edge < 2.4 && p.x - 1.5 < 3.5 { 0.15 } else { 0.0 };
        let shade = (p.x + p.y) / 40.0 - bevel;
        return gold(shade);
    }
    if edge < 0.7 { OUTLINE } else { CLEAR }
}

/// The sword: a silver blade from the top-left, a gold guard and pommel, and a leather grip.
fn sword_pixel(p: Vec2) -> [f32; 4] {
    let (tip, guard, grip_end) = (Vec2::new(2.0, 2.0), Vec2::new(19.5, 19.5), Vec2::new(25.5, 25.5));
    let across = Vec2::new(1.0, -1.0).normalize() * 6.0;
    let pommel = Vec2::new(27.3, 27.3);

    // Each part: its distance from `p`, its half-width and its colour.
    let blade = distance_to_segment(p, tip, guard);
    let guard_bar = distance_to_segment(p, guard - across, guard + across);
    let grip = distance_to_segment(p, guard, grip_end);
    let knob = p.distance(pommel);
    let shade = (p.x + p.y) / 56.0;
    let silver = [0.92 - 0.35 * shade, 0.94 - 0.35 * shade, 0.98 - 0.3 * shade, 1.0];
    let parts = [
        (knob, 2.4, gold(shade)),
        (guard_bar, 1.6, gold(shade)),
        (grip, 1.3, [0.35, 0.2, 0.1, 1.0]),
        // The blade tapers to the tip.
        (blade, 1.0 + 1.0 * (p.distance(tip) / tip.distance(guard)).min(1.0), silver),
    ];
    for (distance, half_width, colour) in parts {
        if distance < half_width - 0.9 {
            return colour;
        }
    }
    if parts.iter().any(|(distance, half_width, _)| *distance < *half_width + 0.3) { OUTLINE } else { CLEAR }
}
