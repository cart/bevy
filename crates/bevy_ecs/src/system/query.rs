use crate::core::{
    Component, Entity, Fetch, Mut, QueryEntityError, QueryFilter, QueryIter, QueryState,
    ReadOnlyFetch, World, WorldQuery,
};
use std::any::TypeId;
use thiserror::Error;

/// Provides scoped access to a World according to a given [WorldQuery] and [QueryFilter]
pub struct Query<'w, Q: WorldQuery, F: QueryFilter = ()> {
    pub(crate) world: &'w World,
    pub(crate) state: &'w QueryState<Q, F>,
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
    pub fn get(&self, entity: Entity) -> Result<<Q::Fetch as Fetch>::Item, QueryEntityError>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { self.state.get_unchecked_manual(self.world, entity) }
    }

    /// Gets the query result for the given `entity`
    #[inline]
    pub fn get_mut(
        &mut self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, QueryEntityError> {
        // // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { self.state.get_unchecked_manual(self.world, entity) }
    }

    /// Gets the query result for the given `entity`
    /// # Safety
    /// This allows aliased mutability. You must make sure this call does not result in multiple mutable references to the same component
    #[inline]
    pub unsafe fn get_unchecked(
        &self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, QueryEntityError> {
        // SEMI-SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        self.state.get_unchecked_manual(self.world, entity)
    }

    /// Gets a reference to the entity's component of the given type. This will fail if the entity does not have
    /// the given component type or if the given component type does not match this query.
    #[inline]
    pub fn get_component<T: Component>(&self, entity: Entity) -> Result<&T, QueryComponentError> {
        let world = self.world;
        let entity_ref = world
            .get_entity(entity)
            .ok_or(QueryComponentError::NoSuchEntity)?;
        let component_id = world
            .components()
            .get_id(TypeId::of::<T>())
            .ok_or(QueryComponentError::MissingComponent)?;
        let archetype_component = entity_ref
            .archetype()
            .get_archetype_component_id(component_id)
            .ok_or(QueryComponentError::MissingComponent)?;
        if self
            .state
            .archetype_component_access
            .has_read(archetype_component)
        {
            entity_ref
                .get::<T>()
                .ok_or(QueryComponentError::MissingComponent)
        } else {
            Err(QueryComponentError::MissingReadAccess)
        }
    }

    /// Gets a mutable reference to the entity's component of the given type. This will fail if the entity does not have
    /// the given component type or if the given component type does not match this query.
    pub fn get_component_mut<T: Component>(
        &mut self,
        entity: Entity,
    ) -> Result<Mut<'_, T>, QueryComponentError> {
        // SAFE: unique access to query (preventing aliased access)
        unsafe { self.get_component_unchecked_mut(entity) }
    }

    /// Gets a mutable reference to the entity's component of the given type. This will fail if the entity does not have
    /// the given component type or the component does not match the query.
    /// # Safety
    /// This allows aliased mutability. You must make sure this call does not result in multiple mutable references to the same component
    pub unsafe fn get_component_unchecked_mut<T: Component>(
        &self,
        entity: Entity,
    ) -> Result<Mut<'_, T>, QueryComponentError> {
        let world = self.world;
        let entity_ref = world
            .get_entity(entity)
            .ok_or(QueryComponentError::NoSuchEntity)?;
        let component_id = world
            .components()
            .get_id(TypeId::of::<T>())
            .ok_or(QueryComponentError::MissingComponent)?;
        let archetype_component = entity_ref
            .archetype()
            .get_archetype_component_id(component_id)
            .ok_or(QueryComponentError::MissingComponent)?;
        if self
            .state
            .archetype_component_access
            .has_write(archetype_component)
        {
            entity_ref
                .get_mut_unchecked::<T>()
                .ok_or(QueryComponentError::MissingComponent)
        } else {
            Err(QueryComponentError::MissingWriteAccess)
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

/// An error that occurs when retrieving a specific [Entity]'s component from a [Query]
#[derive(Error, Debug)]
pub enum QueryComponentError {
    #[error("This query does not have read access to the requested component.")]
    MissingReadAccess,
    #[error("This query does not have read access to the requested component.")]
    MissingWriteAccess,
    #[error("The given entity does not have the requested component.")]
    MissingComponent,
    #[error("The requested entity does not exist.")]
    NoSuchEntity,
}
