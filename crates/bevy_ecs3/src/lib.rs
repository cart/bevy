pub mod core;

pub mod prelude {
    pub use crate::core::*;
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
        core::Entity,
        prelude::{ComponentDescriptor, StorageType, World},
    };

    #[test]
    fn random_access() {
        let mut world = World::new();
        world
            .components_mut()
            .add(ComponentDescriptor::of::<i32>(StorageType::SparseSet))
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
        assert_eq!(world.query::<()>().count(), 2);
        assert!(world.despawn(e));
        assert_eq!(world.query::<()>().count(), 1);
        assert!(world.get::<&str>(e).is_none());
        assert!(world.get::<i32>(e).is_none());
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);
    }

    #[test]
    fn despawn_mixed_storage() {
        let mut world = World::new();
        world
            .components_mut()
            .add(ComponentDescriptor::of::<i32>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456)).id();
        assert_eq!(world.query::<()>().count(), 2);
        assert!(world.despawn(e));
        assert_eq!(world.query::<()>().count(), 1);
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

        let ents = world
            .query::<(Entity, &i32, &&str)>()
            .map(|(e, &i, &s)| (e, i, s))
            .collect::<Vec<_>>();
        assert_eq!(ents.len(), 2);
        assert!(ents.contains(&(e, 123, "abc")));
        assert!(ents.contains(&(f, 456, "def")));

        let ents = world.query::<Entity>().collect::<Vec<_>>();
        assert_eq!(ents.len(), 2);
        assert!(ents.contains(&e));
        assert!(ents.contains(&f));
    }

    #[test]
    fn query_single_component() {
        let mut world = World::new();
        let e = world.spawn().insert_bundle(("abc", 123)).id();
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        let ents = world
            .query::<(Entity, &i32)>()
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
        assert!(world.query::<(&bool, &i32)>().next().is_none());
    }

    #[test]
    fn query_sparse_component() {
        let mut world = World::new();
        world.spawn().insert_bundle(("abc", 123));
        let f = world.spawn().insert_bundle(("def", 456, true)).id();
        let ents = world
            .query::<(Entity, &bool)>()
            .map(|(e, &b)| (e, b))
            .collect::<Vec<_>>();
        assert_eq!(ents, &[(f, true)]);
    }

    // #[test]
    // fn query_optional_component() {
    //     let mut world = World::new();
    //     let e = world.spawn(("abc", 123));
    //     let f = world.spawn(("def", 456, true));
    //     let ents = world
    //         .query::<(Entity, Option<&bool>, &i32)>()
    //         .map(|(e, b, &i)| (e, b.copied(), i))
    //         .collect::<Vec<_>>();
    //     assert_eq!(ents.len(), 2);
    //     assert!(ents.contains(&(e, None, 123)));
    //     assert!(ents.contains(&(f, Some(true), 456)));
    // }

    #[test]
    fn add_remove_components() {
        let mut world = World::new();
        let e1 = world.spawn().insert(42).insert_bundle((true, "abc")).id();
        let e2 = world.spawn().insert(0).insert_bundle((false, "xyz")).id();
        assert_eq!(
            world
                .query::<(Entity, &i32, &bool)>()
                .map(|(e, &i, &b)| (e, i, b))
                .collect::<Vec<_>>(),
            &[(e1, 42, true), (e2, 0, false)]
        );

        assert_eq!(world.entity_mut(e1).unwrap().remove::<i32>(), Some(42));
        assert_eq!(
            world
                .query::<(Entity, &i32, &bool)>()
                .map(|(e, &i, &b)| (e, i, b))
                .collect::<Vec<_>>(),
            &[(e2, 0, false)]
        );
        assert_eq!(
            world
                .query::<(Entity, &bool, &&str)>()
                .map(|(e, &b, &s)| (e, b, s))
                .collect::<Vec<_>>(),
            &[(e2, false, "xyz"), (e1, true, "abc")]
        );
        world.entity_mut(e1).unwrap().insert(43);
        assert_eq!(
            world
                .query::<(Entity, &i32, &bool)>()
                .map(|(e, &i, &b)| (e, i, b))
                .collect::<Vec<_>>(),
            &[(e2, 0, false), (e1, 43, true)]
        );
        world.entity_mut(e1).unwrap().insert(1.0f32);
        assert_eq!(
            world
                .query::<(Entity, &f32)>()
                .map(|(e, &f)| (e, f))
                .collect::<Vec<_>>(),
            &[(e1, 1.0)]
        );
    }

    #[test]
    fn add_remove_many_table() {
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
    fn add_remove_many_sparse_set() {
        let mut world = World::default();
        world
            .components_mut()
            .add(ComponentDescriptor::of::<f32>(StorageType::SparseSet))
            .unwrap();
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

    // #[test]
    // fn spawn_batch() {
    //     let mut world = World::new();
    //     world.spawn_batch((0..100).map(|x| (x, "abc")));
    //     let entities = world.query::<&i32>().map(|&x| x).collect::<Vec<_>>();
    //     assert_eq!(entities.len(), 100);
    // }

    // #[test]
    // fn query_one() {
    //     let mut world = World::new();
    //     let a = world.spawn(("abc", 123));
    //     let b = world.spawn(("def", 456));
    //     let c = world.spawn(("ghi", 789, true));
    //     assert_eq!(world.query_one::<&i32>(a), Ok(&123));
    //     assert_eq!(world.query_one::<&i32>(b), Ok(&456));
    //     assert!(world.query_one::<(&i32, &bool)>(a).is_err());
    //     assert_eq!(world.query_one::<(&i32, &bool)>(c), Ok((&789, &true)));
    //     world.despawn(a).unwrap();
    //     assert!(world.query_one::<&i32>(a).is_err());
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

    // #[test]
    // fn added_tracking() {
    //     let mut world = World::new();
    //     let a = world.spawn((123,));

    //     assert_eq!(world.query::<&i32>().count(), 1);
    //     assert_eq!(world.query_filtered::<(), Added<i32>>().count(), 1);
    //     assert_eq!(world.query_mut::<&i32>().count(), 1);
    //     assert_eq!(world.query_filtered_mut::<(), Added<i32>>().count(), 1);
    //     assert!(world.query_one::<&i32>(a).is_ok());
    //     assert!(world.query_one_filtered::<(), Added<i32>>(a).is_ok());
    //     assert!(world.query_one_mut::<&i32>(a).is_ok());
    //     assert!(world.query_one_filtered_mut::<(), Added<i32>>(a).is_ok());

    //     world.clear_trackers();

    //     assert_eq!(world.query::<&i32>().count(), 1);
    //     assert_eq!(world.query_filtered::<(), Added<i32>>().count(), 0);
    //     assert_eq!(world.query_mut::<&i32>().count(), 1);
    //     assert_eq!(world.query_filtered_mut::<(), Added<i32>>().count(), 0);
    //     assert!(world.query_one_mut::<&i32>(a).is_ok());
    //     assert!(world.query_one_filtered_mut::<(), Added<i32>>(a).is_err());
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
