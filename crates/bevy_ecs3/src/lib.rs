pub mod core;
pub mod system;
pub mod schedule;

// TODO: stop using wildcards here
pub mod prelude {
    pub use crate::core::*;
    // pub use crate::resource::*;
    // pub use crate::system::*;
}

#[macro_export]
macro_rules! smaller_tuples_too {
    ($m: ident, $ty: ident) => {
        $m!{$ty}
        $m!{}
    };
    ($m: ident, $ty: ident, $($tt: ident),*) => {
        $m!{$ty, $($tt),*}
        smaller_tuples_too!{$m, $($tt),*}
    };
}

#[cfg(test)]
mod tests {
    use crate::{
        core::{
            Added, Changed, Component, Entity, IntoQueryState, Mutated, QueryFilter, With, Without,
        },
        prelude::{ComponentDescriptor, StorageType, World},
    };

    #[derive(Debug, PartialEq, Eq)]
    struct A(usize);
    struct B(usize);
    struct C;

    #[test]
    fn random_access() {
        let mut world = World::new();
        world
            .register_component(ComponentDescriptor::of::<i32>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        assert_eq!(*world.get::<&str>(e).unwrap(), "abc");
        assert_eq!(*world.get::<i32>(e).unwrap(), 123);
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);

        // test archetype get_mut()
        *world.get_mut::<&'static str>(e).unwrap() = "xyz";
        assert_eq!(*world.get::<&'static str>(e).unwrap(), "xyz");

        // test sparse set get_mut()
        *world.get_mut::<i32>(f).unwrap() = 42;
        assert_eq!(*world.get::<i32>(f).unwrap(), 42);
    }

    #[test]
    fn despawn_table_storage() {
        let mut world = World::new();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456)).id();
        assert_eq!(world.entities.len(), 2);
        assert!(world.despawn(e));
        assert_eq!(world.entities.len(), 1);
        assert!(world.get::<&str>(e).is_none());
        assert!(world.get::<i32>(e).is_none());
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);
    }

    #[test]
    fn despawn_mixed_storage() {
        let mut world = World::new();
        world
            .register_component(ComponentDescriptor::of::<i32>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456)).id();
        assert_eq!(world.entities.len(), 2);
        assert!(world.despawn(e));
        assert_eq!(world.entities.len(), 1);
        assert!(world.get::<&str>(e).is_none());
        assert!(world.get::<i32>(e).is_none());
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);
    }

    #[test]
    fn query_all() {
        let mut world = World::new();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456)).id();

        let ents = <(Entity, &i32, &&str)>::query()
            .iter(&world)
            .map(|(e, &i, &s)| (e, i, s))
            .collect::<Vec<_>>();
        assert_eq!(ents.len(), 2);
        assert!(ents.contains(&(e, 123, "abc")));
        assert!(ents.contains(&(f, 456, "def")));

        let ents = <Entity>::query().iter(&world).collect::<Vec<_>>();
        assert_eq!(ents.len(), 2);
        assert!(ents.contains(&e));
        assert!(ents.contains(&f));
    }

    #[test]
    fn query_single_component() {
        let mut world = World::new();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        let ents = <(Entity, &i32)>::query()
            .iter(&world)
            .map(|(e, &i)| (e, i))
            .collect::<Vec<_>>();
        assert_eq!(ents.len(), 2);
        assert!(ents.contains(&(e, 123)));
        assert!(ents.contains(&(f, 456)));
    }

    #[test]
    fn query_missing_component() {
        let mut world = World::new();
        world.spawn().insert_bundle(("abc", 123));
        world.spawn().insert_bundle(("def", 456));
        assert!(<(&bool, &i32)>::query().iter(&world).next().is_none());
    }

    #[test]
    fn query_sparse_component() {
        let mut world = World::new();
        world.spawn().insert_bundle(("abc", 123));
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        let ents = <(Entity, &bool)>::query()
            .iter(&world)
            .map(|(e, &b)| (e, b))
            .collect::<Vec<_>>();
        assert_eq!(ents, &[(f, true)]);
    }

    #[test]
    fn query_filter_with() {
        let mut world = World::new();
        world.spawn().insert_bundle((123u32, 1.0f32));
        world.spawn().insert(456u32);
        let result = <&u32>::query()
            .filter::<With<f32>>()
            .iter(&world)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(result, vec![123]);
    }

    #[test]
    fn query_filter_without() {
        let mut world = World::new();
        world.spawn().insert_bundle((123u32, 1.0f32));
        world.spawn().insert(456u32);
        let result = <&u32>::query()
            .filter::<Without<f32>>()
            .iter(&world)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(result, vec![456]);
    }

    #[test]
    fn query_optional_component_table() {
        let mut world = World::new();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        // this should be skipped
        world.spawn().insert("abc");
        let ents = <(Entity, Option<&bool>, &i32)>::query()
            .iter(&world)
            .map(|(e, b, &i)| (e, b.copied(), i))
            .collect::<Vec<_>>();
        assert_eq!(ents, &[(e, None, 123), (f, Some(true), 456)]);
    }

    #[test]
    fn query_optional_component_sparse() {
        let mut world = World::new();
        world
            .register_component(ComponentDescriptor::of::<bool>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        // // this should be skipped
        // world.spawn().insert("abc");
        let ents = <(Entity, Option<&bool>, &i32)>::query()
            .iter(&world)
            .map(|(e, b, &i)| (e, b.copied(), i))
            .collect::<Vec<_>>();
        assert_eq!(ents, &[(e, None, 123), (f, Some(true), 456)]);
    }

    #[test]
    fn query_optional_component_sparse_no_match() {
        let mut world = World::new();
        world
            .register_component(ComponentDescriptor::of::<bool>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456)).id();
        // // this should be skipped
        world.spawn().insert("abc");
        let ents = <(Entity, Option<&bool>, &i32)>::query()
            .iter(&world)
            .map(|(e, b, &i)| (e, b.copied(), i))
            .collect::<Vec<_>>();
        assert_eq!(ents, &[(e, None, 123), (f, None, 456)]);
    }

    #[test]
    fn add_remove_components() {
        let mut world = World::new();
        let e1 = world.spawn().insert(42).insert_bundle((true, "abc")).id();
        let e2 = world.spawn().insert(0).insert_bundle((false, "xyz")).id();

        assert_eq!(
            <(Entity, &i32, &bool)>::query()
                .iter(&world)
                .map(|(e, &i, &b)| (e, i, b))
                .collect::<Vec<_>>(),
            &[(e1, 42, true), (e2, 0, false)]
        );

        assert_eq!(world.entity_mut(e1).unwrap().remove::<i32>(), Some(42));
        assert_eq!(
            <(Entity, &i32, &bool)>::query()
                .iter(&world)
                .map(|(e, &i, &b)| (e, i, b))
                .collect::<Vec<_>>(),
            &[(e2, 0, false)]
        );
        assert_eq!(
            <(Entity, &bool, &&str)>::query()
                .iter(&world)
                .map(|(e, &b, &s)| (e, b, s))
                .collect::<Vec<_>>(),
            &[(e2, false, "xyz"), (e1, true, "abc")]
        );
        world.entity_mut(e1).unwrap().insert(43);
        assert_eq!(
            <(Entity, &i32, &bool)>::query()
                .iter(&world)
                .map(|(e, &i, &b)| (e, i, b))
                .collect::<Vec<_>>(),
            &[(e2, 0, false), (e1, 43, true)]
        );
        world.entity_mut(e1).unwrap().insert(1.0f32);
        assert_eq!(
            <(Entity, &f32)>::query()
                .iter(&world)
                .map(|(e, &f)| (e, f))
                .collect::<Vec<_>>(),
            &[(e1, 1.0)]
        );
    }

    #[test]
    fn table_add_remove_many() {
        let mut world = World::default();
        let mut entities = Vec::with_capacity(10_000);
        for _ in 0..1000 {
            entities.push(world.spawn().insert(0.0f32).id());
        }

        for (i, entity) in entities.iter().cloned().enumerate() {
            world.entity_mut(entity).unwrap().insert(i);
        }

        for (i, entity) in entities.iter().cloned().enumerate() {
            assert_eq!(world.entity_mut(entity).unwrap().remove::<usize>(), Some(i));
        }
    }

    #[test]
    fn sparse_set_add_remove_many() {
        let mut world = World::default();
        world
            .register_component(ComponentDescriptor::of::<usize>(StorageType::SparseSet))
            .unwrap();
        let mut entities = Vec::with_capacity(1000);
        for _ in 0..4 {
            entities.push(world.spawn().insert(0.0f32).id());
        }

        for (i, entity) in entities.iter().cloned().enumerate() {
            world.entity_mut(entity).unwrap().insert(i);
        }

        for (i, entity) in entities.iter().cloned().enumerate() {
            assert_eq!(world.entity_mut(entity).unwrap().remove::<usize>(), Some(i));
        }
    }

    // #[test]
    // fn clear() {
    //     let mut world = World::new();
    //     world.spawn(("abc", 123));
    //     world.spawn(("def", 456, true));
    //     world.clear();
    //     assert_eq!(world.entities().len(), 0);
    // }

    #[test]
    fn remove_missing() {
        let mut world = World::new();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        assert!(world.entity_mut(e).unwrap().remove::<bool>().is_none());
    }

    // #[test]
    // fn query_batched() {
    //     let mut world = World::new();
    //     let a = world.spawn(());
    //     let b = world.spawn(());
    //     let c = world.spawn((42,));
    //     assert_eq!(world.query_batched::<()>(1).count(), 3);
    //     assert_eq!(world.query_batched::<()>(2).count(), 2);
    //     assert_eq!(world.query_batched::<()>(2).flat_map(|x| x).count(), 3);
    //     // different archetypes are always in different batches
    //     assert_eq!(world.query_batched::<()>(3).count(), 2);
    //     assert_eq!(world.query_batched::<()>(3).flat_map(|x| x).count(), 3);
    //     assert_eq!(world.query_batched::<()>(4).count(), 2);
    //     let entities = world
    //         .query_batched::<Entity>(1)
    //         .flat_map(|x| x)
    //         .map(|e| e)
    //         .collect::<Vec<_>>();
    //     assert_eq!(entities.len(), 3);
    //     assert!(entities.contains(&a));
    //     assert!(entities.contains(&b));
    //     assert!(entities.contains(&c));
    // }

    #[test]
    fn spawn_batch() {
        let mut world = World::new();
        world.spawn_batch((0..100).map(|x| (x, "abc")));
        let values = <&i32>::query().iter(&world).map(|&x| x).collect::<Vec<_>>();
        let expected = (0..100).collect::<Vec<_>>();
        assert_eq!(values, expected);
    }

    // #[test]
    // fn query_one() {
    //     let mut world = World::new();
    //     let a = world.spawn().insert_bundle(("abc", 123)).id();
    //     let b = world.spawn().insert_bundle(("def", 456)).id();
    //     let c = world.spawn().insert_bundle(("ghi", 789, true)).id();
    //     assert_eq!(<&i32>().get(a), Some(&123));
    //     assert_eq!(<&i32>().get(b), Some(&456));
    //     assert!(<(&i32, &bool)>().get(a).is_none());
    //     assert_eq!(<(&i32, &bool)>().get(c), Some((&789, &true)));
    //     assert!(world.despawn(a));
    //     assert!(<&i32>().get(a).is_none());
    // }

    // #[test]
    // fn remove_tracking() {
    //     let mut world = World::new();
    //     let a = world.spawn(("abc", 123));
    //     let b = world.spawn(("abc", 123));

    //     world.despawn(a).unwrap();
    //     assert_eq!(
    //         world.removed::<i32>(),
    //         &[a],
    //         "despawning results in 'removed component' state"
    //     );
    //     assert_eq!(
    //         world.removed::<&'static str>(),
    //         &[a],
    //         "despawning results in 'removed component' state"
    //     );

    //     world.insert_one(b, 10.0).unwrap();
    //     assert_eq!(
    //         world.removed::<i32>(),
    //         &[a],
    //         "archetype moves does not result in 'removed component' state"
    //     );

    //     world.remove_one::<i32>(b).unwrap();
    //     assert_eq!(
    //         world.removed::<i32>(),
    //         &[a, b],
    //         "removing a component results in a 'removed component' state"
    //     );

    //     world.clear_trackers();
    //     assert_eq!(
    //         world.removed::<i32>(),
    //         &[],
    //         "clearning trackers clears removals"
    //     );
    //     assert_eq!(
    //         world.removed::<&'static str>(),
    //         &[],
    //         "clearning trackers clears removals"
    //     );
    //     assert_eq!(
    //         world.removed::<f64>(),
    //         &[],
    //         "clearning trackers clears removals"
    //     );

    //     let c = world.spawn(("abc", 123));
    //     let d = world.spawn(("abc", 123));
    //     world.clear();
    //     assert_eq!(
    //         world.removed::<i32>(),
    //         &[c, d],
    //         "world clears result in 'removed component' states"
    //     );
    //     assert_eq!(
    //         world.removed::<&'static str>(),
    //         &[c, d, b],
    //         "world clears result in 'removed component' states"
    //     );
    //     assert_eq!(
    //         world.removed::<f64>(),
    //         &[b],
    //         "world clears result in 'removed component' states"
    //     );
    // }

    #[test]
    fn added_tracking() {
        let mut world = World::new();
        let a = world.spawn().insert(123i32).id();

        assert_eq!(<&i32>::query().iter(&world).count(), 1);
        assert_eq!(<()>::query().filter::<Added<i32>>().iter(&world).count(), 1);
        assert_eq!(<&i32>::query().iter(&world).count(), 1);
        assert_eq!(<()>::query().filter::<Added<i32>>().iter(&world).count(), 1);
        assert!(<&i32>::query().get(&world, a).is_some());
        assert!(<()>::query()
            .filter::<Added<i32>>()
            .get(&world, a)
            .is_some());
        assert!(<&i32>::query().get(&world, a).is_some());
        assert!(<()>::query()
            .filter::<Added<i32>>()
            .get(&world, a)
            .is_some());

        world.clear_trackers();

        assert_eq!(<&i32>::query().iter(&world).count(), 1);
        assert_eq!(<()>::query().filter::<Added<i32>>().iter(&world).count(), 0);
        assert_eq!(<&i32>::query().iter(&world).count(), 1);
        assert_eq!(<()>::query().filter::<Added<i32>>().iter(&world).count(), 0);
        assert!(<&i32>::query().get(&world, a).is_some());
        assert!(<()>::query()
            .filter::<Added<i32>>()
            .get(&world, a)
            .is_none());
        assert!(<&i32>::query().get(&world, a).is_some());
        assert!(<()>::query()
            .filter::<Added<i32>>()
            .get(&world, a)
            .is_none());
    }

    #[test]
    fn added_queries() {
        let mut world = World::default();
        let e1 = world.spawn().insert(A(0)).id();

        fn get_added<Com: Component>(world: &World) -> Vec<Entity> {
            <Entity>::query()
                .filter::<Added<Com>>()
                .iter(&world)
                .collect::<Vec<Entity>>()
        }

        assert_eq!(get_added::<A>(&world), vec![e1]);
        world.entity_mut(e1).unwrap().insert(B(0));
        assert_eq!(get_added::<A>(&world), vec![e1]);
        assert_eq!(get_added::<B>(&world), vec![e1]);

        world.clear_trackers();
        assert!(get_added::<A>(&world).is_empty());
        let e2 = world.spawn().insert_bundle((A(1), B(1))).id();
        assert_eq!(get_added::<A>(&world), vec![e2]);
        assert_eq!(get_added::<B>(&world), vec![e2]);

        let added = <Entity>::query()
            .filter::<(Added<A>, Added<B>)>()
            .iter(&world)
            .collect::<Vec<Entity>>();
        assert_eq!(added, vec![e2]);
    }

    #[test]
    fn mutated_trackers() {
        let mut world = World::default();
        let e1 = world.spawn().insert_bundle((A(0), B(0))).id();
        let e2 = world.spawn().insert_bundle((A(0), B(0))).id();
        let e3 = world.spawn().insert_bundle((A(0), B(0))).id();
        world.spawn().insert_bundle((A(0), B));

        for (i, mut a) in <&mut A>::query().iter_mut(&mut world).enumerate() {
            if i % 2 == 0 {
                a.0 += 1;
            }
        }

        fn get_filtered<F: QueryFilter>(world: &World) -> Vec<Entity> {
            <Entity>::query()
                .filter::<F>()
                .iter(&world)
                .collect::<Vec<Entity>>()
        }

        assert_eq!(get_filtered::<Mutated<A>>(&mut world), vec![e1, e3]);

        // ensure changing an entity's archetypes also moves its mutated state
        world.entity_mut(e1).unwrap().insert(C);

        assert_eq!(get_filtered::<Mutated<A>>(&mut world), vec![e3, e1], "changed entities list should not change (although the order will due to archetype moves)");

        // spawning a new A entity should not change existing mutated state
        world.entity_mut(e1).unwrap().insert_bundle((A(0), B));
        assert_eq!(
            get_filtered::<Mutated<A>>(&mut world),
            vec![e3, e1],
            "changed entities list should not change"
        );

        // removing an unchanged entity should not change mutated state
        assert!(world.despawn(e2));
        assert_eq!(
            get_filtered::<Mutated<A>>(&mut world),
            vec![e3, e1],
            "changed entities list should not change"
        );

        // removing a changed entity should remove it from enumeration
        assert!(world.despawn(e1));
        assert_eq!(
            get_filtered::<Mutated<A>>(&mut world),
            vec![e3],
            "e1 should no longer be returned"
        );

        world.clear_trackers();

        assert!(get_filtered::<Mutated<A>>(&mut world).is_empty());

        let e4 = world.spawn().id();

        world.entity_mut(e4).unwrap().insert(A(0));
        assert!(get_filtered::<Mutated<A>>(&mut world).is_empty());
        assert_eq!(get_filtered::<Added<A>>(&mut world), vec![e4]);

        world.entity_mut(e4).unwrap().insert(A(1));
        assert_eq!(get_filtered::<Mutated<A>>(&mut world), vec![e4]);

        world.clear_trackers();

        // ensure inserting multiple components set mutated state for
        // already existing components and set added state for
        // non existing components even when changing archetype.
        world.entity_mut(e4).unwrap().insert_bundle((A(0), B(0)));

        assert!(get_filtered::<Added<A>>(&mut world).is_empty());
        assert_eq!(get_filtered::<Mutated<A>>(&mut world), vec![e4]);
        assert_eq!(get_filtered::<Added<B>>(&mut world), vec![e4]);
        assert!(get_filtered::<Mutated<B>>(&mut world).is_empty());
    }

    #[test]
    fn empty_spawn() {
        let mut world = World::default();
        let e = world.spawn().id();
        let mut e_mut = world.entity_mut(e).unwrap();
        e_mut.insert(A(0));
        assert_eq!(e_mut.get::<A>().unwrap(), &A(0));
    }

    #[test]
    fn reserve_and_spawn() {
        let mut world = World::default();
        let e = world.entities().reserve_entity();
        world.flush();
        let mut e_mut = world.entity_mut(e).unwrap();
        e_mut.insert(A(0));
        assert_eq!(e_mut.get::<A>().unwrap(), &A(0));
    }

    #[test]
    fn multiple_mutated_query() {
        let mut world = World::default();
        world.spawn().insert_bundle((A(0), B(0))).id();
        let e2 = world.spawn().insert_bundle((A(0), B(0))).id();
        world.spawn().insert_bundle((A(0), B(0)));

        for mut a in <&mut A>::query().iter_mut(&mut world) {
            a.0 += 1;
        }

        for mut b in <&mut B>::query().iter_mut(&mut world).skip(1).take(1) {
            b.0 += 1;
        }

        let a_b_mutated = <Entity>::query()
            .filter::<(Mutated<A>, Mutated<B>)>()
            .iter(&world)
            .collect::<Vec<Entity>>();
        assert_eq!(a_b_mutated, vec![e2]);
    }

    // #[test]
    // fn or_mutated_query() {
    //     let mut world = World::default();
    //     let e1 = world.spawn().insert_bundle((A(0), B(0))).id();
    //     let e2 = world.spawn().insert_bundle((A(0), B(0))).id();
    //     let e3 = world.spawn().insert_bundle((A(0), B(0))).id();
    //     world.spawn().insert_bundle((A(0), B(0)));

    //     // Mutate A in entities e1 and e2
    //     for mut a in <&mut A>().take(2) {
    //         a.0 += 1;
    //     }
    //     // Mutate B in entities e2 and e3
    //     for mut b in <&mut B>().skip(1).take(2) {
    //         b.0 += 1;
    //     }

    //     let a_b_mutated = world
    //         .query::<Entity>()
    //         .filter::<Or<(Mutated<A>, Mutated<B>)>>()
    //         .collect::<Vec<Entity>>();
    //     // e1 has mutated A, e3 has mutated B, e2 has mutated A and B, _e4 has no mutated component
    //     assert_eq!(a_b_mutated, vec![e1, e2, e3]);
    // }

    #[test]
    fn changed_query() {
        let mut world = World::default();
        let e1 = world.spawn().insert_bundle((A(0), B(0))).id();

        fn get_changed(world: &World) -> Vec<Entity> {
            <Entity>::query()
                .filter::<Changed<A>>()
                .iter(&world)
                .collect::<Vec<Entity>>()
        }
        assert_eq!(get_changed(&world), vec![e1]);
        world.clear_trackers();
        assert_eq!(get_changed(&world), vec![]);
        *world.get_mut(e1).unwrap() = A(1);
        assert_eq!(get_changed(&world), vec![e1]);
    }

    #[test]
    fn resource() {
        let mut world = World::default();
        assert!(world.get_resource::<i32>().is_none());

        world.insert_resource(123);
        assert_eq!(*world.get_resource::<i32>().expect("resource exists"), 123);

        world.insert_resource(456.0);
        assert_eq!(*world.get_resource::<f64>().expect("resource exists"), 456.0);

        world.insert_resource(789.0);
        assert_eq!(*world.get_resource::<f64>().expect("resource exists"), 789.0);

        {
            let mut value = world.get_resource_mut::<f64>().expect("resource exists");
            assert_eq!(*value, 789.0);
            *value = -1.0;
        }

        assert_eq!(*world.get_resource::<f64>().expect("resource exists"), -1.0);
    }

    // #[test]
    // fn non_send_resource() {
    //     let mut resources = Resources::default();
    //     resources.insert_non_send(123i32);
    //     resources.insert_non_send(456i64);
    //     assert_eq!(*resources.get_non_send::<i32>().unwrap(), 123);
    //     assert_eq!(*resources.get_non_send_mut::<i64>().unwrap(), 456);
    // }

    // #[test]
    // fn non_send_resource_ref_aliasing() {
    //     let mut resources = Resources::default();
    //     resources.insert_non_send(123i32);
    //     let a = resources.get_non_send::<i32>().unwrap();
    //     let b = resources.get_non_send::<i32>().unwrap();
    //     assert_eq!(*a, 123);
    //     assert_eq!(*b, 123);
    // }

    // #[test]
    // #[should_panic]
    // fn non_send_resource_mut_ref_aliasing() {
    //     let mut resources = Resources::default();
    //     resources.insert_non_send(123i32);
    //     let _a = resources.get_non_send::<i32>().unwrap();
    //     let _b = resources.get_non_send_mut::<i32>().unwrap();
    // }

    // #[test]
    // #[should_panic]
    // fn non_send_resource_panic() {
    //     let mut resources = Resources::default();
    //     resources.insert_non_send(0i32);
    //     std::thread::spawn(move || {
    //         let _ = resources.get_non_send_mut::<i32>();
    //     })
    //     .join()
    //     .unwrap();
    // }


    // #[test]
    // fn flags_query() {
    //     let mut world = World::default();
    //     let e1 = world.spawn((A(0), B(0)));
    //     world.spawn((B(0),));

    //     fn get_flags(world: &World) -> Vec<Flags<A>> {
    //         <Flags<A>>().collect::<Vec<Flags<A>>>()
    //     }
    //     let flags = get_flags(&world);
    //     assert!(flags[0].with());
    //     assert!(flags[0].added());
    //     assert!(!flags[0].mutated());
    //     assert!(flags[0].changed());
    //     assert!(!flags[1].with());
    //     assert!(!flags[1].added());
    //     assert!(!flags[1].mutated());
    //     assert!(!flags[1].changed());
    //     world.clear_trackers();
    //     let flags = get_flags(&world);
    //     assert!(flags[0].with());
    //     assert!(!flags[0].added());
    //     assert!(!flags[0].mutated());
    //     assert!(!flags[0].changed());
    //     *world.get_mut(e1).unwrap() = A(1);
    //     let flags = get_flags(&world);
    //     assert!(flags[0].with());
    //     assert!(!flags[0].added());
    //     assert!(flags[0].mutated());
    //     assert!(flags[0].changed());
    // }

    // #[test]
    // fn exact_size_query() {
    //     let mut world = World::default();
    //     world.spawn((A(0), B(0)));
    //     world.spawn((A(0), B(0)));
    //     world.spawn((C,));

    //     assert_eq!(<(&A, &B)>().len(), 2);
    //     // the following example shouldn't compile because Changed<A> is not an UnfilteredFetch
    //     // assert_eq!(<(Changed<A>, &B)>().len(), 2);
    // }

    // #[test]
    // #[cfg_attr(
    //     debug_assertions,
    //     should_panic(
    //         expected = "attempted to allocate entity with duplicate f32 components; each type must occur at most once!"
    //     )
    // )]
    // #[cfg_attr(
    //     not(debug_assertions),
    //     should_panic(
    //         expected = "attempted to allocate entity with duplicate components; each type must occur at most once!"
    //     )
    // )]
    // fn duplicate_components_panic() {
    //     let mut world = World::new();
    //     world.reserve::<(f32, i64, f32)>(1);
    // }
}
