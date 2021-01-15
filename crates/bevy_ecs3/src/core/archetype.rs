use crate::core::{
    BundleId, ComponentId, Entity, EntityLocation, SparseArray, StorageType, Storages, TableId,
};
use bevy_utils::AHasher;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

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

#[derive(Default)]
pub struct Edges {
    pub add_bundle: SparseArray<BundleId, ArchetypeId>,
    pub remove_bundle: SparseArray<BundleId, ArchetypeId>,
}

impl Edges {
    #[inline]
    pub fn get_add_bundle(&self, bundle_id: BundleId) -> Option<ArchetypeId> {
        self.add_bundle.get(bundle_id).cloned()
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

pub struct Archetype {
    id: ArchetypeId,
    table_info: TableInfo,
    components: SparseArray<ComponentId, StorageType>,
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

    #[inline]
    pub fn get_storage_type(&self, component_id: ComponentId) -> Option<StorageType> {
        self.components.get(component_id).cloned()
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

#[derive(Hash)]
pub struct ArchetypeComponents<'a> {
    table_components: &'a [ComponentId],
    sparse_set_components: &'a [ComponentId],
}

pub struct Archetypes {
    archetypes: Vec<Archetype>,
    archetype_ids: HashMap<u64, ArchetypeId>,
}

impl Default for Archetypes {
    fn default() -> Self {
        let mut archetypes = Archetypes {
            archetypes: Vec::new(),
            archetype_ids: Default::default(),
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
    pub(crate) fn get_mut(&mut self, id: ArchetypeId) -> Option<&mut Archetype> {
        self.archetypes.get_mut(id.0 as usize)
    }

    /// SAFETY: `a` and `b` must both be valid archetypes and they _must_ be different
    #[inline]
    pub(crate) unsafe fn get_2_mut_unchecked(&mut self, a: ArchetypeId, b: ArchetypeId) -> (&mut Archetype, &mut Archetype) {
        let ptr = self.archetypes.as_mut_ptr();
        (&mut *ptr.add(a.index() as usize), &mut *ptr.add(b.index() as usize))
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Archetype> {
        self.archetypes.iter()
    }

    #[inline]
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Archetype> {
        self.archetypes.iter_mut()
    }

    /// Gets the archetype id matching the given inputs or inserts a new one if it doesn't exist.
    /// `table_components` and `sparse_set_components` must be sorted
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
