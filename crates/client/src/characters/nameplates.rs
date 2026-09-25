//! Names floating above characters. The selected target's name has a
//! glowing orb on each side.

use bevy::prelude::*;

use crate::interaction::targeting::Target;

/// How far above a character's feet its name floats, in metres.
const HEIGHT: f32 = 2.15;
/// Names further than this from the camera (in metres) are hidden.
const MAX_DISTANCE: f32 = 60.0;
/// Diameter of the orbs beside the selected target's name, in pixels.
const ORB_SIZE: f32 = 8.0;
/// Colour of the orbs, and of their glow.
const ORB_COLOUR: Color = Color::srgb(1.0, 0.85, 0.4);
const ORB_GLOW: Color = Color::srgba(1.0, 0.75, 0.3, 0.8);

/// Shows names above characters.
pub struct NameplatesPlugin;

impl Plugin for NameplatesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (create_labels, place_labels, show_orbs_on_target).chain().before(TransformSystems::Propagate),
        );
    }
}

/// Gives a character a floating name.
#[derive(Component)]
pub struct Nameplate(pub String);

/// The on-screen label showing a [`Nameplate`]: a row of orb, name, orb.
#[derive(Component)]
struct NameLabel {
    owner: Entity,
}

/// The name text in a label.
#[derive(Component)]
struct LabelText;

/// An orb beside a name, shown when its character is the selected target.
#[derive(Component)]
struct TargetOrb;

fn create_labels(mut commands: Commands, new: Query<(Entity, &Nameplate), Added<Nameplate>>) {
    for (owner, nameplate) in &new {
        let orb = || {
            (
                TargetOrb,
                Node { width: px(ORB_SIZE), height: px(ORB_SIZE), border_radius: BorderRadius::MAX, ..default() },
                BackgroundColor(ORB_COLOUR),
                BoxShadow::new(ORB_GLOW, Val::ZERO, Val::ZERO, px(1), px(5)),
                Visibility::Hidden,
            )
        };
        commands
            .spawn((
                NameLabel { owner },
                Node {
                    position_type: PositionType::Absolute,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    ..default()
                },
                // Labels never block clicks on the world.
                Pickable::IGNORE,
            ))
            .with_children(|label| {
                label.spawn(orb());
                label.spawn((
                    LabelText,
                    Text::new(nameplate.0.clone()),
                    TextFont { font_size: FontSize::Px(14.0), ..default() },
                    TextColor(Color::srgb(0.95, 0.92, 0.8)),
                    TextShadow { offset: Vec2::splat(1.0), ..default() },
                ));
                label.spawn(orb());
            });
    }
}

/// Keeps each label above its character on screen, hiding it when the
/// character is gone, behind the camera or far away.
fn place_labels(
    mut commands: Commands,
    camera: Single<(&Camera, &GlobalTransform)>,
    owners: Query<(&GlobalTransform, &Nameplate)>,
    mut labels: Query<(Entity, &NameLabel, &Children, &mut Node, &mut Visibility, &ComputedNode)>,
    mut texts: Query<&mut Text, With<LabelText>>,
) {
    let (camera, camera_transform) = *camera;
    for (entity, label, children, mut node, mut visibility, computed) in &mut labels {
        let Ok((owner_transform, nameplate)) = owners.get(label.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != nameplate.0
            {
                text.0.clone_from(&nameplate.0);
            }
        }
        let head = owner_transform.translation() + Vec3::Y * HEIGHT;
        let near = head.distance(camera_transform.translation()) < MAX_DISTANCE;
        match camera.world_to_viewport(camera_transform, head).ok().filter(|_| near) {
            Some(screen) => {
                // Centre the label on the point (sizes are in physical pixels).
                let size = computed.size() * computed.inverse_scale_factor();
                node.left = Val::Px(screen.x - size.x / 2.0);
                node.top = Val::Px(screen.y - size.y);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// Shows the orbs on the selected target's label only.
fn show_orbs_on_target(
    target: Res<Target>,
    labels: Query<(&NameLabel, &Children)>,
    mut orbs: Query<&mut Visibility, With<TargetOrb>>,
) {
    for (label, children) in &labels {
        let selected = target.0 == Some(label.owner);
        for child in children.iter() {
            if let Ok(mut visibility) = orbs.get_mut(child) {
                *visibility = if selected { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}
