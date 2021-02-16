mod commands;
mod exclusive_system;
mod into_system;
mod query;
#[allow(clippy::module_inception)]
mod system;
mod system_chaining;
mod system_param;

pub use commands::*;
pub use exclusive_system::*;
pub use into_system::*;
pub use query::*;
pub use system::*;
pub use system_chaining::*;
pub use system_param::*;

#[cfg(test)]
mod tests {
    use crate::{
        core::{Added, Changed, Entity, FromWorld, Mutated, Or, With, World},
        schedule::{Schedule, Stage, SystemStage},
        system::{IntoSystem, Local, Query, QuerySet, RemovedComponents, Res, ResMut, System},
    };

    #[derive(Debug, Eq, PartialEq, Default)]
    struct A;
    struct B;
    struct C;
    struct D;

    #[test]
    fn simple_system() {
        fn sys(query: Query<&A>) {
            for a in query.iter() {
                println!("{:?}", a);
            }
        }

        let mut system = sys.system();
        let mut world = World::new();
        world.spawn().insert(A);

        system.initialize(&mut world);
        system.update(&mut world);
        system.run((), &mut world);
    }

    fn run_system<S: System<In = (), Out = ()>>(world: &mut World, system: S) {
        let mut schedule = Schedule::default();
        let mut update = SystemStage::parallel();
        update.add_system(system);
        schedule.add_stage("update", update);
        schedule.run(world);
    }

    #[test]
    fn query_system_gets() {
        fn query_system(
            mut ran: ResMut<bool>,
            entity_query: Query<Entity, With<A>>,
            b_query: Query<&B>,
            a_c_query: Query<(&A, &C)>,
            d_query: Query<&D>,
        ) {
            let entities = entity_query.iter().collect::<Vec<Entity>>();
            assert!(
                b_query.get_component::<B>(entities[0]).is_err(),
                "entity 0 should not have B"
            );
            assert!(
                b_query.get_component::<B>(entities[1]).is_ok(),
                "entity 1 should have B"
            );
            assert!(
                b_query.get_component::<A>(entities[1]).is_err(),
                "entity 1 should have A, but b_query shouldn't have access to it"
            );
            assert!(
                b_query.get_component::<D>(entities[3]).is_err(),
                "entity 3 should have D, but it shouldn't be accessible from b_query"
            );
            assert!(
                b_query.get_component::<C>(entities[2]).is_err(),
                "entity 2 has C, but it shouldn't be accessible from b_query"
            );
            assert!(
                a_c_query.get_component::<C>(entities[2]).is_ok(),
                "entity 2 has C, and it should be accessible from a_c_query"
            );
            assert!(
                a_c_query.get_component::<D>(entities[3]).is_err(),
                "entity 3 should have D, but it shouldn't be accessible from b_query"
            );
            assert!(
                d_query.get_component::<D>(entities[3]).is_ok(),
                "entity 3 should have D"
            );

            *ran = true;
        }

        let mut world = World::default();
        world.insert_resource(false);
        world.spawn().insert_bundle((A,));
        world.spawn().insert_bundle((A, B));
        world.spawn().insert_bundle((A, C));
        world.spawn().insert_bundle((A, D));

        run_system(&mut world, query_system.system());

        assert!(*world.get_resource::<bool>().unwrap(), "system ran");
    }

    #[test]
    fn or_query_set_system() {
        // Regression test for issue #762
        fn query_system(
            mut ran: ResMut<bool>,
            set: QuerySet<(
                Query<(), Or<(Changed<A>, Changed<B>)>>,
                Query<(), Or<(Added<A>, Added<B>)>>,
                Query<(), Or<(Mutated<A>, Mutated<B>)>>,
            )>,
        ) {
            let changed = set.q0().iter().count();
            let added = set.q1().iter().count();
            let mutated = set.q2().iter().count();

            assert_eq!(changed, 1);
            assert_eq!(added, 1);
            assert_eq!(mutated, 0);

            *ran = true;
        }

        let mut world = World::default();
        world.insert_resource(false);
        world.spawn().insert_bundle((A, B));

        run_system(&mut world, query_system.system());

        assert!(*world.get_resource::<bool>().unwrap(), "system ran");
    }

    // #[test]
    // fn changed_resource_system() {
    //     fn incr_e_on_flip(_run_on_flip: ChangedRes<bool>, mut query: Query<&mut i32>) {
    //         for mut i in query.iter_mut() {
    //             *i += 1;
    //         }
    //     }

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(false);
    //     let ent = world.spawn((0,));

