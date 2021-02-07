use crate::{
    core::{
        ArchetypeGeneration, ArchetypeId, Archetypes, Entity, Fetch, QueryFilter, QueryIter,
        TableId, World, WorldQuery,
    },
    system::Query,
};
use fixedbitset::FixedBitSet;
use std::{marker::PhantomData, ops::Range};

use super::ReadOnlyFetch;

pub struct QueryState<Q: WorldQuery, F: QueryFilter> {
    pub(crate) archetype_generation: ArchetypeGeneration,
    pub(crate) matched_tables: FixedBitSet,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    pub(crate) matched_table_ids: Vec<TableId>,
    marker: PhantomData<(Q, F)>,
}

/// SAFE: Q and F are markers
unsafe impl<Q: WorldQuery, F: QueryFilter> Send for QueryState<Q, F> {}
/// SAFE: Q and F are markers
unsafe impl<Q: WorldQuery, F: QueryFilter> Sync for QueryState<Q, F> {}

impl<Q: WorldQuery, F: QueryFilter> Default for QueryState<Q, F> {
    fn default() -> Self {
        Self {
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_tables: FixedBitSet::default(),
            matched_table_ids: Vec::new(),
            marker: PhantomData,
        }
    }
}

impl<Q: WorldQuery, F: QueryFilter> QueryState<Q, F> {
    // SAFETY: this must be called on the same fetch and filter types on every call, or unsafe access could occur during iteration
    pub(crate) unsafe fn update_archetypes(&mut self, world: &World) -> Range<usize> {
        todo!("finish this");
        // let old_generation = self.archetype_generation;
        // self.archetype_generation = archetypes.generation();
        // let archetype_index_range = if old_generation == self.archetype_generation {
        //     0..0
        // } else {
        //     if old_generation.value() == usize::MAX {
        //         0..archetypes.len()
        //     } else {
        //         old_generation.value()..archetypes.len()
        //     }
        // };
        // for archetype_index in archetype_index_range.clone() {
        //     let archetype = archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32));
        //     let table_index = archetype.table_id().index();
        //     if !self.matched_tables.contains(table_index)
        //         && fetch.matches_archetype(archetype)
        //         && filter.matches_archetype(archetype)
        //     {
        //         self.matched_tables.set(table_index, true);
        //         self.matched_table_ids.push(archetype.table_id());
        //     }
        // }

        // archetype_index_range
    }

    pub fn get<'w>(
        &self,
        world: &'w World,
        entity: Entity,
    ) -> Option<<Q::Fetch as Fetch<'w>>::Item> {
        // SAFE: Queries can only be created in ways that honor rust's mutability rules. This consumes the query, which prevents aliased access.
        unsafe {
            let location = world.entities.get(entity)?;
            // SAFE: live entities always exist in an archetype
            let archetype = world.archetypes.get_unchecked(location.archetype_id);
            let mut fetch = <Q::Fetch as Fetch>::init(world).expect("unmatched fetch");
            let mut filter = F::init(world).expect("unmatched filter");
            if !fetch.matches_archetype(archetype) || !filter.matches_archetype(archetype) {
                return None;
            }

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
    }

    pub fn filter<Filter: QueryFilter>(self) -> QueryState<Q, Filter> {
        QueryState::default()
    }

    #[inline]
    pub fn iter<'w, 's>(&'s mut self, world: &'w World) -> QueryIter<'w, 's, Q, F>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { QueryIter::new(world, self) }
    }

    #[inline]
    pub fn iter_mut<'w, 's>(&'s self, world: &'w mut World) -> QueryIter<'w, 's, Q, F> {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
        unsafe { QueryIter::new(world, self) }
    }

    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn iter_unchecked<'w, 's>(&'s self, world: &'w World) -> QueryIter<'w, 's, Q, F> {
        // SAFE: system runs without conflicts with other systems. same-system queries have runtime borrow checks when they conflict
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
