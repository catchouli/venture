//! Glints on trees the player can fell: while an axe is in hand, each
//! standing tree near them that their Woodworking level can fell every few
//! seconds twinkles with a small golden glint on its trunk (or a bush's
//! body), on the side facing the camera (so its own trunk never hides it),
//! fading in and out and coming back somewhere else. Easy to spot
//! when you're looking for trees to chop, but small and brief enough not to
//! be in the way. Glints keep about the same size on screen whatever their
//! distance, and are additive, bright enough to catch the bloom.

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use crate::characters::player::LocalPlayer;
use crate::common::procedural_image::paint_image;
use crate::content::ContentStore;
use crate::interaction::targeting::{Level, Targetable};
use crate::network::ClientState;
use crate::ui::character_window::Sheet;
use crate::woodcutting::axe_in_hand;
use crate::world::trees::{StandingTree, TreeDimensions};

/// Trees within this distance of the player glint, in metres.
const RANGE: f32 = 40.0;
/// Glints on each tree.
const PER_TREE: usize = 1;
/// How long a glint shows, in seconds (from nothing, to brightest, to nothing)...
const LIFE: f32 = 1.0;
/// ...and how long it's gone before it shows again, somewhere else.
const PAUSE: f32 = 2.2;
/// How big a glint is at its brightest, in metres, close up...
const SIZE: f32 = 0.18;
/// ...growing by its own size again every this many metres from the camera,
/// so far trees' glints stay about as easy to see as near ones'.
const GROWS_EVERY: f32 = 10.0;
/// How high up the trunk glints appear, in metres (from, to), and no
/// higher than this share of the tree (a bush has no trunk to speak of).
const HEIGHTS: (f32, f32) = (0.5, 2.2);
const MOST_OF_HEIGHT: f32 = 0.4;
/// How much wider than its trunk a tree is low down (its flare), for glints to sit outside.
const FLARE_ALLOWANCE: f32 = 1.5;
/// How far round the trunk from the side facing the camera a glint may be, in radians.
const FACING_SPREAD: f32 = 1.0;
/// Trees shorter than this (in metres) are bushy: glints show on their outside.
const BUSHY: f32 = 4.0;
/// Its colour (bright, so the bloom catches it).
const COLOUR: Color = Color::linear_rgb(1.4, 1.15, 0.65);

/// Glints on fellable trees.
pub struct SparklesPlugin;

impl Plugin for SparklesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_glint).add_systems(Update, glint.run_if(in_state(ClientState::Playing)));
    }
}