    //     let mut schedule = Schedule::default();
    //     let mut update = SystemStage::parallel();
    //     update.add_system(incr_e_on_flip.system());
    //     schedule.add_stage("update", update);
    //     schedule.add_stage(
    //         "clear_trackers",
    //         SystemStage::single(clear_trackers_system.system()),
    //     );

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     *resources.get_mut::<bool>().unwrap() = true;
    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 2);
    // }

    // #[test]
    // fn changed_resource_or_system() {
    //     fn incr_e_on_flip(
    //         _or: Or<(Option<ChangedRes<bool>>, Option<ChangedRes<i32>>)>,
    //         mut query: Query<&mut i32>,
    //     ) {
    //         for mut i in query.iter_mut() {
    //             *i += 1;
    //         }
    //     }

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(false);
    //     resources.insert::<i32>(10);
    //     let ent = world.spawn((0,));

    //     let mut schedule = Schedule::default();
    //     let mut update = SystemStage::parallel();
    //     update.add_system(incr_e_on_flip.system());
    //     schedule.add_stage("update", update);
    //     schedule.add_stage(
    //         "clear_trackers",
    //         SystemStage::single(clear_trackers_system.system()),
    //     );

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     *resources.get_mut::<bool>().unwrap() = true;
    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 2);

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 2);

    //     *resources.get_mut::<i32>().unwrap() = 20;
    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 3);
    // }

    #[test]
    #[should_panic]
    fn conflicting_query_mut_system() {
        fn sys(_q1: Query<&mut A>, _q2: Query<&mut A>) {}

        let mut world = World::default();
        world.spawn().insert(A);

        run_system(&mut world, sys.system());
    }

    #[test]
    #[should_panic]
    fn conflicting_query_immut_system() {
        fn sys(_q1: Query<&A>, _q2: Query<&mut A>) {}

        let mut world = World::default();
        world.spawn().insert(A);

        run_system(&mut world, sys.system());
    }

    #[test]
    fn query_set_system() {
        fn sys(mut _set: QuerySet<(Query<&mut A>, Query<&A>)>) {}
        let mut world = World::default();
        world.spawn().insert(A);

        run_system(&mut world, sys.system());
    }

    #[test]
    #[should_panic]
    fn conflicting_query_with_query_set_system() {
        fn sys(_query: Query<&mut A>, _set: QuerySet<(Query<&mut A>, Query<&B>)>) {}

        let mut world = World::default();
        world.spawn().insert(A);

        run_system(&mut world, sys.system());
    }

    #[test]
    #[should_panic]
    fn conflicting_query_sets_system() {
        fn sys(_set_1: QuerySet<(Query<&mut A>,)>, _set_2: QuerySet<(Query<&mut A>, Query<&B>)>) {}

        let mut world = World::default();
        world.spawn().insert(A);
        run_system(&mut world, sys.system());
    }

    #[derive(Default)]
    struct BufferRes {
        _buffer: Vec<u8>,
    }

    fn test_for_conflicting_resources<S: System<In = (), Out = ()>>(sys: S) {
        let mut world = World::default();
        world.insert_resource(BufferRes::default());
        world.insert_resource(A);
        world.insert_resource(B);
        run_system(&mut world, sys.system());
    }

    #[test]
    #[should_panic]
    fn conflicting_system_resources() {
        fn sys(_: ResMut<BufferRes>, _: Res<BufferRes>) {}
        test_for_conflicting_resources(sys.system())
    }

    #[test]
    #[should_panic]
    fn conflicting_system_resources_reverse_order() {
        fn sys(_: Res<BufferRes>, _: ResMut<BufferRes>) {}
        test_for_conflicting_resources(sys.system())
    }

    #[test]
    #[should_panic]
    fn conflicting_system_resources_multiple_mutable() {
        fn sys(_: ResMut<BufferRes>, _: ResMut<BufferRes>) {}
        test_for_conflicting_resources(sys.system())
    }

    // #[test]
    // #[should_panic]
    // fn conflicting_changed_and_mutable_resource() {
    //     // A tempting pattern, but unsound if allowed.
    //     fn sys(_: ResMut<BufferRes>, _: ChangedRes<BufferRes>) {}
    //     test_for_conflicting_resources(sys.system())
    // }

    #[test]
    fn nonconflicting_system_resources() {
        fn sys(_: Local<BufferRes>, _: ResMut<BufferRes>, _: Local<A>, _: ResMut<A>) {}
        test_for_conflicting_resources(sys.system())
    }

    #[test]
    fn local_system() {
        let mut world = World::default();
        world.insert_resource(1u32);
        world.insert_resource(false);
        struct Foo {
            value: u32,
        }

        impl FromWorld for Foo {
            fn from_world(world: &World) -> Self {
                Foo {
                    value: *world.get_resource::<u32>().unwrap() + 1,
                }
            }
        }

        fn sys(local: Local<Foo>, mut modified: ResMut<bool>) {
            assert_eq!(local.value, 2);
            *modified = true;
        }

        run_system(&mut world, sys.system());

        // ensure the system actually ran
        assert_eq!(*world.get_resource::<bool>().unwrap(), true);
    }

    #[test]
    fn remove_tracking() {
        let mut world = World::new();
        struct Despawned(Entity);
        let a = world.spawn().insert_bundle(("abc", 123)).id();
        world.spawn().insert_bundle(("abc", 123));
        world.insert_resource(false);
        world.insert_resource(Despawned(a));

        world.entity_mut(a).despawn();

        fn validate_removed(
            removed_i32: RemovedComponents<i32>,
            despawned: Res<Despawned>,
            mut ran: ResMut<bool>,
        ) {
            assert_eq!(
                removed_i32.iter().collect::<Vec<_>>(),
                &[despawned.0],
                "despawning results in 'removed component' state"
            );

            *ran = true;
        }

        run_system(&mut world, validate_removed.system());
        assert_eq!(*world.get_resource::<bool>().unwrap(), true, "system ran");
    }
}
