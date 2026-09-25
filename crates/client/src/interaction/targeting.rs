//! Selecting a target: players now, and later NPCs and creatures.
//!
//! - Clicking a character selects it (see [`crate::interaction::clicks`]).
//! - The selected character's name gets an orb on each side, and the target
//!   frame at the top of the screen shows it (see [`crate::ui::target_frame`]).
//! - The frame's ✕ or Escape deselects.
//!
//! Selecting is purely a client-side choice of what to act on. Acting (such as
//! following, or later attacking) sends a command to the server.

use bevy::prelude::*;
use venture_protocol::EntityId;

use crate::characters::player::LocalPlayer;
use crate::dev::settings::DevSettings;
use crate::ui::text_input::KeyboardFocus;
use crate::world::query::raycast_ground;
use crate::world::streaming::ChunkStore;

/// Characters are picked by a vertical cylinder this tall (in metres)...
const PICK_HEIGHT: f32 = 1.9;
/// ...and this wide in radius.
const PICK_RADIUS: f32 = 0.45;

/// Keeps track of what's hovered and selected.
pub struct TargetingPlugin;

impl Plugin for TargetingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<Hovered>()
            .add_systems(PreUpdate, update_hovered)
            .add_systems(Update, (deselect_on_escape, forget_missing_target, select_nearest_if_requested));
    }
}

/// Something that can be selected: an entity in the game world.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Targetable {
    /// Its id on the server.
    pub id: EntityId,
    /// What kind of thing it is.
    pub kind: TargetKind,
}

/// What kind of thing a target is, which decides what the target frame shows
/// and what clicking it again does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Creatures are coming later.
pub enum TargetKind {
    /// Another player: clicking again follows them. Their health isn't shown.
    Player,
    /// A non-player character: clicking again talks to them. Can't be
    /// attacked, so no health is shown.
    Npc,
    /// A creature that can be fought: the target frame shows its health.
    Enemy,
}

/// How healthy a creature is, shown in the target frame.
#[derive(Component, Clone, Copy, Debug)]
#[allow(dead_code)] // Creatures are coming later.
pub struct Health {
    /// Current health.
    pub current: f32,
    /// Full health.
    pub max: f32,
}

/// A character's level (players now; creatures later), shown under their
/// name when selected.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level(pub u32);

/// The selected target, if any.
#[derive(Resource, Default)]
pub struct Target(pub Option<Entity>);

/// The targetable thing under the mouse cursor, if any.
#[derive(Resource, Default)]
pub struct Hovered(pub Option<Entity>);

/// Finds what's under the cursor each frame.
fn update_hovered(
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    store: Res<ChunkStore>,
    ui: Query<&Interaction>,
    targetables: Query<(Entity, &GlobalTransform), With<Targetable>>,
    mut hovered: ResMut<Hovered>,
) {
    let over_interface = ui.iter().any(|interaction| *interaction != Interaction::None);
    let (camera, camera_transform) = *camera;
    let ray = window.cursor_position().and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok());
    hovered.0 = match ray {
        Some(ray) if !over_interface => pick(ray, &store, &targetables),
        _ => None,
    };
}

/// The nearest targetable character the ray passes through, unless the
/// ground is in the way.
fn pick(
    ray: Ray3d,
    store: &ChunkStore,
    targetables: &Query<(Entity, &GlobalTransform), With<Targetable>>,
) -> Option<Entity> {
    let ground = raycast_ground(store, ray).map(|point| point.distance(ray.origin));
    targetables
        .iter()
        .filter_map(|(entity, transform)| {
            let feet = transform.translation();
            let distance = ray_hits_upright_cylinder(ray, feet, PICK_HEIGHT, PICK_RADIUS)?;
            ground.is_none_or(|ground| distance <= ground + PICK_RADIUS).then_some((entity, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(entity, _)| entity)
}

/// If the ray passes within `radius` of the vertical line from `base` up
/// `height` metres, how far along the ray that happens.
fn ray_hits_upright_cylinder(ray: Ray3d, base: Vec3, height: f32, radius: f32) -> Option<f32> {
    // Closest points between the ray and the (infinite) vertical axis, then clamp to the segment.
    let direction = *ray.direction;
    let to_base = ray.origin - base;
    let a = direction.dot(direction);
    let b = direction.y;
    let d = direction.dot(to_base);
    let e = to_base.y;
    let denominator = a - b * b;
    let along_ray = if denominator.abs() < 1e-6 { 0.0 } else { (b * e - d) / denominator };
    if along_ray < 0.0 {
        return None;
    }
    let up = (e + b * along_ray).clamp(0.0, height);
    let closest_on_axis = base + Vec3::Y * up;
    let closest_on_ray = ray.origin + direction * along_ray;
    (closest_on_ray.distance(closest_on_axis) <= radius).then_some(along_ray)
}

/// Escape deselects (unless it's being used to stop typing).
fn deselect_on_escape(keys: Res<ButtonInput<KeyCode>>, focus: Res<KeyboardFocus>, mut target: ResMut<Target>) {
    if keys.just_pressed(KeyCode::Escape) && !focus.is_typing() && target.0.is_some() {
        target.0 = None;
    }
}

/// Deselects a target that no longer exists (such as a player who left).
fn forget_missing_target(mut target: ResMut<Target>, targetables: Query<(), With<Targetable>>) {
    if target.0.is_some_and(|entity| !targetables.contains(entity)) {
        target.0 = None;
    }
}

/// For development: selects the character nearest the player's own, once,
/// if `VENTURE_SELECT_NEAREST` is set.
fn select_nearest_if_requested(
    dev: Res<DevSettings>,
    mut done: Local<bool>,
    player: Single<&GlobalTransform, With<LocalPlayer>>,
    targetables: Query<(Entity, &GlobalTransform), With<Targetable>>,
    mut target: ResMut<Target>,
) {
    if !dev.select_nearest || *done {
        return;
    }
    let player = player.translation();
    let nearest = targetables
        .iter()
        .min_by(|a, b| a.1.translation().distance(player).total_cmp(&b.1.translation().distance(player)));
    if let Some((entity, _)) = nearest {
        target.0 = Some(entity);
        *done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_pick_upright_cylinders() {
        let ray = Ray3d::new(Vec3::new(0.0, 1.0, -10.0), Dir3::Z);
        assert_eq!(ray_hits_upright_cylinder(ray, Vec3::ZERO, 1.9, 0.45), Some(10.0));
        assert_eq!(ray_hits_upright_cylinder(ray, Vec3::new(1.0, 0.0, 0.0), 1.9, 0.45), None);
        // Passing over the top misses.
        let high = Ray3d::new(Vec3::new(0.0, 3.0, -10.0), Dir3::Z);
        assert_eq!(ray_hits_upright_cylinder(high, Vec3::ZERO, 1.9, 0.45), None);
        // Behind the ray's start misses.
        assert_eq!(ray_hits_upright_cylinder(ray, Vec3::new(0.0, 0.0, -20.0), 1.9, 0.45), None);
    }
}
