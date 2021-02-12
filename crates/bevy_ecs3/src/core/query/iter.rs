use crate::core::{
    world, ArchetypeId, Fetch, QueryFilter, QueryState, TableId, Tables, World, WorldQuery,
};

pub struct QueryIter<'w, 's, Q: WorldQuery, F: QueryFilter> {
    tables: &'w Tables,
    // TODO: try removing this for bitset iterator
    query_state: &'s QueryState<Q, F>,
    world: &'w World,
    table_id_iter: std::slice::Iter<'s, TableId>,
    fetch: Q::Fetch,
    filter: F,
    pub(crate) is_dense: bool,
    table_len: usize,
    table_index: usize,
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> QueryIter<'w, 's, Q, F> {
    pub(crate) unsafe fn new(world: &'w World, query_state: &'s QueryState<Q, F>) -> Self {
        let fetch = <Q::Fetch as Fetch>::init(world, &query_state.fetch_state);
        QueryIter {
            is_dense: fetch.is_dense(),
            world,
            query_state,
            fetch,
            filter: F::init(world, &query_state.filter_state),
            tables: &world.storages().tables,
            table_id_iter: query_state.matched_table_ids.iter(),
            table_len: 0,
            table_index: 0,
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
                    if self.table_index == self.table_len {
                        let table_id = self.table_id_iter.next()?;
                        let table = self.tables.get_unchecked(*table_id);
                        self.fetch.next_table_dense(table);
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
                let archetype = unsafe {
                    self.world
                        .archetypes
                        .get_unchecked(ArchetypeId::new(index as u32))
                };
                archetype.len()
            })
            .sum()
    }
}
