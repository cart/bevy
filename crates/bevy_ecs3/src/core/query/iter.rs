use crate::core::{
    ArchetypeId, Archetypes, Entity, Fetch, QueryFilter, QueryState, TableId, Tables, World,
    WorldQuery,
};
use std::num::Wrapping;

/// Iterates the entities that match a given query.
/// This iterator is less efficient than StatefulQueryIter. It must scan each archetype to check for a match.
/// It won't necessarily do a linear scan of Tables, which can affect its cache-friendliness.
pub struct QueryIter<'w, Q: WorldQuery, F: QueryFilter> {
    world: &'w World,
    archetypes: &'w Archetypes,
    tables: &'w Tables,
    current_archetype: Wrapping<u32>,
    fetch: Q::Fetch,
    filter: F,
    archetype_len: usize,
    archetype_index: usize,
}

impl<'w, Q: WorldQuery, F: QueryFilter> QueryIter<'w, Q, F> {
    /// SAFETY: ensure that the given `query_state` is only used with this exact [QueryIter] type   
    pub unsafe fn with_state<'s>(
        self,
        query_state: &'s mut QueryState,
    ) -> StatefulQueryIter<'w, 's, Q, F> {
        StatefulQueryIter::new(
            self.archetypes,
            self.tables,
            self.fetch,
            self.filter,
            query_state,
        )
    }

    pub fn get(mut self, entity: Entity) -> Option<<Q::Fetch as Fetch<'w>>::Item> {
        // SAFE: Queries can only be created in ways that honor rust's mutability rules. This consumes the query, which prevents aliased access.
        unsafe {
            let location = self.world.entities.get(entity)?;
            // SAFE: live entities always exist in an archetype
            let archetype = self.archetypes.get_unchecked(location.archetype_id);
            if !self.fetch.matches_archetype(archetype) || !self.filter.matches_archetype(archetype)
            {
                return None;
            }

            let table = self.tables.get_unchecked(archetype.table_id());
            self.fetch.next_table(table);
            self.filter.next_table(table);
            let table_row = archetype.entity_table_row_unchecked(location.index);
            if self.filter.matches_entity(table_row) {
                Some(self.fetch.fetch(table_row))
            } else {
                None
            }
        }
    }
}

impl<'w, Q: WorldQuery> QueryIter<'w, Q, ()> {
    pub unsafe fn new(world: &'w World) -> Self {
        let (fetch, current_archetype) = if let Some(fetch) = <Q::Fetch as Fetch>::init(world) {
            // Start at "max" u32, so when we add 1 it will wrap around to 0
            (fetch, Wrapping(u32::MAX))
        } else {
            // could not fetch. this iterator will return None
            (
                <Q::Fetch as Fetch>::DANGLING,
                Wrapping(world.archetypes().len() as u32),
            )
        };
        QueryIter {
            world,
            fetch,
            current_archetype,
            archetypes: &world.archetypes,
            tables: &world.storages.tables,
            filter: <() as QueryFilter>::DANGLING,
            archetype_len: 0,
            archetype_index: 0,
        }
    }

    pub fn filter<F: QueryFilter>(mut self) -> QueryIter<'w, Q, F> {
        let filter = if let Some(filter) = unsafe { F::init(self.world) } {
            filter
        } else {
            self.archetype_index = self.archetype_len;
            self.current_archetype = Wrapping(self.world.archetypes().len() as u32);
            F::DANGLING
        };
        QueryIter {
            world: self.world,
            fetch: self.fetch,
            current_archetype: self.current_archetype,
            archetypes: self.archetypes,
            tables: self.tables,
            filter,
            archetype_len: self.archetype_len,
            archetype_index: self.archetype_index,
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> Iterator for QueryIter<'w, Q, F> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            loop {
                if self.archetype_index == self.archetype_len {
                    let next_index = self.current_archetype + Wrapping(1);
                    if next_index.0 as usize >= self.archetypes.len() {
                        return None;
                    }
                    self.current_archetype = next_index;
                    let archetype = self
                        .archetypes
                        .get_unchecked(ArchetypeId::new(self.current_archetype.0));
                    if !self.fetch.matches_archetype(archetype)
                        || !self.filter.matches_archetype(archetype)
                    {
                        continue;
                    }
                    let table = self.tables.get_unchecked(archetype.table_id());
                    self.fetch.next_table(table);
                    self.filter.next_table(table);
                    self.archetype_len = archetype.len();
                    self.archetype_index = 0;
                    continue;
                }

                let archetype = self
                    .archetypes
                    .get_unchecked(ArchetypeId::new(self.current_archetype.0));
                let table_row = archetype.entity_table_row_unchecked(self.archetype_index);
                if !self.filter.matches_entity(table_row) {
                    self.archetype_index += 1;
                    continue;
                }

                let item = self.fetch.fetch(table_row);
                self.archetype_index += 1;
                return Some(item);
            }
        }
    }
}

pub struct StatefulQueryIter<'w, 's, Q: WorldQuery, F: QueryFilter> {
    tables: &'w Tables,
    // TODO: try removing this for bitset iterator
    table_id_iter: std::slice::Iter<'s, TableId>,
    fetch: Q::Fetch,
    filter: F,
    table_len: usize,
    table_index: usize,
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> StatefulQueryIter<'w, 's, Q, F> {
    pub(crate) unsafe fn new(
        archetypes: &'w Archetypes,
        tables: &'w Tables,
        fetch: Q::Fetch,
        filter: F,
        query_state: &'s QueryState,
    ) -> Self {
        StatefulQueryIter {
            fetch,
            tables,
            filter,
            table_id_iter: query_state.matched_table_ids.iter(),
            table_len: 0,
            table_index: 0,
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> Iterator for StatefulQueryIter<'w, 's, Q, F> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            if self.fetch.is_dense() {
                loop {
                    if self.table_index == self.table_len {
                        let table_id = self.table_id_iter.next()?;
                        let table = self.tables.get_unchecked(*table_id);
                        self.fetch.next_table(table);
                        self.filter.next_table(table);
                        self.table_len = table.len();
                        self.table_index = 0;
                        continue;
                    }

                    if !self.filter.matches_entity(self.table_index) {
                        self.table_index += 1;
                        continue;
                    }

                    let item = self.fetch.fetch(self.table_index);

                    self.table_index += 1;
                    return Some(item);
                }
            } else {
                loop {
                    if self.table_index == self.table_len {
                        let table_id = self.table_id_iter.next()?;
                        let table = self.tables.get_unchecked(*table_id);
                        self.fetch.next_table(table);
                        self.filter.next_table(table);
                        self.table_len = table.len();
                        self.table_index = 0;
                        continue;
                    }

                    if !self.filter.matches_entity(self.table_index) {
                        self.table_index += 1;
                        continue;
                    }

                    let item = self.fetch.try_fetch(self.table_index);
                    self.table_index += 1;
                    if item.is_none() {
                        continue;
                    }
                    return item;
                }
            }
        }
    }
}
