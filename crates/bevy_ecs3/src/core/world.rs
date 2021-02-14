use crate::core::{
    ArchetypeComponentId, ArchetypeComponentInfo, ArchetypeId, Archetypes, Bundle, Bundles, Column,
    Component, ComponentDescriptor, ComponentId, Components, ComponentsError, Entities, Entity,
    EntityMut, EntityRef, Mut, QueryFilter, QueryState, SparseSet, SpawnBatchIter, StorageType,
    Storages, WorldQuery,
};
use std::{
    any::{Any, TypeId},
    fmt,
};

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
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.entity(entity)?.get()
    }

    #[inline]
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<T>> {
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
    pub fn query<Q: WorldQuery>(&mut self) -> QueryState<Q, ()> {
        QueryState::new(self)
    }

    #[inline]
    pub fn query_filtered<Q: WorldQuery, F: QueryFilter>(&mut self) -> QueryState<Q, F> {
        QueryState::new(self)
    }

    pub fn removed<T: Component>(&self) -> std::iter::Cloned<std::slice::Iter<'_, Entity>> {
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

    #[inline]
    pub fn insert_resource<T: Component>(&mut self, value: T) {
        let component_id = self.components.get_or_insert_resource_id::<T>();
        self.insert_resource_with_id(component_id, value);
    }

    #[inline]
    pub fn insert_non_send_resource<T: Any>(&mut self, value: T) {
        let component_id = self.components.get_or_insert_non_send_resource_id::<T>();
        self.insert_resource_with_id(component_id, value);
    }

    #[inline]
    pub fn get_resource<T: Component>(&self) -> Option<&T> {
        let column = self.get_resource_column_with_type(TypeId::of::<T>())?;
        // SAFE: resource exists and is of type T
        unsafe { Some(&*column.get_ptr().as_ptr().cast::<T>()) }
    }

    #[inline]
    pub fn get_resource_mut<T: Component>(&mut self) -> Option<Mut<'_, T>> {
        let column = self.get_resource_column_with_type(TypeId::of::<T>())?;
        // SAFE: resource exists and is of type T
        unsafe {
            Some(Mut {
                value: &mut *column.get_ptr().as_ptr().cast::<T>(),
                flags: &mut *column.get_flags_mut_ptr(),
            })
        }
    }

    // PERF: optimize this to avoid redundant lookups
    #[inline]
    pub(crate) fn get_resource_or_insert_with<T: Component>(
        &mut self,
        func: impl FnOnce() -> T,
    ) -> Mut<'_, T> {
        if self.contains_resource::<T>() {
            self.get_resource_mut().unwrap()
        } else {
            self.insert_resource(func());
            self.get_resource_mut().unwrap()
        }
    }

    #[inline]
    pub fn contains_resource<T: Component>(&mut self) -> bool {
        let component_id =
            if let Some(component_id) = self.components.get_resource_id(TypeId::of::<T>()) {
                component_id
            } else {
                return false;
            };
        // SAFE: resource archetype is guaranteed to exist
        let resource_archetype = unsafe {
            self.archetypes
                .get_unchecked(ArchetypeId::resource_archetype())
        };
        let unique_components = resource_archetype.unique_components();
        unique_components.contains(component_id)
    }


    #[inline]
    fn insert_resource_with_id<T>(&mut self, component_id: ComponentId, mut value: T) {
        // SAFE: resource archetype is guaranteed to exist
        let resource_archetype = unsafe {
            self.archetypes
                .archetypes
                .get_unchecked_mut(ArchetypeId::resource_archetype().index())
        };
        let unique_components = &mut resource_archetype.unique_components;
        if let Some(column) = unique_components.get_mut(component_id) {
            // SAFE: column is of type T and has already been allocated
            let row = unsafe { &mut *column.get_unchecked(0).cast::<T>() };
            *row = value;
        } else {
            resource_archetype.components.insert(
                component_id,
                ArchetypeComponentInfo {
                    archetype_component_id: ArchetypeComponentId::new(
                        self.archetypes.archetype_component_count,
                    ),
                    storage_type: StorageType::Table,
                },
            );
            self.archetypes.archetype_component_count += 1;
            // SAFE: component was initialized above
            let component_info = unsafe { self.components.get_info_unchecked(component_id) };
            let mut column = Column::with_capacity(component_info, 1);
            unsafe {
                column.push_uninit();
                // SAFE: column is of type T and has been allocated above
                let data = (&mut value as *mut T).cast::<u8>();
                column.set_unchecked(0, data);
                std::mem::forget(value);
            }

            unique_components.insert(component_id, column);
        }
    }

    fn get_resource_column_with_type(&self, type_id: TypeId) -> Option<&Column> {
        let component_id = self.components.get_resource_id(type_id)?;
        self.get_resource_column(component_id)
    }

    pub(crate) fn get_resource_column(&self, component_id: ComponentId) -> Option<&Column> {
        // SAFE: resource archetype is guaranteed to exist
        let resource_archetype = unsafe {
            self.archetypes
                .get_unchecked(ArchetypeId::resource_archetype())
        };
        let unique_components = resource_archetype.unique_components();
        unique_components.get(component_id)
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
