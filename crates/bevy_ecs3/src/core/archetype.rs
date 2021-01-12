use crate::core::{
    Bundle, ComponentId, Components, DynamicBundle, Entity, EntityLocation, SparseArray,
    SparseSets, StorageType, Storages, TableId, Tables, TypeInfo,
};
use bevy_utils::{AHasher, HashMap};
use std::{
    any::TypeId,
    hash::{Hash, Hasher},
};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ArchetypeId(pub(crate) u32);

impl ArchetypeId {
    #[inline]
    pub const fn empty_archetype() -> ArchetypeId {
        ArchetypeId(0)
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub fn is_empty_archetype(&self) -> bool {
        self.0 == 0
    }
}

#[derive(Default)]
pub struct Edges {
    add_table_component: SparseArray<ComponentId, ArchetypeId>,
    remove_table_component: SparseArray<ComponentId, ArchetypeId>,
    add_sparse_set_component: SparseArray<ComponentId, ArchetypeId>,
    remove_sparse_set_component: SparseArray<ComponentId, ArchetypeId>,
}

struct TableInfo {
    id: TableId,
    entity_rows: Vec<usize>,
}

pub struct Archetype {
    id: ArchetypeId,
    table_info: TableInfo,
    components: SparseArray<ComponentId, StorageType>,
    table_components: Vec<ComponentId>,
    sparse_set_components: Vec<ComponentId>,
    entities: Vec<Entity>,
    // edges: Edges,
}

impl Archetype {
    pub fn new(
        id: ArchetypeId,
        table_id: TableId,
        table_components: Vec<ComponentId>,
        sparse_set_components: Vec<ComponentId>,
    ) -> Self {
        let mut components = SparseArray::default();
        for component_id in table_components.iter().cloned() {
            components.insert(component_id, StorageType::Table);
        }

        for component_id in sparse_set_components.iter().cloned() {
            components.insert(component_id, StorageType::SparseSet);
        }
        Self {
            id,
            table_info: TableInfo {
                id: table_id,
                entity_rows: Default::default(),
            },
            components,
            table_components,
            sparse_set_components,
            entities: Default::default(),
            // edges: Default::default(),
        }
    }

    #[inline]
    pub fn id(&self) -> ArchetypeId {
        self.id
    }

    #[inline]
    pub fn table_id(&self) -> TableId {
        self.table_info.id
    }

    #[inline]
    pub fn table_components(&self) -> &[ComponentId] {
        &self.table_components
    }

    #[inline]
    pub fn sparse_set_components(&self) -> &[ComponentId] {
        &self.sparse_set_components
    }

    /// SAFETY: index must be in bounds
    #[inline]
    pub unsafe fn entity_table_row_unchecked(&self, index: usize) -> usize {
        *self.table_info.entity_rows.get_unchecked(index)
    }

    /// SAFETY: index must be in bounds
    #[inline]
    pub unsafe fn set_entity_table_row_unchecked(&mut self, index: usize, table_row: usize) {
        *self.table_info.entity_rows.get_unchecked_mut(index) = table_row;
    }

    /// SAFETY: valid component values must be immediately written to the relevant storages
    pub unsafe fn allocate(&mut self, entity: Entity, storages: &mut Storages) -> EntityLocation {
        self.entities.push(entity);
        // SAFE: self.table_id is always valid
        let table = storages.tables.get_unchecked_mut(self.table_info.id);
        let table_row = table.allocate(entity);
        self.table_info.entity_rows.push(table_row);

        EntityLocation {
            archetype_id: self.id,
            index: self.entities.len() - 1,
        }
    }

    /// Removes the entity at `index` by swapping it out. Returns the table row the entity is stored in.
    pub fn swap_remove(&mut self, index: usize) -> usize {
        self.entities.swap_remove(index);
        self.table_info.entity_rows.swap_remove(index)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[inline]
    pub fn contains(&self, component_id: ComponentId) -> bool {
        self.components.contains(component_id)
    }
}

/// A generational id that changes every time the set of archetypes changes
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ArchetypeGeneration(pub(crate) u32);
#[derive(Hash)]
pub struct ArchetypeHash<'a> {
    table_components: &'a [ComponentId],
    sparse_set_components: &'a [ComponentId],
}

#[derive(Hash)]
pub struct ArchetypeComponents<'a> {
    table_components: &'a [ComponentId],
    sparse_set_components: &'a [ComponentId],
}

#[derive(Default)]
pub struct Archetypes {
    archetypes: Vec<Archetype>,
    archetype_ids: HashMap<u64, ArchetypeId>,
}

impl Archetypes {
    #[inline]
    pub fn generation(&self) -> ArchetypeGeneration {
        ArchetypeGeneration(self.archetypes.len() as u32)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.archetypes.len()
    }

    #[inline]
    pub fn get(&self, id: ArchetypeId) -> Option<&Archetype> {
        self.archetypes.get(id.0 as usize)
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, id: ArchetypeId) -> &Archetype {
        self.archetypes.get_unchecked(id.0 as usize)
    }

    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        self.archetypes.get_unchecked_mut(id.0 as usize)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: ArchetypeId) -> Option<&mut Archetype> {
        self.archetypes.get_mut(id.0 as usize)
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Archetype> {
        self.archetypes.iter()
    }

    #[inline]
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Archetype> {
        self.archetypes.iter_mut()
    }

    pub fn get_id_or_insert(
        &mut self,
        table_id: TableId,
        table_components: Vec<ComponentId>,
        sparse_set_components: Vec<ComponentId>,
    ) -> ArchetypeId {
        let archetype_hash = ArchetypeHash {
            sparse_set_components: &sparse_set_components,
            table_components: &table_components,
        };

        let mut hasher = AHasher::default();
        archetype_hash.hash(&mut hasher);
        let hash = hasher.finish();
        let archetypes = &mut self.archetypes;
        *self.archetype_ids.entry(hash).or_insert_with(|| {
            let id = ArchetypeId(archetypes.len() as u32);
            archetypes.push(Archetype::new(
                id,
                table_id,
                table_components,
                sparse_set_components,
            ));
            id
        })
    }
}
