use crate::core::{
    Access, ArchetypeComponentId, ArchetypeGeneration, ArchetypeId, Archetypes, ComponentId,
    Entity, Fetch, FetchState, QueryFilter, QueryIter, TableId, World, WorldQuery,
};
use fixedbitset::FixedBitSet;
use std::ops::Range;

use super::ReadOnlyFetch;

// TODO: consider splitting out into QueryState and SystemQueryState
pub struct QueryState<Q: WorldQuery, F: QueryFilter = ()> {
    pub(crate) archetype_generation: ArchetypeGeneration,
    pub(crate) matched_tables: FixedBitSet,
    pub(crate) matched_archetypes: FixedBitSet,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) component_access: Access<ComponentId>,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    pub(crate) matched_table_ids: Vec<TableId>,
    pub(crate) state: Option<(Q::State, F::State)>,
}

impl<Q: WorldQuery, F: QueryFilter> Default for QueryState<Q, F> {
    fn default() -> Self {
        Self {
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_table_ids: Vec::new(),
            state: None,
            matched_tables: Default::default(),
            matched_archetypes: Default::default(),
            archetype_component_access: Default::default(),
            component_access: Default::default(),
        }
    }
}

// TODO: try removing these and adding constraints to Q::State
/// SAFE: Q and F are markers
unsafe impl<Q: WorldQuery, F: QueryFilter> Send for QueryState<Q, F> {}
/// SAFE: Q and F are markers
unsafe impl<Q: WorldQuery, F: QueryFilter> Sync for QueryState<Q, F> {}

impl<Q: WorldQuery, F: QueryFilter> QueryState<Q, F> {
    pub fn update(&mut self, world: &World) {
        if self.state.is_none() {
            if let (Some(fetch_state), Some(filter_state)) = (
                <Q::State as FetchState>::init(world),
                <F::State as FetchState>::init(world),
            ) {
                self.component_access.grow(world.components.len());
                fetch_state.update_component_access(&mut self.component_access);
                filter_state.update_component_access(&mut self.component_access);
                self.state = Some((fetch_state, filter_state));
            }
        }
        self.update_internal(world);
    }

    fn update_internal(&mut self, world: &World) {
        if let Some((fetch_state, filter_state)) = &self.state {
            let archetypes = world.archetypes();
            let old_generation = self.archetype_generation;
            self.archetype_generation = archetypes.generation();
            self.matched_tables.grow(world.storages().tables.len());
            self.matched_archetypes.grow(archetypes.len());
            self.archetype_component_access.grow(archetypes.archetype_components_len());
            let archetype_index_range = if old_generation == self.archetype_generation {
                0..0
            } else {
                if old_generation.value() == usize::MAX {
                    0..archetypes.len()
                } else {
                    old_generation.value()..archetypes.len()
                }
            };
            for archetype_index in archetype_index_range.clone() {
                // SAFE: archetype indices less than the archetype generation are guaranteed to exist
                let archetype =
                    unsafe { archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32)) };
                let table_index = archetype.table_id().index();
                if !self.matched_tables.contains(table_index)
                    && fetch_state.matches_archetype(archetype)
                    && filter_state.matches_archetype(archetype)
                {
                    fetch_state.update_archetype_component_access(
                        archetype,
                        &mut self.archetype_component_access,
                    );
                    filter_state.update_archetype_component_access(
                        archetype,
                        &mut self.archetype_component_access,
                    );
                    self.matched_tables.set(table_index, true);
                    self.matched_archetypes.set(archetype_index, true);
                    self.matched_table_ids.push(archetype.table_id());
                }
            }
        }
    }

    pub fn filter<Filter: QueryFilter>(self) -> QueryState<Q, Filter> {
        QueryState::default()
    }

    pub fn get<'w>(
        &mut self,
        world: &'w World,
        entity: Entity,
    ) -> Option<<Q::Fetch as Fetch<'w>>::Item>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: query is read only
        unsafe { self.get_unchecked(world, entity) }
    }

    pub fn get_mut<'w>(
        &mut self,
        world: &'w mut World,
        entity: Entity,
    ) -> Option<<Q::Fetch as Fetch<'w>>::Item>
    {
        // SAFE: query has unique world access
        unsafe { self.get_unchecked(world, entity) }
    }


    pub unsafe fn get_unchecked<'w>(
        &mut self,
        world: &'w World,
        entity: Entity,
    ) -> Option<<Q::Fetch as Fetch<'w>>::Item> {
        self.update(world);
        let location = world.entities.get(entity)?;
        if !self
            .matched_archetypes
            .contains(location.archetype_id.index() as usize)
        {
            return None;
        }
        // SAFE: live entities always exist in an archetype
        let archetype = world.archetypes.get_unchecked(location.archetype_id);
        let (fetch_state, filter_state) = self.state.as_ref()?;
        let mut fetch = <Q::Fetch as Fetch>::init(world, fetch_state);
        let mut filter = F::init(world, filter_state);

        let table = world.storages.tables.get_unchecked(archetype.table_id());
        fetch.next_table(table);
        filter.next_table(table);
        let table_row = archetype.entity_table_row_unchecked(location.index);
        if filter.matches_entity(table_row) {
            Some(fetch.fetch(table_row))
        } else {
            None
        }
    }

    #[inline]
    pub fn iter<'w, 's>(&'s mut self, world: &'w World) -> QueryIter<'w, 's, Q, F>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        self.update(world);
        // SAFE: query is read only
        unsafe { self.iter_unchecked(world) }
    }

    #[inline]
    pub fn iter_mut<'w, 's>(&'s mut self, world: &'w mut World) -> QueryIter<'w, 's, Q, F> {
        self.update(world);
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
        self.update(world);
        QueryIter::new(world, self)
    }
}

pub trait IntoQueryState<Q: WorldQuery> {
    fn query() -> QueryState<Q, ()>;
}

impl<Q: WorldQuery> IntoQueryState<Q> for Q {
    fn query() -> QueryState<Q, ()> {
        QueryState::default()
    }
}
