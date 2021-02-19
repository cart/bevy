use crate::core::{Access, ArchetypeComponentId, ArchetypeGeneration, ArchetypeId, ComponentId, Entity, Fetch, FetchState, FilteredAccess, QueryFilter, QueryIter, ReadOnlyFetch, TableId, World, WorldQuery};
use fixedbitset::FixedBitSet;
use thiserror::Error;

// TODO: consider splitting out into QueryState and SystemQueryState
pub struct QueryState<Q: WorldQuery, F: QueryFilter = ()> {
    pub(crate) archetype_generation: ArchetypeGeneration,
    pub(crate) matched_tables: FixedBitSet,
    pub(crate) matched_archetypes: FixedBitSet,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) component_access: FilteredAccess<ComponentId>,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    pub(crate) matched_table_ids: Vec<TableId>,
    // NOTE: we maintain both a ArchetypeId bitset and a vec because iterating the vec is faster
    pub(crate) matched_archetype_ids: Vec<ArchetypeId>,
    pub(crate) fetch_state: Q::State,
    pub(crate) filter_state: F::State,
}

impl<Q: WorldQuery, F: QueryFilter> QueryState<Q, F> {
    pub fn new(world: &mut World) -> Self {
        let fetch_state = <Q::State as FetchState>::init(world);
        let filter_state = <F::State as FetchState>::init(world);
        let mut component_access = Default::default();
        fetch_state.update_component_access(&mut component_access);
        filter_state.update_component_access(&mut component_access);
        let mut state = Self {
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_table_ids: Vec::new(),
            matched_archetype_ids: Vec::new(),
            fetch_state,
            filter_state,
            component_access,
            matched_tables: Default::default(),
            matched_archetypes: Default::default(),
            archetype_component_access: Default::default(),
        };
        state.update_archetypes(world);
        state
    }

    pub fn update_archetypes(&mut self, world: &World) {
        let archetypes = world.archetypes();
        let old_generation = self.archetype_generation;
        let archetype_index_range = if old_generation == archetypes.generation() {
            0..0
        } else {
            self.archetype_generation = archetypes.generation();
            self.matched_tables.grow(world.storages().tables.len());
            self.matched_archetypes.grow(archetypes.len());
            if old_generation.value() == usize::MAX {
                0..archetypes.len()
            } else {
                old_generation.value()..archetypes.len()
            }
        };
        for archetype_index in archetype_index_range {
            // SAFE: archetype indices less than the archetype generation are guaranteed to exist
            let archetype = unsafe { archetypes.get_unchecked(ArchetypeId::new(archetype_index)) };
            let table_index = archetype.table_id().index();
            if self.fetch_state.matches_archetype(archetype)
                && self.filter_state.matches_archetype(archetype)
            {
                self.fetch_state.update_archetype_component_access(
                    archetype,
                    &mut self.archetype_component_access,
                );
                self.filter_state.update_archetype_component_access(
                    archetype,
                    &mut self.archetype_component_access,
                );
                self.matched_archetypes.set(archetype_index, true);
                self.matched_archetype_ids.push(archetype.id());
                if !self.matched_tables.contains(table_index) {
                    self.matched_tables.set(table_index, true);
                    self.matched_table_ids.push(archetype.table_id());
                }
            }
        }
    }

