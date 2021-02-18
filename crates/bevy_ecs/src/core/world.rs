use crate::core::{
    world_cell::WorldCell, ArchetypeComponentId, ArchetypeComponentInfo, ArchetypeId, Archetypes,
    Bundle, Bundles, Column, Component, ComponentDescriptor, ComponentId, Components,
    ComponentsError, Entities, Entity, EntityMut, EntityRef, Mut, QueryFilter, QueryState,
    SparseSet, SpawnBatchIter, StorageType, Storages, WorldQuery,
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
    main_thread_validator: MainThreadValidator,
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
    pub fn components_mut(&mut self) -> &mut Components {
        &mut self.components
    }

    #[inline]
    pub fn storages(&self) -> &Storages {
        &self.storages
    }

    #[inline]
    pub fn bundles(&self) -> &Bundles {
        &self.bundles
    }

    #[inline]
    pub fn cell(&mut self) -> WorldCell<'_> {
        WorldCell {
            world: self,
            access: Default::default(),
        }
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
    pub fn entity(&self, entity: Entity) -> EntityRef {
        self.get_entity(entity).expect("Entity does not exist")
    }

    #[inline]
    pub fn entity_mut(&mut self, entity: Entity) -> EntityMut {
        self.get_entity_mut(entity).expect("Entity does not exist")
    }

    #[inline]
    pub fn get_entity(&self, entity: Entity) -> Option<EntityRef> {
        let location = self.entities.get(entity)?;
        Some(EntityRef::new(self, entity, location))
    }

    #[inline]
    pub fn get_entity_mut(&mut self, entity: Entity) -> Option<EntityMut> {
        let location = self.entities.get(entity)?;
        // SAFE: `entity` exists and `location` is that entity's location
        Some(unsafe { EntityMut::new(self, entity, location) })
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
        self.get_entity(entity)?.get()
    }

    #[inline]
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<T>> {
        self.get_entity_mut(entity)?.get_mut()
    }

    #[inline]
    pub fn despawn(&mut self, entity: Entity) -> bool {
        self.get_entity_mut(entity)
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
    pub fn insert_non_send<T: 'static>(&mut self, value: T) {
        let component_id = self.components.get_or_insert_non_send_id::<T>();
        self.insert_resource_with_id(component_id, value);
    }

    #[inline]
    pub fn get_resource<T: Component>(&self) -> Option<&T> {
        let component_id = self.components.get_resource_id(TypeId::of::<T>())?;
        unsafe { self.get_resource_with_id(component_id) }
    }

    #[inline]
    pub fn get_resource_mut<T: Component>(&mut self) -> Option<Mut<'_, T>> {
        // SAFE: unique world access
        unsafe { self.get_resource_mut_unchecked() }
    }

    #[inline]
    pub unsafe fn get_resource_mut_unchecked<T: Component>(&self) -> Option<Mut<'_, T>> {
        let component_id = self.components.get_resource_id(TypeId::of::<T>())?;
        self.get_resource_mut_unchecked_with_id(component_id)
    }

    #[inline]
    pub fn get_non_send<T: 'static>(&self) -> Option<&T> {
        let component_id = self.components.get_resource_id(TypeId::of::<T>())?;
        // SAFE: component id matches type T
        unsafe { self.get_non_send_with_id(component_id) }
    }

    #[inline]
    pub fn get_non_send_mut<T: 'static>(&mut self) -> Option<Mut<'_, T>> {
        // SAFE: unique world access
        unsafe { self.get_non_send_mut_unchecked() }
    }

    /// # Safety
    /// `component_id` must be assigned to a component of type T
    /// Caller must ensure this doesn't violate Rust mutability rules for the given resource.
    #[inline]
    pub(crate) unsafe fn get_resource_with_id<T: 'static>(
        &self,
        component_id: ComponentId,
    ) -> Option<&T> {
        let column = self.get_resource_column(component_id)?;
        Some(&*column.get_ptr().as_ptr().cast::<T>())
    }

    /// # Safety
    /// `component_id` must be assigned to a component of type T.
    /// Caller must ensure this doesn't violate Rust mutability rules for the given resource.
    #[inline]
    pub(crate) unsafe fn get_resource_mut_unchecked_with_id<T>(
        &self,
        component_id: ComponentId,
    ) -> Option<Mut<'_, T>> {
        let column = self.get_resource_column(component_id)?;
        Some(Mut {
            value: &mut *column.get_ptr().as_ptr().cast::<T>(),
            flags: &mut *column.get_flags_mut_ptr(),
        })
    }

    /// # Safety
    /// Caller must ensure this doesn't violate Rust mutability rules for the given resource.
    #[inline]
    pub unsafe fn get_non_send_mut_unchecked<T: 'static>(&self) -> Option<Mut<'_, T>> {
        let component_id = self.components.get_resource_id(TypeId::of::<T>())?;
        self.get_non_send_mut_unchecked_with_id(component_id)
    }

    /// # Safety
    /// `component_id` must be assigned to a component of type T
    /// Caller must ensure this doesn't violate Rust mutability rules for the given resource.
    #[inline]
    pub(crate) unsafe fn get_non_send_with_id<T: 'static>(
        &self,
        component_id: ComponentId,
    ) -> Option<&T> {
        self.validate_non_send_access::<T>();
        self.get_resource_with_id(component_id)
    }

    /// # Safety
    /// `component_id` must be assigned to a component of type T.
    /// Caller must ensure this doesn't violate Rust mutability rules for the given resource.
    #[inline]
    pub(crate) unsafe fn get_non_send_mut_unchecked_with_id<T: 'static>(
        &self,
        component_id: ComponentId,
    ) -> Option<Mut<'_, T>> {
        self.validate_non_send_access::<T>();
        self.get_resource_mut_unchecked_with_id(component_id)
    }

    // PERF: optimize this to avoid redundant lookups
    #[inline]
    pub fn get_resource_or_insert_with<T: Component>(
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

    pub(crate) fn get_resource_column(&self, component_id: ComponentId) -> Option<&Column> {
        // SAFE: resource archetype is guaranteed to exist
        let resource_archetype = unsafe {
            self.archetypes
                .get_unchecked(ArchetypeId::resource_archetype())
        };
        let unique_components = resource_archetype.unique_components();
        unique_components.get(component_id)
    }

    fn validate_non_send_access<T: 'static>(&self) {
        if !self.main_thread_validator.is_main_thread() {
            panic!(
                "attempted to access NonSend resource {} off of the main thread",
                std::any::type_name::<T>()
            );
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

struct MainThreadValidator {
    main_thread: std::thread::ThreadId,
}

impl MainThreadValidator {
    fn is_main_thread(&self) -> bool {
        self.main_thread == std::thread::current().id()
    }
}

impl Default for MainThreadValidator {
    fn default() -> Self {
        Self {
            main_thread: std::thread::current().id(),
        }
    }
}
