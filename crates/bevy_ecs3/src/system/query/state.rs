use crate::{core::{Access, ArchetypeComponentId, ArchetypeId, Archetypes, ComponentId, Fetch, QueryFilter, QueryState, World, WorldQuery}, system::Query};

// TODO: try making typed if/when system params can store typed data
pub struct SystemQueryState {
    pub(crate) type_name: &'static str,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) component_access: Access<ComponentId>,
    pub(crate) query_state: QueryState,
}

impl Default for SystemQueryState {
    fn default() -> Self {
        Self {
            type_name: "Unknown",
            archetype_component_access: Default::default(),
            component_access: Default::default(),
            query_state: Default::default(),
        }
    }
}

impl SystemQueryState {
    // SAFETY: this must be called on the same fetch and filter types on every call, or unsafe access could occur during iteration
    pub(crate) unsafe fn initialize<F: for<'w> Fetch<'w>, FI: QueryFilter>(
        &mut self,
        fetch: &F,
        filter: &FI,
    ) {
        fetch.update_component_access(&mut self.component_access);
        filter.update_component_access(&mut self.component_access);
    }

    // SAFETY: this must be called on the same fetch and filter types on every call, or unsafe access could occur during iteration
    pub(crate) unsafe fn update_archetypes<F: for<'w> Fetch<'w>, FI: QueryFilter>(
        &mut self,
        archetypes: &Archetypes,
        fetch: &F,
        filter: &FI,
    ) {
        for archetype_index in self
            .query_state
            .update_archetypes(archetypes, fetch, filter)
        {
            // SAFE: archetype index range generated directly from archetypes len
            let archetype = archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32));
            fetch
                .update_archetype_component_access(archetype, &mut self.archetype_component_access);
            filter
                .update_archetype_component_access(archetype, &mut self.archetype_component_access);
        }
    }
}
