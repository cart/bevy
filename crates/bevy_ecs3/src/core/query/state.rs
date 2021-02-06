use crate::core::{ArchetypeGeneration, ArchetypeId, Archetypes, Fetch, QueryFilter, TableId, World};
use fixedbitset::FixedBitSet;
use std::ops::Range;

// TODO: try making typed if/when system params can store typed data
pub struct QueryState {
    pub(crate) archetype_generation: ArchetypeGeneration,
    pub(crate) matched_tables: FixedBitSet,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    pub(crate) matched_table_ids: Vec<TableId>,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_tables: FixedBitSet::default(),
            matched_table_ids: Vec::new(),
        }
    }
}

impl QueryState {
    // SAFETY: this must be called on the same fetch and filter types on every call, or unsafe access could occur during iteration
    pub(crate) unsafe fn update_archetypes<F: for<'w> Fetch<'w>, FI: QueryFilter>(
        &mut self,
        archetypes: &Archetypes,
        fetch: &F,
        filter: &FI,
    ) -> Range<usize> {
        let old_generation = self.archetype_generation;
        self.archetype_generation = archetypes.generation();
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
            let archetype = archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32));
            let table_index = archetype.table_id().index();
            if !self.matched_tables.contains(table_index)
                && fetch.matches_archetype(archetype)
                && filter.matches_archetype(archetype)
            {
                self.matched_tables.set(table_index, true);
                self.matched_table_ids.push(archetype.table_id());
            }
        }

        archetype_index_range
    }
}
