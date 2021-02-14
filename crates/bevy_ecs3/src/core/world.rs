use crate::core::{
    ArchetypeId, Archetypes, Bundle, Bundles, Component, ComponentDescriptor, ComponentId,
    Components, ComponentsError, Entities, Entity, EntityMut, EntityRef, Mut, QueryFilter,
    QueryState, SparseSet, SpawnBatchIter, StorageType, Storages, WorldQuery,
};
use std::{any::TypeId, fmt};

#[derive(Default)]
pub struct World {
    pub(crate) entities: Entities,
    pub(crate) components: Components,
    pub(crate) archetypes: Archetypes,
    pub(crate) storages: Storages,
    pub(crate) bundles: Bundles,
    pub(crate) removed_components: SparseSet<ComponentId, Vec<Entity>>,
}

impl World {
    #[inline]
    pub fn new() -> World {
        World::default()
    }

    #[inline]
    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    #[inline]
    pub fn archetypes(&self) -> &Archetypes {
        &self.archetypes
    }

    #[inline]
    pub fn components(&self) -> &Components {
        &self.components
    }

    #[inline]
    pub fn storages(&self) -> &Storages {
        &self.storages
    }

    #[inline]
    pub fn bundles(&self) -> &Bundles {
        &self.bundles
    }

    pub fn register_component(
        &mut self,
        descriptor: ComponentDescriptor,
    ) -> Result<ComponentId, ComponentsError> {
        let storage_type = descriptor.storage_type();
        let component_id = self.components.add(descriptor)?;
        // ensure sparse set is created for SparseSet components
        if storage_type == StorageType::SparseSet {
            // SAFE: just created
            let info = unsafe { self.components.get_info_unchecked(component_id) };
            self.storages.sparse_sets.get_or_insert(info);
        }

        Ok(component_id)
    }

    #[inline]
    pub fn entity(&self, entity: Entity) -> Option<EntityRef> {
        let location = self.entities.get(entity)?;
        Some(EntityRef::new(self, entity, location))
    }

    #[inline]
    pub fn entity_mut(&mut self, entity: Entity) -> Option<EntityMut> {
        let location = self.entities.get(entity)?;
        Some(EntityMut::new(self, entity, location))
    }

    pub fn spawn(&mut self) -> EntityMut {
        self.flush();
        let entity = self.entities.alloc();
        // SAFE: empty archetype exists and no components are allocated by archetype.allocate() because the archetype is empty
        unsafe {
            let archetype = self
                .archetypes
                .get_unchecked_mut(ArchetypeId::empty_archetype());
            // PERF: avoid allocating entities in the empty archetype unless needed
            let table = self.storages.tables.get_unchecked_mut(archetype.table_id());
            let location = archetype.allocate(entity, table.allocate(entity));
            // SAFE: entity index was just allocated
            self.entities
                .meta
                .get_unchecked_mut(entity.id() as usize)
                .location = location;
            EntityMut::new(self, entity, location)
        }
    }

    #[inline]
    pub fn get<T: Component + Send + Sync>(&self, entity: Entity) -> Option<&T> {
        self.entity(entity)?.get()
    }

    #[inline]
    pub fn get_mut<T: Component + Send + Sync>(&mut self, entity: Entity) -> Option<Mut<T>> {
        self.entity_mut(entity)?.get_mut()
    }

    #[inline]
    pub fn despawn(&mut self, entity: Entity) -> bool {
        self.entity_mut(entity)
            .map(|e| {
                e.despawn();
                true
            })
            .unwrap_or(false)
    }

    pub fn spawn_batch<I>(&mut self, iter: I) -> SpawnBatchIter<'_, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: Bundle,
    {
        SpawnBatchIter::new(self, iter.into_iter())
    }

    pub(crate) fn flush(&mut self) {
        // SAFE: empty archetype is initialized when the world is constructed
        unsafe {
            let empty_archetype = self
                .archetypes
                .get_unchecked_mut(ArchetypeId::empty_archetype());
            let table = self
                .storages
                .tables
                .get_unchecked_mut(empty_archetype.table_id());
            // PERF: consider pre-allocating space for flushed entities
            self.entities.flush(|entity, location| {
                *location = empty_archetype.allocate(entity, table.allocate(entity));
            });
        }
    }

    pub fn clear_trackers(&mut self) {
        self.storages.tables.clear_flags();
        self.storages.sparse_sets.clear_flags();
        for entities in self.removed_components.values_mut() {
            entities.clear();
        }
    }

    #[inline]
    pub fn insert_resource<T: Component + Send + Sync>(&mut self, value: T) {
        self.archetypes.insert_resource(&mut self.components, value);
    }

    #[inline]
    pub fn get_resource<T: Component + Send + Sync>(&self) -> Option<&T> {
        self.archetypes.get_resource(&self.components)
    }

    #[inline]
    pub fn get_resource_mut<T: Component + Send + Sync>(&mut self) -> Option<Mut<'_, T>> {
        self.archetypes.get_resource_mut(&self.components)
    }

    #[inline]
    pub fn get_resource_or_insert_with<T: Component + Send + Sync>(
        &mut self,
        func: impl FnOnce() -> T,
    ) -> Mut<'_, T> {
        self.archetypes
            .get_resource_or_insert_with(&mut self.components, func)
    }

    #[inline]
    pub fn contains_resource<T: Component + Send + Sync>(&mut self) -> bool {
        self.archetypes.contains_resource::<T>(&self.components)
    }

    #[inline]
    pub fn query<Q: WorldQuery>(&mut self) -> QueryState<Q, ()> {
        QueryState::new(self)
    }

    #[inline]
    pub fn query_filtered<Q: WorldQuery, F: QueryFilter>(&mut self) -> QueryState<Q, F> {
        QueryState::new(self)
    }

    pub fn removed<T: Component + Send + Sync>(&self) -> std::iter::Cloned<std::slice::Iter<'_, Entity>> {
        if let Some(component_id) = self.components.get_id(TypeId::of::<T>()) {
            self.removed_with_id(component_id)
        } else {
            [].iter().cloned()
        }
    }

    pub fn removed_with_id(
        &self,
        component_id: ComponentId,
    ) -> std::iter::Cloned<std::slice::Iter<'_, Entity>> {
        if let Some(removed) = self.removed_components.get(component_id) {
            removed.iter().cloned()
        } else {
            [].iter().cloned()
        }
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "World")
    }
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

/// Creates `Self` using data from the given [World]
pub trait FromWorld {
    /// Creates `Self` using data from the given [World]
    fn from_world(world: &World) -> Self;
}

impl<T: Default> FromWorld for T {
    fn from_world(_world: &World) -> Self {
        T::default()
    }
}
