//! Selecting a target: a player, an NPC, a monster, a trap, or a tree
//! (standing or fallen). Anything that can be selected has a [`Targetable`]
//! (with its kind); new kinds of things to select (chests, say) get one too.
//! Things that aren't characters say how they're picked with a
//! [`PickShape`], and their name with a [`TargetLabel`] (they have no
//! floating name).
//!
//! - Clicking a character selects it (see [`crate::interaction::clicks`]).
//! - `/target NAME` selects the nearest NPC or monster with that name, and
//!   `/target` (or its hotbar button) the nearest monster within
//!   [`NEARBY_TARGET_RANGE`] (see [`crate::interaction::actions`]).
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

/// How near a monster must be for `/target` (without a name) to select it, in metres.
pub const NEARBY_TARGET_RANGE: f32 = 5.0;

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
pub enum TargetKind {
    /// Another player: clicking again follows them. Their health isn't shown.
    Player,
    /// A non-player character: clicking again talks to them. Can't be
    /// attacked, so no health is shown.
    Npc,
    /// A monster that can be fought: the target frame shows its health.
    Enemy,
    /// A fishing trap: clicking your own again collects it.
    Trap,
    /// A standing tree: clicking it again fells it (with an axe in hand).
    /// Trees aren't the server's entities, so their [`Targetable::id`] is
    /// [`EntityId::NONE`] (see [`crate::world::trees::StandingTree`]).
    Tree,
    /// A fallen tree: clicking it again cuts it up for logs.
    FallenTree,
}

/// How something is picked by the mouse, if not as a character (an
/// upright cylinder [`PICK_HEIGHT`] tall and [`PICK_RADIUS`] wide).
#[derive(Component, Clone, Copy, Debug)]
pub struct PickShape {
    /// Where its axis starts and ends, from where it stands (its transform), in metres.
    pub from: Vec3,
    /// (See [`PickShape::from`].)
    pub to: Vec3,
    /// How far round its axis it can be picked, in metres.
    pub radius: f32,
    /// It can't be picked from further than this from the player, in metres.
    pub within: f32,
}

/// The name shown for a target that has no floating name (a tree).
#[derive(Component, Clone, Debug)]
pub struct TargetLabel(pub String);

/// How healthy a monster is, shown in the target frame.
#[derive(Component, Clone, Copy, Debug)]
pub struct Health {
    /// Current health.
    pub current: f32,
    /// Full health.
    pub max: f32,
}

/// A character's level (players now; monsters later), shown under their
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
    targetables: Query<(Entity, &GlobalTransform, Option<&PickShape>), With<Targetable>>,
    player: Option<Single<&GlobalTransform, With<LocalPlayer>>>,
    mut hovered: ResMut<Hovered>,
) {
    let over_interface = ui.iter().any(|interaction| *interaction != Interaction::None);
    let (camera, camera_transform) = *camera;
    let ray = window.cursor_position().and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok());
    hovered.0 = match ray {
        Some(ray) if !over_interface => pick(ray, &store, &targetables, player.map(|player| player.translation())),
        _ => None,
    };
}

