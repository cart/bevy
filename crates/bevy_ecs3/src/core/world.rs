use crate::core::{
    entity_ref::EntityMut, ArchetypeId, Archetypes, Bundles, Component, Components, Entities,
    Entity, EntityRef, Mut, QueryFilter, QueryIter, ReadOnlyFetch, Storages, WorldQuery,
};
use std::fmt;

#[derive(Default)]
pub struct World {
    pub(crate) entities: Entities,
    pub(crate) components: Components,
    pub(crate) archetypes: Archetypes,
    pub(crate) storages: Storages,
    pub(crate) bundles: Bundles,
}

impl World {
    #[inline]
    pub fn new() -> World {
        World::default()
    }

    #[inline]
    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    #[inline]
    pub fn archetypes(&self) -> &Archetypes {
        &self.archetypes
    }

    #[inline]
    pub fn components(&self) -> &Components {
        &self.components
    }

    #[inline]
    pub fn components_mut(&mut self) -> &mut Components {
        &mut self.components
    }

    #[inline]
    pub fn storages(&self) -> &Storages {
        &self.storages
    }

    #[inline]
    pub fn bundles(&self) -> &Bundles {
        &self.bundles
    }

    #[inline]
    pub fn entity(&self, entity: Entity) -> Option<EntityRef> {
        let location = self.entities.get(entity)?;
        Some(EntityRef::new(self, entity, location))
    }

    #[inline]
    pub fn entity_mut(&mut self, entity: Entity) -> Option<EntityMut> {
        let location = self.entities.get(entity)?;
        Some(EntityMut::new(self, entity, location))
    }

    pub fn spawn(&mut self) -> EntityMut {
        self.flush();
        let entity = self.entities.alloc();
        // SAFE: empty archetype exists and no components are allocated by archetype.allocate() because the archetype is empty
        unsafe {
            let archetype = self
                .archetypes
                .get_unchecked_mut(ArchetypeId::empty_archetype());
            // PERF: avoid allocating entities in the empty archetype unless needed
            let table = self.storages.tables.get_unchecked_mut(archetype.table_id());
            let location = archetype.allocate(entity, table.allocate(entity));
            // SAFE: entity index was just allocated
            self.entities
                .meta
                .get_unchecked_mut(entity.id() as usize)
                .location = location;
            EntityMut::new(self, entity, location)
        }
    }

