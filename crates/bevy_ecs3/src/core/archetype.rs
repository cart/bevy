use crate::core::{
    BundleId, ComponentFlags, ComponentId, Entity, EntityLocation, SparseArray, StorageType,
    TableId,
};
use bevy_utils::AHasher;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use super::SparseSetIndex;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ArchetypeId(u32);

impl ArchetypeId {
    #[inline]
    pub const fn new(index: u32) -> Self {
        ArchetypeId(index)
    }

    #[inline]
    pub const fn empty_archetype() -> ArchetypeId {
        ArchetypeId(0)
    }

    #[inline]
    pub fn index(&self) -> u32 {
        self.0
    }

    #[inline]
    pub fn is_empty_archetype(&self) -> bool {
        self.0 == 0
    }
}

pub struct FromBundle {
    pub archetype_id: ArchetypeId,
    pub bundle_flags: Vec<ComponentFlags>,
}

#[derive(Default)]
pub struct Edges {
    pub add_bundle: SparseArray<BundleId, ArchetypeId>,
    pub remove_bundle: SparseArray<BundleId, Option<ArchetypeId>>,
    pub from_bundle: SparseArray<BundleId, FromBundle>,
}

impl Edges {
    #[inline]
    pub fn get_add_bundle(&self, bundle_id: BundleId) -> Option<ArchetypeId> {
        self.add_bundle.get(bundle_id).cloned()
    }

    /// SAFETY: bundle must exist
    #[inline]
    pub unsafe fn get_from_bundle_unchecked(&self, bundle_id: BundleId) -> &FromBundle {
        self.from_bundle.get_unchecked(bundle_id)
    }

    #[inline]
    pub fn set_from_bundle(
        &mut self,
        bundle_id: BundleId,
        archetype_id: ArchetypeId,
        bundle_flags: Vec<ComponentFlags>,
    ) {
        self.from_bundle.insert(
            bundle_id,
            FromBundle {
                archetype_id,
                bundle_flags,
            },
        );
    }

    #[inline]
    pub fn get_remove_bundle(&self, bundle_id: BundleId) -> Option<Option<ArchetypeId>> {
        self.remove_bundle.get(bundle_id).cloned()
    }

    #[inline]
    pub fn set_remove_bundle(&mut self, bundle_id: BundleId, archetype_id: Option<ArchetypeId>) {
        self.remove_bundle.insert(bundle_id, archetype_id);
    }

    #[inline]
    pub fn set_add_bundle(&mut self, bundle_id: BundleId, archetype_id: ArchetypeId) {
        self.add_bundle.insert(bundle_id, archetype_id);
    }
}

struct TableInfo {
    id: TableId,
    entity_rows: Vec<usize>,
}

pub(crate) struct ArchetypeSwapRemoveResult {
    pub swapped_entity: Option<Entity>,
    pub table_row: usize,
}

struct ArchetypeComponentInfo {
    storage_type: StorageType,
    archetype_component_id: ArchetypeComponentId,
}

pub struct Archetype {
    id: ArchetypeId,
    table_info: TableInfo,
    components: SparseArray<ComponentId, ArchetypeComponentInfo>,
    table_components: Vec<ComponentId>,
    sparse_set_components: Vec<ComponentId>,
    entities: Vec<Entity>,
    edges: Edges,
}