/// The nearest targetable thing the ray passes through, unless the ground
/// is in the way.
fn pick(
    ray: Ray3d,
    store: &ChunkStore,
    targetables: &Query<(Entity, &GlobalTransform, Option<&PickShape>), With<Targetable>>,
    player: Option<Vec3>,
) -> Option<Entity> {
    let ground = raycast_ground(store, ray).map(|point| point.distance(ray.origin));
    targetables
        .iter()
        .filter_map(|(entity, transform, shape)| {
            let place = transform.translation();
            let (distance, radius) = match shape {
                Some(shape) => {
                    if player.is_some_and(|player| player.distance(place) > shape.within) {
                        return None;
                    }
                    let (from, to) = (transform.transform_point(shape.from), transform.transform_point(shape.to));
                    (ray_hits_capsule(ray, from, to, shape.radius)?, shape.radius)
                }
                None => (ray_hits_capsule(ray, place, place + Vec3::Y * PICK_HEIGHT, PICK_RADIUS)?, PICK_RADIUS),
            };
            ground.is_none_or(|ground| distance <= ground + radius).then_some((entity, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(entity, _)| entity)
}

/// If the ray passes within `radius` of the segment from `a` to `b`, how
/// far along the ray that happens (the closest points between the ray's
/// line and the segment's).
fn ray_hits_capsule(ray: Ray3d, a: Vec3, b: Vec3, radius: f32) -> Option<f32> {
    let direction = *ray.direction;
    let axis = b - a;
    let to_a = ray.origin - a;
    let (dd, da, aa) = (direction.dot(direction), direction.dot(axis), axis.dot(axis));
    let (d_to, a_to) = (direction.dot(to_a), axis.dot(to_a));
    let denominator = dd * aa - da * da;
    // Along the segment (0 to 1) nearest the ray's line, then along the ray nearest that point.
    let along_axis = if denominator.abs() < 1e-6 || aa < 1e-9 {
        0.0
    } else {
        ((dd * a_to - da * d_to) / denominator).clamp(0.0, 1.0)
    };
    let on_axis = a + axis * along_axis;
    let along_ray = (on_axis - ray.origin).dot(direction) / dd;
    if along_ray < 0.0 {
        return None;
    }
    let on_ray = ray.origin + direction * along_ray;
    (on_ray.distance(on_axis) <= radius).then_some(along_ray)
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

/// The nearest of `candidates` (entities and where they are) to `from`,
/// and how far away it is, measured across the ground (ignoring height).
pub fn nearest(from: Vec3, candidates: impl Iterator<Item = (Entity, Vec3)>) -> Option<(Entity, f32)> {
    candidates.map(|(entity, at)| (entity, from.xz().distance(at.xz()))).min_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_is_found_across_the_ground() {
        let mut world = World::new();
        let (near, far) = (world.spawn_empty().id(), world.spawn_empty().id());
        // Height doesn't count: the far one is only above.
        let candidates = [(far, Vec3::new(6.0, 0.0, 0.0)), (near, Vec3::new(0.0, 30.0, 4.0))];
        assert_eq!(nearest(Vec3::ZERO, candidates.into_iter()), Some((near, 4.0)));
        assert_eq!(nearest(Vec3::ZERO, std::iter::empty()), None);
    }

    #[test]
    fn rays_pick_upright_and_lying_capsules() {
        let upright = |base: Vec3| (base, base + Vec3::Y * 1.9);
        let ray = Ray3d::new(Vec3::new(0.0, 1.0, -10.0), Dir3::Z);
        let (a, b) = upright(Vec3::ZERO);
        assert!((ray_hits_capsule(ray, a, b, 0.45).unwrap() - 10.0).abs() < 1e-4);
        let (a, b) = upright(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(ray_hits_capsule(ray, a, b, 0.45), None);
        // Passing over the top misses.
        let high = Ray3d::new(Vec3::new(0.0, 3.0, -10.0), Dir3::Z);
        let (a, b) = upright(Vec3::ZERO);
        assert_eq!(ray_hits_capsule(high, a, b, 0.45), None);
        // Behind the ray's start misses.
        let (a, b) = upright(Vec3::new(0.0, 0.0, -20.0));
        assert_eq!(ray_hits_capsule(ray, a, b, 0.45), None);
        // A log lying across the ray's path, and one lying along it, off to the side.
        let down = Ray3d::new(Vec3::new(2.0, 10.0, 0.0), Dir3::NEG_Y);
        assert!(ray_hits_capsule(down, Vec3::new(0.0, 0.3, 0.0), Vec3::new(5.0, 0.3, 0.0), 0.4).is_some());
        assert_eq!(ray_hits_capsule(down, Vec3::new(0.0, 0.3, 3.0), Vec3::new(5.0, 0.3, 3.0), 0.4), None);
    }
}