/// The glint's quad and material.
#[derive(Resource)]
struct GlintAssets {
    quad: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// A glint on a tree: seconds since it started (negative: not yet), and
/// where round the trunk (from -1 to 1: either side of the side facing the
/// camera) and how high (0 to 1, from the lowest a glint goes on that tree
/// to the highest).
#[derive(Component)]
struct Glint {
    tree: Entity,
    /// Counts up each time it moves, to pick its next place.
    turn: u32,
    age: f32,
    around: f32,
    height: f32,
}

/// Paints a four-pointed star with a soft glow.
fn make_glint(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let star = paint_image(64, RenderAssetUsages::RENDER_WORLD, |u, v| {
        let glow = (-(u * u + v * v) * 10.0).exp();
        let arms = (-u.abs() * 28.0).exp() * (-v.abs() * 3.0).exp() + (-v.abs() * 28.0).exp() * (-u.abs() * 3.0).exp();
        [1.0, 1.0, 1.0, (glow + arms).min(1.0)]
    });
    commands.insert_resource(GlintAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        material: materials.add(StandardMaterial {
            base_color: COLOUR,
            base_color_texture: Some(images.add(star)),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
    });
}

/// Keeps glints on the trees the player could fell right now, twinkling.
#[allow(clippy::too_many_arguments)]
fn glint(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<GlintAssets>,
    sheet: Res<Sheet>,
    content: Res<ContentStore>,
    player: Option<Single<&GlobalTransform, With<LocalPlayer>>>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    trees: Query<(Entity, &GlobalTransform, &Level, &TreeDimensions), (With<StandingTree>, With<Targetable>)>,
    mut glints: Query<(Entity, &mut Glint, &mut Transform)>,
) {
    // Which trees: with an axe in hand, those near enough and not beyond the player's level.
    let skill = sheet.0.as_ref().and_then(|sheet| sheet.skills.iter().find(|progress| progress.skill == "woodworking"));
    let level = skill.map_or(1, |progress| progress.level);
    let near = player.map(|player| player.translation()).filter(|_| axe_in_hand(&sheet, &content).is_some());
    let fellable = |entity: Entity| {
        let (_, place, tree_level, _) = trees.get(entity).ok()?;
        let near = near?;
        (tree_level.0 <= level && place.translation().distance(near) <= RANGE).then_some(())
    };
    // Glints on trees that no longer qualify go; the rest twinkle.
    let mut glinting = std::collections::HashMap::<Entity, usize>::new();
    for (entity, mut glint, mut transform) in &mut glints {
        let Some(()) = fellable(glint.tree) else {
            commands.entity(entity).despawn();
            continue;
        };
        *glinting.entry(glint.tree).or_default() += 1;
        glint.age += time.delta_secs();
        if glint.age >= LIFE + PAUSE {
            // Somewhere else round the trunk.
            glint.age = 0.0;
            glint.turn += 1;
            let seed = glint.tree.index_u32().wrapping_mul(31).wrapping_add(glint.turn * 7919);
            glint.around = scatter(seed) * 2.0 - 1.0;
            glint.height = scatter(seed ^ 0x5bd1);
        }
        let Ok((_, place, _, size)) = trees.get(glint.tree) else { continue };
        // Low on a trunk; within a bush's body (it has no trunk to speak of).
        let top = HEIGHTS.1.min(size.height * MOST_OF_HEIGHT);
        let bottom = HEIGHTS.0.min(top * 0.5);
        let height = bottom + (top - bottom) * glint.height;
        // (Outside the flare near the ground, which is wider than the trunk above.)
        let out = if size.height < BUSHY { size.height * 0.3 } else { size.trunk_radius * FLARE_ALLOWANCE + 0.05 };
        let brightness = if (0.0..LIFE).contains(&glint.age) { (PI * glint.age / LIFE).sin().powi(2) } else { 0.0 };
        // On the side facing the camera (never hidden behind its own trunk).
        let to_camera = camera.translation() - place.translation();
        let angle = to_camera.x.atan2(to_camera.z) + glint.around * FACING_SPREAD;
        let on_bark = place.translation() + Vec3::new(angle.sin() * out, height, angle.cos() * out);
        transform.translation = on_bark;
        transform.rotation = camera.rotation();
        let away = on_bark.distance(camera.translation());
        transform.scale = Vec3::splat(SIZE * (1.0 + away / GROWS_EVERY) * brightness.max(0.001));
    }
    // New glints on trees that have none yet (starting at different times, so they don't pulse together).
    let Some(near) = near else { return };
    for (tree, place, tree_level, _) in &trees {
        if tree_level.0 > level || place.translation().distance(near) > RANGE || glinting.contains_key(&tree) {
            continue;
        }
        for index in 0..PER_TREE {
            commands.spawn((
                Glint {
                    tree,
                    turn: index as u32 * 101,
                    age: -(index as f32) * (LIFE + PAUSE) / PER_TREE as f32
                        - scatter(tree.index_u32() + index as u32) * PAUSE,
                    around: scatter(tree.index_u32() * 13 + index as u32) * 2.0 - 1.0,
                    height: scatter(tree.index_u32() * 17 + index as u32),
                },
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(assets.material.clone()),
                Transform::from_scale(Vec3::splat(0.001)),
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
}

/// A number from 0 to 1, scattered by `seed` (a hash, so glints needn't share a random stream).
fn scatter(seed: u32) -> f32 {
    let mut h = seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    (h >> 8) as f32 / (1u32 << 24) as f32
}
