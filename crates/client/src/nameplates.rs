//! Names floating above characters.

use bevy::prelude::*;

/// How far above a character's feet its name floats, in metres.
const HEIGHT: f32 = 2.15;
/// Names further than this from the camera (in metres) are hidden.
const MAX_DISTANCE: f32 = 60.0;

/// Shows names above characters.
pub struct NameplatesPlugin;

impl Plugin for NameplatesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, (create_labels, place_labels).chain().before(TransformSystems::Propagate));
    }
}

/// Gives a character a floating name.
#[derive(Component)]
pub struct Nameplate(pub String);

/// The on-screen label showing a [`Nameplate`].
#[derive(Component)]
struct NameLabel {
    owner: Entity,
}

fn create_labels(mut commands: Commands, new: Query<(Entity, &Nameplate), Added<Nameplate>>) {
    for (owner, nameplate) in &new {
        commands.spawn((
            NameLabel { owner },
            Text::new(nameplate.0.clone()),
            TextFont { font_size: FontSize::Px(14.0), ..default() },
            TextColor(Color::srgb(0.95, 0.92, 0.8)),
            TextShadow { offset: Vec2::splat(1.0), ..default() },
            Node { position_type: PositionType::Absolute, ..default() },
            // Labels never block clicks on the world.
            Pickable::IGNORE,
        ));
    }
}

/// Keeps each label above its character on screen, hiding it when the
/// character is gone, behind the camera or far away.
fn place_labels(
    mut commands: Commands,
    camera: Single<(&Camera, &GlobalTransform)>,
    owners: Query<(&GlobalTransform, &Nameplate)>,
    mut labels: Query<(Entity, &NameLabel, &mut Node, &mut Text, &mut Visibility, &ComputedNode)>,
) {
    let (camera, camera_transform) = *camera;
    for (entity, label, mut node, mut text, mut visibility, computed) in &mut labels {
        let Ok((owner_transform, nameplate)) = owners.get(label.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        if text.0 != nameplate.0 {
            text.0.clone_from(&nameplate.0);
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
