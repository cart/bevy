use crate::core::{Access, ArchetypeComponentId, ArchetypeGeneration, ArchetypeId, Fetch, QueryFilter, TableId, World};
use fixedbitset::FixedBitSet;

pub struct QueryState {
    pub(crate) type_name: &'static str,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) archetype_generation: ArchetypeGeneration,
    pub(crate) matched_tables: FixedBitSet,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    pub(crate) matched_table_ids: Vec<TableId>,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            type_name: "Unknown",
            archetype_component_access: Default::default(),
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_tables: FixedBitSet::default(),
            matched_table_ids: Vec::new(),
        }
    }
}

impl QueryState {
    // SAFETY: this must be called on the same fetch and filter types on every call, or unsafe access could occur during iteration
    pub(crate) unsafe fn update<F: for<'w> Fetch<'w>, FI: QueryFilter>(
        &mut self,
        fetch: F,
        filter: FI,
        world: &World,
    ) {
        let old_generation = self.archetype_generation;
        let archetypes = world.archetypes();
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
        for archetype_index in archetype_index_range {
            let archetype = archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32));
            let table_index = archetype.table_id().index();
            // SAFE: ArchetypeGeneration is used to generate the range, and is by definition valid
            if !self.matched_tables.contains(table_index)
                && fetch.matches_archetype(archetype)
                && filter.matches_archetype(archetype)
            {
                self.matched_tables.set(table_index, true);
                self.matched_table_ids.push(archetype.table_id());
            }
        }
    }
}
