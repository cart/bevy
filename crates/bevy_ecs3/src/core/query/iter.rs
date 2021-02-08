use crate::core::{Fetch, QueryFilter, QueryState, TableId, Tables, World, WorldQuery};

pub struct QueryIter<'w, 's, Q: WorldQuery, F: QueryFilter> {
    tables: &'w Tables,
    // TODO: try removing this for bitset iterator
    table_id_iter: std::slice::Iter<'s, TableId>,
    fetch: Q::Fetch,
    filter: F,
    table_len: usize,
    table_index: usize,
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> QueryIter<'w, 's, Q, F> {
    pub(crate) unsafe fn new(world: &'w World, query_state: &'s QueryState<Q, F>) -> Self {
        let (fetch, filter) = query_state
            .state
            .as_ref()
            .map(|(fetch_state, filter_state)| {
                (
                    <Q::Fetch as Fetch>::init(world, fetch_state),
                    F::init(world, filter_state),
                )
            })
            .unwrap_or_else(|| (<Q::Fetch as Fetch>::DANGLING, F::DANGLING));
        QueryIter {
            fetch,
            tables: &world.storages().tables,
            filter,
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