    #[inline]
    pub fn get<'w>(
        &mut self,
        world: &'w World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w>>::Item, QueryEntityError>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: query is read only
        unsafe { self.get_unchecked(world, entity) }
    }

    #[inline]
    pub fn get_mut<'w>(
        &mut self,
        world: &'w mut World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w>>::Item, QueryEntityError> {
        // SAFE: query has unique world access
        unsafe { self.get_unchecked(world, entity) }
    }

    #[inline]
    pub unsafe fn get_unchecked<'w>(
        &mut self,
        world: &'w World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w>>::Item, QueryEntityError> {
        self.update_archetypes(world);
        self.get_unchecked_manual(world, entity)
    }

    pub unsafe fn get_unchecked_manual<'w>(
        &self,
        world: &'w World,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch<'w>>::Item, QueryEntityError> {
        let location = world
            .entities
            .get(entity)
            .ok_or(QueryEntityError::NoSuchEntity)?;
        if !self
            .matched_archetypes
            .contains(location.archetype_id.index())
        {
            return Err(QueryEntityError::QueryDoesNotMatch);
        }
        // SAFE: live entities always exist in an archetype
        let archetype = world.archetypes.get_unchecked(location.archetype_id);
        let mut fetch = <Q::Fetch as Fetch>::init(world, &self.fetch_state);
        let mut filter = F::init(world, &self.filter_state);

        fetch.set_archetype(&self.fetch_state, archetype, &world.storages().tables);
        filter.set_archetype(archetype, &world.storages().tables);
        if filter.matches_archetype_entity(location.index) {
            Ok(fetch.archetype_fetch(location.index))
        } else {
            Err(QueryEntityError::QueryDoesNotMatch)
        }
    }

    #[inline]
    pub fn iter<'w, 's>(&'s mut self, world: &'w World) -> QueryIter<'w, 's, Q, F>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        self.update_archetypes(world);
        // SAFE: query is read only
        unsafe { self.iter_unchecked(world) }
    }

    #[inline]
    pub fn iter_mut<'w, 's>(&'s mut self, world: &'w mut World) -> QueryIter<'w, 's, Q, F> {
        self.update_archetypes(world);
        // SAFE: query has unique world access
        unsafe { self.iter_unchecked(world) }
    }

    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn iter_unchecked<'w, 's>(
        &'s mut self,
        world: &'w World,
    ) -> QueryIter<'w, 's, Q, F> {
        self.update_archetypes(world);
        QueryIter::new(world, self)
    }

    #[inline]
    pub unsafe fn iter_unchecked_manual<'w, 's>(
        &'s self,
        world: &'w World,
    ) -> QueryIter<'w, 's, Q, F> {
        QueryIter::new(world, self)
    }

    #[inline]
    pub fn for_each<'w>(&self, world: &'w World, func: impl FnMut(<Q::Fetch as Fetch<'w>>::Item))
    where
        Q::Fetch: ReadOnlyFetch,
    {
        unsafe {
            self.for_each_unchecked_manual(world, func);
        }
    }

    #[inline]
    pub fn for_each_mut<'w>(
        &mut self,
        world: &'w mut World,
        func: impl FnMut(<Q::Fetch as Fetch<'w>>::Item),
    ) {
        unsafe {
            self.for_each_unchecked_manual(world, func);
        }
    }

    #[inline]
    pub fn for_each_mut_manual<'w>(
        &self,
        world: &'w World,
        func: impl FnMut(<Q::Fetch as Fetch<'w>>::Item),
    ) {
        unsafe {
            self.for_each_unchecked_manual(world, func);
        }
    }

    pub unsafe fn for_each_unchecked_manual<'w, 's>(
        &'s self,
        world: &'w World,
        mut func: impl FnMut(<Q::Fetch as Fetch<'w>>::Item),
    ) {
        let mut fetch = <Q::Fetch as Fetch>::init(world, &self.fetch_state);
        let mut filter = F::init(world, &self.filter_state);
        if fetch.is_dense() && filter.is_dense() {
            let tables = &world.storages().tables;
            for table_id in self.matched_table_ids.iter() {
                let table = tables.get_unchecked(*table_id);
                fetch.set_table(&self.fetch_state, table);
                filter.set_table(table);

                for table_index in 0..table.len() {
                    if !filter.matches_table_entity(table_index) {
                        continue;
                    }
                    let item = fetch.table_fetch(table_index);
                    func(item);
                }
            }
        } else {
            let archetypes = &world.archetypes;
            let tables = &world.storages().tables;
            for archetype_id in self.matched_archetype_ids.iter() {
                let archetype = archetypes.get_unchecked(*archetype_id);
                fetch.set_archetype(&self.fetch_state, archetype, tables);
                filter.set_archetype(archetype, tables);

                for archetype_index in 0..archetype.len() {
                    if !filter.matches_archetype_entity(archetype_index) {
                        continue;
                    }
                    func(fetch.archetype_fetch(archetype_index));
                }
            }
        }
    }
}

/// An error that occurs when retrieving a specific [Entity]'s query result.
#[derive(Error, Debug)]
pub enum QueryEntityError {
    #[error("The given entity does not have the requested component.")]
    QueryDoesNotMatch,
    #[error("The requested entity does not exist.")]
    NoSuchEntity,
}