    #[inline]
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.entity(entity)?.get()
    }

    #[inline]
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<T>> {
        self.entity_mut(entity)?.get_mut()
    }

    #[inline]
    pub fn despawn(&mut self, entity: Entity) -> bool {
        self.entity_mut(entity)
            .map(|e| {
                e.despawn();
                true
            })
            .unwrap_or(false)
    }

    // /// Efficiently spawn a large number of entities with the same components
    // ///
    // /// Faster than calling `spawn` repeatedly with the same components.
    // ///
    // /// # Example
    // /// ```
    // /// # use bevy_ecs::*;
    // /// let mut world = World::new();
    // /// let entities = world.spawn_batch((0..1_000).map(|i| (i, "abc"))).collect::<Vec<_>>();
    // /// for i in 0..1_000 {
    // ///     assert_eq!(*world.get::<i32>(entities[i]).unwrap(), i as i32);
    // /// }
    // /// ```
    // pub fn spawn_batch<I>(&mut self, iter: I) -> SpawnBatchIter<'_, I::IntoIter>
    // where
    //     I: IntoIterator,
    //     I::Item: Bundle,
    // {
    //     // Ensure all entity allocations are accounted for so `self.entities` can realloc if
    //     // necessary
    //     self.flush();

    //     let iter = iter.into_iter();
    //     let (lower, upper) = iter.size_hint();

    //     let bundle_info = self.get_static_bundle_archetype::<I::Item>();

    //     let archetype = self.archetypes.get_mut(bundle_info.archetype).unwrap();
    //     let length = upper.unwrap_or(lower);
    //     archetype.reserve(length);
    //     self.entities.reserve(length as u32);
    //     SpawnBatchIter {
    //         inner: iter,
    //         entities: &mut self.entities,
    //         archetype_id: bundle_info.archetype,
    //         archetype,
    //     }
    // }

    /// Efficiently iterate over all entities that have certain components
    ///
    /// Calling `iter` on the returned value yields `(Entity, Q)` tuples, where `Q` is some query
    /// type. A query type is `&T`, `&mut T`, a tuple of query types, or an `Option` wrapping a
    /// query type, where `T` is any component type. Components queried with `&mut` must only appear
    /// once. Entities which do not have a component type referenced outside of an `Option` will be
    /// skipped.
    ///
    /// Entities are yielded in arbitrary order.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let a = world.spawn((123, true, "abc"));
    /// let b = world.spawn((456, false));
    /// let c = world.spawn((42, "def"));
    /// let entities = world.query::<(Entity, &i32, &bool)>()
    ///     .map(|(e, &i, &b)| (e, i, b)) // Copy out of the world
    ///     .collect::<Vec<_>>();
    /// assert_eq!(entities.len(), 2);
    /// assert!(entities.contains(&(a, 123, true)));
    /// assert!(entities.contains(&(b, 456, false)));
    /// ```
    #[inline]
    pub fn query<Q: WorldQuery>(&self) -> QueryIter<'_, Q, ()>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_unchecked() }
    }

    /// Efficiently iterate over all entities that have certain components
    ///
    /// Calling `iter` on the returned value yields `(Entity, Q)` tuples, where `Q` is some query
    /// type. A query type is `&T`, `&mut T`, a tuple of query types, or an `Option` wrapping a
    /// query type, where `T` is any component type. Components queried with `&mut` must only appear
    /// once. Entities which do not have a component type referenced outside of an `Option` will be
    /// skipped.
    ///
    /// Entities are yielded in arbitrary order.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let a = world.spawn((123, true, "abc"));
    /// let b = world.spawn((456, false));
    /// let c = world.spawn((42, "def"));
    /// let entities = world.query_mut::<(Entity, &mut i32, &bool)>()
    ///     .map(|(e, i, &b)| (e, *i, b)) // Copy out of the world
    ///     .collect::<Vec<_>>();
    /// assert_eq!(entities.len(), 2);
    /// assert!(entities.contains(&(a, 123, true)));
    /// assert!(entities.contains(&(b, 456, false)));
    /// ```
    #[inline]
    pub fn query_mut<Q: WorldQuery>(&mut self) -> QueryIter<'_, Q, ()> {
        // SAFE: unique mutable access
        unsafe { self.query_unchecked() }
    }

    /// Efficiently iterate over all entities that have certain components
    ///
    /// Calling `iter` on the returned value yields `(Entity, Q)` tuples, where `Q` is some query
    /// type. A query type is `&T`, `&mut T`, a tuple of query types, or an `Option` wrapping a
    /// query type, where `T` is any component type. Components queried with `&mut` must only appear
    /// once. Entities which do not have a component type referenced outside of an `Option` will be
    /// skipped.
    ///
    /// Entities are yielded in arbitrary order.
    ///
    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn query_unchecked<Q: WorldQuery>(&self) -> QueryIter<'_, Q, ()> {
        QueryIter::new(&self)
    }

    // /// Like `query`, but instead of returning a single iterator it returns a "batched iterator",
    // /// where each batch is `batch_size`. This is generally used for parallel iteration.
    // ///
    // /// # Safety
    // /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    // /// have unique access to the components they query.
    // #[inline]
    // pub unsafe fn query_batched_unchecked<Q: WorldQuery, F: QueryFilter>(
    //     &self,
    //     batch_size: usize,
    // ) -> BatchedIter<'_, Q, F> {
    //     BatchedIter::new(&self.archetypes, batch_size)
    // }

    pub(crate) fn flush(&mut self) {
        // SAFE: empty archetype is initialized when the world is constructed
        unsafe {
            let empty_archetype = self
                .archetypes
                .get_unchecked_mut(ArchetypeId::empty_archetype());
            let table = self
                .storages
                .tables
                .get_unchecked_mut(empty_archetype.table_id());
            // PERF: consider pre-allocating space for flushed entities
            self.entities.flush(|entity, location| {
                *location = empty_archetype.allocate(entity, table.allocate(entity));
            });
        }
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "World")
    }
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

// /// Entity IDs created by `World::spawn_batch`
// pub struct SpawnBatchIter<'a, I>
// where
//     I: Iterator,
//     I::Item: Bundle,
// {
//     inner: I,
//     entities: &'a mut Entities,
//     archetype_id: ArchetypeId,
//     archetype: &'a mut Archetype,
// }

// impl<I> Drop for SpawnBatchIter<'_, I>
// where
//     I: Iterator,
//     I::Item: Bundle,
// {
//     fn drop(&mut self) {
//         for _ in self {}
//     }
// }

// impl<I> Iterator for SpawnBatchIter<'_, I>
// where
//     I: Iterator,
//     I::Item: Bundle,
// {
//     type Item = Entity;

//     fn next(&mut self) -> Option<Entity> {
//         let components = self.inner.next()?;
//         let entity = self.entities.alloc();
//         unsafe {
//             let index = self.archetype.allocate(entity);
//             components.put(|ptr, ty, size| {
//                 self.archetype
//                     .put_component(ptr, ty, size, index, ComponentFlags::ADDED);
//                 true
//             });
//             self.entities.meta[entity.id as usize].location = EntityLocation {
//                 archetype: self.archetype_id,
//                 index,
//             };
//         }
//         Some(entity)
//     }

//     fn size_hint(&self) -> (usize, Option<usize>) {
//         self.inner.size_hint()
//     }
// }

// impl<I, T> ExactSizeIterator for SpawnBatchIter<'_, I>
// where
//     I: ExactSizeIterator<Item = T>,
//     T: Bundle,
// {
//     fn len(&self) -> usize {
//         self.inner.len()
//     }
// }
