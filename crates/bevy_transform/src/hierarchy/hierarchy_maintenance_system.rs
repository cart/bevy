use crate::components::*;
use bevy_ecs::{Entity, Query, QueryError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddChildError {
    #[error("The given parent already has the given child.")]
    ParentAlreadyHasChild { parent: Entity, child: Entity },
    #[error("Encountered a query error.")]
    QueryError(#[from] QueryError),
}

pub trait Hierarchy {
    fn add_child(&mut self, parent: Entity, child: Entity) -> Result<(), AddChildError>;
}

type HierarchyQuery<'a, 'b, 'c, Filter = ()> =
    Query<'a, (Option<&'b mut Parent>, Option<&'c mut Children>), Filter>;

impl<'a, 'b, 'c> Hierarchy for HierarchyQuery<'a, 'b, 'c> {
    fn add_child(&mut self, parent: Entity, child: Entity) -> Result<(), AddChildError> {
        // SAFE: unique access to children and parent. no overlap
        unsafe {
            let mut child_parent = self.get_component_unsafe::<Parent>(parent)?;
            if child_parent.0 == parent {
                return Err(AddChildError::ParentAlreadyHasChild { child, parent });
            }

            let mut parent_children = self.get_component_unsafe::<Children>(parent)?;
            parent_children.0.push(child);
            child_parent.0 = parent;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{hierarchy::BuildChildren, transform_propagate_system::transform_propagate_system};
    use bevy_ecs::{Commands, IntoSystem, Res, Resources, Schedule, SystemStage, World};
    use bevy_math::Vec3;
    use smallvec::{smallvec, SmallVec};

    struct A;

    #[test]
    fn hierarchy_query_add_child() {
        let mut world = World::default();
        let mut resources = Resources::default();
        let e1 = world.spawn((A,));
        let e2 = world.spawn((A,));

        resources.insert((e1, e2));

        fn child_adder(entities: Res<(Entity, Entity)>, mut hierarchy: HierarchyQuery) {
            hierarchy.add_child(entities.0, entities.1).unwrap();
        }

        let update_stage = SystemStage::parallel().with_system(child_adder.system());
        let mut schedule = Schedule::default().with_stage("update", update_stage);
        schedule.initialize_and_run(&mut world, &mut resources);
        let expected_children: SmallVec<[Entity; 4]> = smallvec![e2];

        assert_eq!(world.get::<Parent>(e2).unwrap().0, e1);
        assert_eq!(world.get::<Children>(e1).unwrap().0, expected_children);
    }

    #[test]
    fn correct_children() {
        let mut world = World::default();
        let mut resources = Resources::default();

        let mut update_stage = SystemStage::parallel();
        update_stage.add_system(transform_propagate_system.system());

        let mut schedule = Schedule::default();
        schedule.add_stage("update", update_stage);

        // Add parent entities
        let mut commands = Commands::default();
        commands.set_entity_reserver(world.get_entity_reserver());
        let mut parent = None;
        let mut children = Vec::new();
        commands
            .spawn((Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),))
            .for_current_entity(|entity| parent = Some(entity))
            .with_children(|parent| {
                parent
                    .spawn((Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),))
                    .for_current_entity(|entity| children.push(entity))
                    .spawn((Transform::from_translation(Vec3::new(0.0, 0.0, 3.0)),))
                    .for_current_entity(|entity| children.push(entity));
            });
        let parent = parent.unwrap();
        commands.apply(&mut world, &mut resources);
        schedule.initialize_and_run(&mut world, &mut resources);

        assert_eq!(
            world
                .get::<Children>(parent)
                .unwrap()
                .0
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            children,
        );

        // Parent `e1` to `e2`.
        (*world.get_mut::<Parent>(children[0]).unwrap()).0 = children[1];

        schedule.initialize_and_run(&mut world, &mut resources);

        assert_eq!(
            world
                .get::<Children>(parent)
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![children[1]]
        );

        assert_eq!(
            world
                .get::<Children>(children[1])
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![children[0]]
        );

        world.despawn(children[0]).unwrap();

        schedule.initialize_and_run(&mut world, &mut resources);

        assert_eq!(
            world
                .get::<Children>(parent)
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![children[1]]
        );
    }
}
