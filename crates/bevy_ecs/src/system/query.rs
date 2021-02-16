use crate::core::{
    Component, Entity, Fetch, Mut, QueryFilter, QueryIter, QueryState, ReadOnlyFetch, World,
    WorldQuery,
};
use std::any::TypeId;

/// Provides scoped access to a World according to a given [WorldQuery] and [QueryFilter]
pub struct Query<'w, Q: WorldQuery, F: QueryFilter = ()> {
    pub(crate) world: &'w World,
    pub(crate) state: &'w QueryState<Q, F>,
}

/// An error that occurs when using a [Query]
#[derive(Debug)]
pub enum QueryError {
    CannotReadArchetype,
    CannotWriteArchetype,
    MissingComponent,
    NoSuchEntity,
}

impl<'w, Q: WorldQuery, F: QueryFilter> Query<'w, Q, F> {
    /// # Safety
    /// This will create a Query that could violate memory safety rules. Make sure that this is only called in
    /// ways that ensure the Queries have unique mutable access.
    #[inline]
    pub(crate) unsafe fn new(world: &'w World, state: &'w QueryState<Q, F>) -> Self {
        Self { world, state }
    }

    /// Iterates over the query results. This can only be called for read-only queries
    #[inline]
    pub fn iter(&self) -> QueryIter<'_, '_, Q, F>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { self.state.iter_unchecked_manual(self.world) }
    }

    /// Iterates over the query results
    #[inline]
    pub fn iter_mut(&mut self) -> QueryIter<'_, '_, Q, F> {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { self.state.iter_unchecked_manual(self.world) }
    }

    /// Iterates over the query results
    /// # Safety
    /// This allows aliased mutability. You must make sure this call does not result in multiple mutable references to the same component
    #[inline]
    pub unsafe fn iter_unsafe(&self) -> QueryIter<'_, '_, Q, F> {
        // SEMI-SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        self.state.iter_unchecked_manual(self.world)
    }

    // #[inline]
    // pub fn par_iter(&self, batch_size: usize) -> ParIter<'_, Q, F>
    // where
    //     Q::Fetch: ReadOnlyFetch,
    // {
    //     // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
    //     unsafe { ParIter::new(self.world.query_batched_unchecked(batch_size)) }
    // }

    // #[inline]
    // pub fn par_iter_mut(&mut self, batch_size: usize) -> ParIter<'_, Q, F> {
    //     // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
    //     unsafe { ParIter::new(self.world.query_batched_unchecked(batch_size)) }
    // }

    /// Gets the query result for the given `entity`
    #[inline]
    pub fn get(&self, entity: Entity) -> Option<<Q::Fetch as Fetch>::Item>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { self.state.get_unchecked_manual(self.world, entity) }
    }

    /// Gets the query result for the given `entity`
    #[inline]
    pub fn get_mut(&mut self, entity: Entity) -> Option<<Q::Fetch as Fetch>::Item> {
        // // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { self.state.get_unchecked_manual(self.world, entity) }
    }

    /// Gets the query result for the given `entity`
    /// # Safety
    /// This allows aliased mutability. You must make sure this call does not result in multiple mutable references to the same component
    #[inline]
    pub unsafe fn get_unchecked(&self, entity: Entity) -> Option<<Q::Fetch as Fetch>::Item> {
        // SEMI-SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        self.state.get_unchecked_manual(self.world, entity)
    }

    /// Gets a reference to the entity's component of the given type. This will fail if the entity does not have
    /// the given component type or if the given component type does not match this query.
    pub fn get_component<T: Component>(&self, entity: Entity) -> Option<&T> {
        let entity_ref = self.world.get_entity(entity)?;
        let component_id = self.world.components().get_id(TypeId::of::<T>())?;
        let archetype_component = entity_ref
            .archetype()
            .get_archetype_component_id(component_id)?;
        if self
            .state
            .archetype_component_access
            .has_read(archetype_component)
        {
            entity_ref.get::<T>()
        } else {
            None
        }
    }

    /// Gets a mutable reference to the entity's component of the given type. This will fail if the entity does not have
    /// the given component type or if the given component type does not match this query.
    pub fn get_component_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<'_, T>> {
        // SAFE: unique access to query (preventing aliased access)
        unsafe { self.get_component_unchecked(entity) }
    }

    /// Gets a mutable reference to the entity's component of the given type. This will fail if the entity does not have
    /// the given component type or the component does not match the query.
    /// # Safety
    /// This allows aliased mutability. You must make sure this call does not result in multiple mutable references to the same component
    pub unsafe fn get_component_unchecked<T: Component>(
        &self,
        entity: Entity,
    ) -> Option<Mut<'_, T>> {
        let entity_ref = self.world.get_entity(entity)?;
        let component_id = self.world.components().get_id(TypeId::of::<T>())?;
        let archetype_component = entity_ref
            .archetype()
            .get_archetype_component_id(component_id)?;
        if self
            .state
            .archetype_component_access
            .has_read(archetype_component)
        {
            entity_ref.get_mut_unchecked::<T>()
        } else {
            None
        }
    }
}

// /// Parallel version of QueryIter
// pub struct ParIter<'w, Q: WorldQuery, F: QueryFilter> {
//     batched_iter: BatchedIter<'w, Q, F>,
// }

// impl<'w, Q: WorldQuery, F: QueryFilter> ParIter<'w, Q, F> {
//     pub fn new(batched_iter: BatchedIter<'w, Q, F>) -> Self {
//         Self { batched_iter }
//     }
// }

// unsafe impl<'w, Q: WorldQuery, F: QueryFilter> Send for ParIter<'w, Q, F> {}

// impl<'w, Q: WorldQuery, F: QueryFilter> ParallelIterator<Batch<'w, Q, F>> for ParIter<'w, Q, F> {
//     type Item = <Q::Fetch as Fetch<'w>>::Item;

//     #[inline]
//     fn next_batch(&mut self) -> Option<Batch<'w, Q, F>> {
//         self.batched_iter.next()
//     }
// }
