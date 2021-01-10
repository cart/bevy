use bevy_utils::HashMap;

use crate::{ArchetypeId, ComponentId, table::TableId};
use std::hash::{Hash, Hasher};

#[derive(Hash)]
pub struct ArchetypeHash<'a> {
    table_components: &'a [ComponentId],
    sparse_set_components: &'a [ComponentId],
}

pub struct Edges {
    add_table_component: Vec<ArchetypeId>,
    remove_table_component: Vec<ArchetypeId>,
    add_sparse_set_component: Vec<ArchetypeId>,
    remove_sparse_set_component: Vec<ArchetypeId>,
}

pub struct Archetype {
    table_components: Vec<ComponentId>,
    sparse_set_components: Vec<ComponentId>,
    table: Option<TableId>,
    edges: Edges,
}

pub struct Archetypes {
    archetypes: Vec<Archetype>,
    archetype_ids: HashMap<u64, ArchetypeId>,
}