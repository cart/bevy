use bevy_ecs::{
    core::{Changed, Entity, With, Without},
    system::Query,
};

use crate::components::*;

pub fn transform_propagate_system(
    mut root_query: Query<
        (Entity, Option<&Children>, &Transform, &mut GlobalTransform),
        Without<Parent>,
    >,
    mut transform_query: Query<(&Transform, &mut GlobalTransform), With<Parent>>,
    changed_transform_query: Query<Entity, Changed<Transform>>,
    children_query: Query<Option<&Children>, (With<Parent>, With<GlobalTransform>)>,
) {
    for (entity, children, transform, mut global_transform) in root_query.iter_mut() {
        let mut changed = false;
        if changed_transform_query.get(entity).is_some() {
            *global_transform = GlobalTransform::from(*transform);
            changed = true;
        }

        if let Some(children) = children {
            for child in children.0.iter() {
                propagate_recursive(
                    &global_transform,
                    &changed_transform_query,
                    &mut transform_query,
                    &children_query,
                    *child,
                    changed,
                );
            }
        }
    }
}

fn propagate_recursive(
    parent: &GlobalTransform,
    changed_transform_query: &Query<Entity, Changed<Transform>>,
    transform_query: &mut Query<(&Transform, &mut GlobalTransform), With<Parent>>,
    children_query: &Query<Option<&Children>, (With<Parent>, With<GlobalTransform>)>,
    entity: Entity,
    mut changed: bool,
) {
    changed |= changed_transform_query.get(entity).is_some();

    let global_matrix = {
        if let Some((transform, mut global_transform)) = transform_query.get_mut(entity) {
            if changed {
                *global_transform = parent.mul_transform(*transform);
            }
            *global_transform
        } else {
            return;
        }
    };

    if let Some(Some(children)) = children_query.get(entity) {
        for child in children.0.iter() {
            propagate_recursive(
                &global_matrix,
                changed_transform_query,
                transform_query,
                children_query,
                *child,
                changed,
            );
        }
    }
}

#[cfg(test)]
mod test {
    use bevy_ecs::{
        core::World,
        schedule::{Schedule, Stage, SystemStage},
        system::{CommandQueue, Commands, IntoSystem},
    };

    use super::*;
    use crate::hierarchy::{parent_update_system, BuildChildren};

    // TODO: add world / entitymut child builder
    // #[test]
    // fn did_propagate() {
    //     let mut world = World::default();

    //     let mut update_stage = SystemStage::parallel();
    //     update_stage.add_system(parent_update_system.system());
    //     update_stage.add_system(transform_propagate_system.system());

    //     let mut schedule = Schedule::default();
    //     schedule.add_stage("update", update_stage);

    //     // Root entity
    //     world.spawn().insert_bundle((
    //         Transform::from_xyz(1.0, 0.0, 0.0),
    //         GlobalTransform::identity(),
    //     ));

    //     let mut children = Vec::new();
    //     world
    //         .spawn()
    //         .insert_bundle((
    //             Transform::from_xyz(1.0, 0.0, 0.0),
    //             GlobalTransform::identity(),
    //         ))
    //         .with_children(|parent| {
    //             parent
    //                 .spawn((
    //                     Transform::from_xyz(0.0, 2.0, 0.),
    //                     GlobalTransform::identity(),
    //                 ))
    //                 .for_current_entity(|entity| children.push(entity))
    //                 .spawn((
    //                     Transform::from_xyz(0.0, 0.0, 3.),
    //                     GlobalTransform::identity(),
    //                 ))
    //                 .for_current_entity(|entity| children.push(entity));
    //         });
    //     schedule.run(&mut world);

    //     assert_eq!(
    //         *world.get::<GlobalTransform>(children[0]).unwrap(),
    //         GlobalTransform::from_xyz(1.0, 0.0, 0.0) * Transform::from_xyz(0.0, 2.0, 0.0)
    //     );

    //     assert_eq!(
    //         *world.get::<GlobalTransform>(children[1]).unwrap(),
    //         GlobalTransform::from_xyz(1.0, 0.0, 0.0) * Transform::from_xyz(0.0, 0.0, 3.0)
    //     );
    // }

    #[test]
    fn did_propagate_command_buffer() {
        let mut world = World::default();

        let mut update_stage = SystemStage::parallel();
        update_stage.add_system(parent_update_system.system());
        update_stage.add_system(transform_propagate_system.system());

        let mut schedule = Schedule::default();
        schedule.add_stage("update", update_stage);

        // Root entity
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut children = Vec::new();
        commands
            .spawn((
                Transform::from_xyz(1.0, 0.0, 0.0),
                GlobalTransform::identity(),
            ))
            .with_children(|parent| {
                parent
                    .spawn((
                        Transform::from_xyz(0.0, 2.0, 0.0),
                        GlobalTransform::identity(),
                    ))
                    .for_current_entity(|entity| children.push(entity))
                    .spawn((
                        Transform::from_xyz(0.0, 0.0, 3.0),
                        GlobalTransform::identity(),
                    ))
                    .for_current_entity(|entity| children.push(entity));
            });
        queue.apply(&mut world);
        schedule.run(&mut world);

        assert_eq!(
            *world.get::<GlobalTransform>(children[0]).unwrap(),
            GlobalTransform::from_xyz(1.0, 0.0, 0.0) * Transform::from_xyz(0.0, 2.0, 0.0)
        );

        assert_eq!(
            *world.get::<GlobalTransform>(children[1]).unwrap(),
            GlobalTransform::from_xyz(1.0, 0.0, 0.0) * Transform::from_xyz(0.0, 0.0, 3.0)
        );
    }
}