impl Archetype {
    pub fn new(
        id: ArchetypeId,
        table_id: TableId,
        table_components: Vec<ComponentId>,
        sparse_set_components: Vec<ComponentId>,
        table_archetype_components: Vec<ArchetypeComponentId>,
        sparse_set_archetype_components: Vec<ArchetypeComponentId>,
    ) -> Self {
        let mut components = SparseArray::default();
        for (component_id, archetype_component_id) in
            table_components.iter().zip(table_archetype_components)
        {
            components.insert(
                *component_id,
                ArchetypeComponentInfo {
                    storage_type: StorageType::Table,
                    archetype_component_id,
                },
            );
        }

        for (component_id, archetype_component_id) in sparse_set_components
            .iter()
            .zip(sparse_set_archetype_components)
        {
            components.insert(
                *component_id,
                ArchetypeComponentInfo {
                    storage_type: StorageType::SparseSet,
                    archetype_component_id,
                },
            );
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
            edges: Default::default(),
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
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    #[inline]
    pub fn table_components(&self) -> &Vec<ComponentId> {
        &self.table_components
    }

    #[inline]
    pub fn sparse_set_components(&self) -> &Vec<ComponentId> {
        &self.sparse_set_components
    }

    #[inline]
    pub fn edges(&self) -> &Edges {
        &self.edges
    }

    #[inline]
    pub(crate) fn edges_mut(&mut self) -> &mut Edges {
        &mut self.edges
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
    /// `table_row` must be valid
    pub unsafe fn allocate(&mut self, entity: Entity, table_row: usize) -> EntityLocation {
        self.entities.push(entity);
        self.table_info.entity_rows.push(table_row);

        EntityLocation {
            archetype_id: self.id,
            index: self.entities.len() - 1,
        }
    }

    pub fn reserve(&mut self, additional: usize) {
        self.entities.reserve(additional);
        self.table_info.entity_rows.reserve(additional);
    }

    /// Removes the entity at `index` by swapping it out. Returns the table row the entity is stored in.
    pub(crate) fn swap_remove(&mut self, index: usize) -> ArchetypeSwapRemoveResult {
        let is_last = index == self.entities.len() - 1;
        self.entities.swap_remove(index);
        ArchetypeSwapRemoveResult {
            swapped_entity: if is_last {
                None
            } else {
                Some(self.entities[index])
            },
            table_row: self.table_info.entity_rows.swap_remove(index),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[inline]
    pub fn contains(&self, component_id: ComponentId) -> bool {
        self.components.contains(component_id)
    }

    #[inline]
    pub fn get_storage_type(&self, component_id: ComponentId) -> Option<StorageType> {
        self.components
            .get(component_id)
            .map(|info| info.storage_type)
    }

    #[inline]
    pub fn get_archetype_component_id(
        &self,
        component_id: ComponentId,
    ) -> Option<ArchetypeComponentId> {
        self.components
            .get(component_id)
            .map(|info| info.archetype_component_id)
    }
}

/// A generational id that changes every time the set of archetypes changes
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ArchetypeGeneration(usize);

impl ArchetypeGeneration {
    #[inline]
    pub fn new(generation: usize) -> Self {
        ArchetypeGeneration(generation)
    }

    #[inline]
    pub fn value(&self) -> usize {
        self.0
    }
}

#[derive(Hash)]
pub struct ArchetypeHash<'a> {
    table_components: &'a [ComponentId],
    sparse_set_components: &'a [ComponentId],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ArchetypeComponent {
    pub archetype_id: ArchetypeId,
    pub component_id: ComponentId,
}

impl ArchetypeComponent {
    #[inline]
    pub fn new(archetype_id: ArchetypeId, component_id: ComponentId) -> Self {
        ArchetypeComponent {
            archetype_id,
            component_id,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ArchetypeComponentId(usize);

impl ArchetypeComponentId {
    #[inline]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0
    }
}

impl SparseSetIndex for ArchetypeComponentId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.0
    }
}

struct ArchetypeComponents {

}

pub struct Archetypes {
    archetypes: Vec<Archetype>,
    archetype_ids: HashMap<u64, ArchetypeId>,
    archetype_component_count: usize,
}

impl Default for Archetypes {
    fn default() -> Self {
        let mut archetypes = Archetypes {
            archetypes: Vec::new(),
            archetype_ids: Default::default(),
            archetype_component_count: 0,
        };
        archetypes.get_id_or_insert(TableId::empty_table(), Vec::new(), Vec::new());
        archetypes
    }
}

impl Archetypes {
    #[inline]
    pub fn generation(&self) -> ArchetypeGeneration {
        ArchetypeGeneration(self.archetypes.len())
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
    pub fn iter(&self) -> impl Iterator<Item = &Archetype> {
        self.archetypes.iter()
    }

    /// Gets the archetype id matching the given inputs or inserts a new one if it doesn't exist.
    /// `table_components` and `sparse_set_components` must be sorted
    /// SAFETY: TableId must exist in tables
    pub(crate) fn get_id_or_insert(
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
        let archetype_component_count = &mut self.archetype_component_count;
        let mut next_archetype_component_id = move || {
            let id = ArchetypeComponentId(*archetype_component_count);
            *archetype_component_count += 1;
            id
        };
        *self.archetype_ids.entry(hash).or_insert_with(move || {
            let id = ArchetypeId(archetypes.len() as u32);
            let table_archetype_components = (0..table_components.len())
                .map(|_| next_archetype_component_id())
                .collect();
            let sparse_set_archetype_components = (0..sparse_set_components.len())
                .map(|_| next_archetype_component_id())
                .collect();
            archetypes.push(Archetype::new(
                id,
                table_id,
                table_components,
                sparse_set_components,
                table_archetype_components,
                sparse_set_archetype_components,
            ));
            id
        })
    }

    fn get_next_archetype_component_id(&mut self) -> ArchetypeComponentId {
        let id = ArchetypeComponentId(self.archetype_component_count);
        self.archetype_component_count += 1;
        id
    }
}
