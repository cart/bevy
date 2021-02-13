use crate::core::{
    ArchetypeId, Archetypes, Fetch, QueryFilter, QueryState, TableId, Tables, World, WorldQuery,
};

pub struct QueryIter<'w, 's, Q: WorldQuery, F: QueryFilter> {
    tables: &'w Tables,
    archetypes: &'w Archetypes,
    // TODO: try removing this for bitset iterator
    query_state: &'s QueryState<Q, F>,
    world: &'w World,
    table_id_iter: std::slice::Iter<'s, TableId>,
    archetype_id_iter: std::slice::Iter<'s, ArchetypeId>,
    fetch: Q::Fetch,
    filter: F,
    pub(crate) is_dense: bool,
    current_len: usize,
    current_index: usize,
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> QueryIter<'w, 's, Q, F> {
    pub(crate) unsafe fn new(world: &'w World, query_state: &'s QueryState<Q, F>) -> Self {
        let fetch = <Q::Fetch as Fetch>::init(world, &query_state.fetch_state);
        let filter = F::init(world, &query_state.filter_state);
        QueryIter {
            is_dense: fetch.is_dense() && filter.is_dense(),
            world,
            query_state,
            fetch,
            filter,
            tables: &world.storages().tables,
            archetypes: &world.archetypes,
            table_id_iter: query_state.matched_table_ids.iter(),
            archetype_id_iter: query_state.matched_archetype_ids.iter(),
            current_len: 0,
            current_index: 0,
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> Iterator for QueryIter<'w, 's, Q, F> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            if self.is_dense {
                loop {
                    if self.current_index == self.current_len {
                        let table_id = self.table_id_iter.next()?;
                        let table = self.tables.get_unchecked(*table_id);
                        self.fetch.next_table(&self.query_state.fetch_state, table);
                        self.filter.next_table(table);
                        self.current_len = table.len();
                        self.current_index = 0;
                        continue;
                    }

                    if !self.filter.matches_table_entity(self.current_index) {
                        self.current_index += 1;
                        continue;
                    }

                    let item = self.fetch.table_fetch(self.current_index);

                    self.current_index += 1;
                    return Some(item);
                }
            } else {
                loop {
                    if self.current_index == self.current_len {
                        let archetype_id = self.archetype_id_iter.next()?;
                        let archetype = self.archetypes.get_unchecked(*archetype_id);
                        self.fetch.next_archetype(&self.query_state.fetch_state, archetype, self.tables);
                        self.filter.next_archetype(archetype, self.tables);
                        self.current_len = archetype.len();
                        self.current_index = 0;
                        continue;
                    }

                    if !self.filter.matches_archetype_entity(self.current_index) {
                        self.current_index += 1;
                        continue;
                    }

                    let item = self.fetch.archetype_fetch(self.current_index);
                    self.current_index += 1;
                    return Some(item);
                }
            }
        }
    }
}

// NOTE: We can cheaply implement this for unfiltered Queries because we have:
// (1) pre-computed archetype matches
// (2) each archetype pre-computes length
// (3) there are no per-entity filters
// TODO: add an ArchetypeOnlyFilter that enables us to implement this for filters like With<T>
impl<'w, 's, Q: WorldQuery> ExactSizeIterator for QueryIter<'w, 's, Q, ()> {
    fn len(&self) -> usize {
        self.query_state
            .matched_archetypes
            .ones()
            .map(|index| {
                // SAFE: matched archetypes always exist
                let archetype =
                    unsafe { self.world.archetypes.get_unchecked(ArchetypeId::new(index)) };
                archetype.len()
            })
            .sum()
    }
}
