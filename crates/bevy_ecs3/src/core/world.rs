use std::{any::TypeId, collections::HashMap, fmt};
use thiserror::Error;

use crate::core::{Archetype, ArchetypeId, Archetypes, Bundle, Component, ComponentFlags, ComponentId, Components, DynamicBundle, Entities, Entity, Fetch, Location, Mut, NoSuchEntity, QueryFilter, QueryIter, QueryState, ReadOnlyFetch, SparseSets, StorageType, TypeInfo, WorldQuery};

#[derive(Debug)]
struct BundleInfo {
    archetype: ArchetypeId,
    storage_types: Vec<StorageType>,
    component_ids: Vec<ComponentId>,
}

#[derive(Default)]
struct BundleInfos {
    bundle_info: HashMap<TypeId, BundleInfo, fxhash::FxBuildHasher>,
}

impl BundleInfos {
    #[inline]
    fn get_dynamic<'a, T: DynamicBundle>(
        &'a mut self,
        bundle: &T,
        components: &mut Components,
        archetypes: &mut Archetypes,
        sparse_sets: &mut SparseSets,
    ) -> &'a BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let type_info = bundle.type_info();
                Self::get_bundle_info(type_info, components, archetypes, sparse_sets)
            })
    }

    #[inline]
    fn get_static<'a, T: Bundle>(
        &'a mut self,
        components: &mut Components,
        archetypes: &mut Archetypes,
        sparse_sets: &mut SparseSets,
    ) -> &'a BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let type_info = T::static_type_info();
                Self::get_bundle_info(type_info, components, archetypes, sparse_sets)
            })
    }

    #[inline]
    fn get_bundle_info(
        mut type_info: Vec<TypeInfo>,
        components: &mut Components,
        archetypes: &mut Archetypes,
        sparse_sets: &mut SparseSets,
    ) -> BundleInfo {
        let mut storage_types = Vec::with_capacity(type_info.len());
        let mut component_ids = Vec::with_capacity(type_info.len());

        // filter out non-archetype TypeInfo and collect component info
        type_info.retain(|type_info| {
            let component_id = components.init_type_info(type_info);
            // SAFE: component info either previously existed or was just initialized
            let component_info = unsafe { components.get_info_unchecked(component_id) };
            component_ids.push(component_id);
            storage_types.push(component_info.storage_type);
            if component_info.storage_type == StorageType::SparseSet {
                sparse_sets.get_or_insert(component_id, type_info);
            }

            component_info.storage_type == StorageType::Archetype
        });

        let archetype_id = archetypes.get_or_insert(type_info);
        BundleInfo {
            archetype: archetype_id,
            storage_types,
            component_ids,
        }
    }
}

#[derive(Default)]
pub struct World {
    entities: Entities,
    components: Components,
    archetypes: Archetypes,
    sparse_sets: SparseSets,
    bundle_infos: BundleInfos,
}

impl World {
    pub fn new() -> World {
        World::default()
    }

    #[inline]
    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    #[inline]
    pub fn entities_mut(&mut self) -> &mut Entities {
        &mut self.entities
    }

    #[inline]
    pub fn archetypes(&self) -> &Archetypes {
        &self.archetypes
    }

    #[inline]
    pub fn archetypes_mut(&mut self) -> &mut Archetypes {
        &mut self.archetypes
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
    pub fn sparse_sets(&self) -> &SparseSets {
        &self.sparse_sets
    }

    #[inline]
    pub fn sparse_sets_mut(&mut self) -> &mut SparseSets {
        &mut self.sparse_sets
    }

    pub fn spawn(&mut self, bundle: impl DynamicBundle) -> Entity {
        self.flush();
        let bundle_info = self.bundle_infos.get_dynamic(
            &bundle,
            &mut self.components,
            &mut self.archetypes,
            &mut self.sparse_sets,
        );

        // SAFE: archetype was created if it didn't already exist
        let archetype = unsafe { self.archetypes.get_unchecked_mut(bundle_info.archetype) };
        let entity = self.entities.alloc();
        unsafe {
            let archetype_index = archetype.allocate(entity);
            let mut bundle_index = 0;
            let sparse_sets = &mut self.sparse_sets;
            bundle.put(|ptr, ty, size| {
                // TODO: instead of using type, use index to cut down on hashing
                // TODO: sort by TypeId instead of alignment for clearer results?
                // TODO: use ComponentId instead of TypeId in archetype?
                match bundle_info.storage_types[bundle_index] {
                    StorageType::Archetype => {
                        archetype.put_component(
                            ptr,
                            ty,
                            size,
                            archetype_index,
                            ComponentFlags::ADDED,
                        );
                    }
                    StorageType::SparseSet => {
                        let component_id = bundle_info.component_ids[bundle_index];
                        let sparse_set = sparse_sets.get_mut(component_id).unwrap();
                        sparse_set.put_component(entity, ptr, ComponentFlags::ADDED);
                    }
                }
                bundle_index += 1;
                true
            });
            self.entities.meta[entity.id as usize].location = Location {
                archetype: bundle_info.archetype,
                index: archetype_index,
            };
        }
        entity
    }

    /// Efficiently spawn a large number of entities with the same components
    ///
    /// Faster than calling `spawn` repeatedly with the same components.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let entities = world.spawn_batch((0..1_000).map(|i| (i, "abc"))).collect::<Vec<_>>();
    /// for i in 0..1_000 {
    ///     assert_eq!(*world.get::<i32>(entities[i]).unwrap(), i as i32);
    /// }
    /// ```
    pub fn spawn_batch<I>(&mut self, iter: I) -> SpawnBatchIter<'_, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: Bundle,
    {
        // Ensure all entity allocations are accounted for so `self.entities` can realloc if
        // necessary
        self.flush();

        let iter = iter.into_iter();
        let (lower, upper) = iter.size_hint();

        let bundle_info = self.bundle_infos.get_static::<I::Item>(
            &mut self.components,
            &mut self.archetypes,
            &mut self.sparse_sets,
        );

        let archetype = self.archetypes.get_mut(bundle_info.archetype).unwrap();
        let length = upper.unwrap_or(lower);
        archetype.reserve(length);
        self.entities.reserve(length as u32);
        SpawnBatchIter {
            inner: iter,
            entities: &mut self.entities,
            archetype_id: bundle_info.archetype,
            archetype,
        }
    }

    pub fn despawn(&mut self, entity: Entity) -> Result<(), NoSuchEntity> {
        self.flush();

        let location = self.entities.free(entity)?;
        // SAFE: the entity is guaranteed to exist inside this archetype because all stored Locations are valid
        if let Some(moved) = unsafe { self.archetypes.remove_entity_unchecked(entity, location) } {
            // update the moved entity's location to account for the fact that it was
            self.entities.get_mut(moved).unwrap().index = location.index;
        }

        self.sparse_sets.remove_entity(entity);
        Ok(())
    }

    pub fn insert(
        &mut self,
        entity: Entity,
        bundle: impl DynamicBundle,
    ) -> Result<(), NoSuchEntity> {
        todo!()
    }

    /// Add `component` to `entity`
    ///
    /// See `insert`.
    pub fn insert_one(
        &mut self,
        entity: Entity,
        component: impl Component,
    ) -> Result<(), NoSuchEntity> {
        self.insert(entity, (component,))
    }

    /// Borrow the `T` component of `entity` without checking if it can be mutated
    ///
    /// # Safety
    /// This does not check for mutable access correctness. To be safe, make sure this is the only
    /// thing accessing this entity's T component.
    #[inline]
    pub unsafe fn get_mut_unchecked<T: Component>(
        &self,
        entity: Entity,
    ) -> Result<Mut<'_, T>, ComponentError> {
        let location = self.entities.get(entity)?;
        // SAFE: location is valid
        self.get_mut_at_location_unchecked(entity, location)
    }

    pub fn remove<T: Bundle>(&mut self, entity: Entity) -> Result<T, ComponentError> {
        todo!()
    }

    pub fn remove_one_by_one<T: Bundle>(&mut self, entity: Entity) -> Result<(), ComponentError> {
        todo!()
    }

    /// Remove the `T` component from `entity`
    ///
    /// See `remove`.
    pub fn remove_one<T: Component>(&mut self, entity: Entity) -> Result<T, ComponentError> {
        self.remove::<(T,)>(entity).map(|(x,)| x)
    }

    /// Efficiently iterate over all entities that have certain components
    ///
    /// Calling `iter` on the returned value yields `(Entity, Q)` tuples, where `Q` is some query
    /// type. A query type is `&T`, `&mut T`, a tuple of query types, or an `Option` wrapping a
    /// query type, where `T` is any component type. Components queried with `&mut` must only appear
    /// once. Entities which do not have a component type referenced outside of an `Option` will be
    /// skipped.
    ///
    /// Entities are yielded in arbitrary order.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let a = world.spawn((123, true, "abc"));
    /// let b = world.spawn((456, false));
    /// let c = world.spawn((42, "def"));
    /// let entities = world.query::<(Entity, &i32, &bool)>()
    ///     .map(|(e, &i, &b)| (e, i, b)) // Copy out of the world
    ///     .collect::<Vec<_>>();
    /// assert_eq!(entities.len(), 2);
    /// assert!(entities.contains(&(a, 123, true)));
    /// assert!(entities.contains(&(b, 456, false)));
    /// ```
    #[inline]
    pub fn query<Q: WorldQuery>(&self) -> QueryIter<'_, '_, Q, ()>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_unchecked() }
    }

    #[inline]
    pub fn query_with_state<'w, 's, Q: WorldQuery>(
        &'w self,
        state: &'s mut QueryState,
    ) -> QueryIter<'w, 's, Q, ()> {
        unsafe { QueryIter::new(self, state) }
    }

    #[inline]
    pub fn query_filtered<Q: WorldQuery, F: QueryFilter>(&self) -> QueryIter<'_, '_, Q, F>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_unchecked() }
    }

    /// Efficiently iterate over all entities that have certain components
    ///
    /// Calling `iter` on the returned value yields `(Entity, Q)` tuples, where `Q` is some query
    /// type. A query type is `&T`, `&mut T`, a tuple of query types, or an `Option` wrapping a
    /// query type, where `T` is any component type. Components queried with `&mut` must only appear
    /// once. Entities which do not have a component type referenced outside of an `Option` will be
    /// skipped.
    ///
    /// Entities are yielded in arbitrary order.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let a = world.spawn((123, true, "abc"));
    /// let b = world.spawn((456, false));
    /// let c = world.spawn((42, "def"));
    /// let entities = world.query_mut::<(Entity, &mut i32, &bool)>()
    ///     .map(|(e, i, &b)| (e, *i, b)) // Copy out of the world
    ///     .collect::<Vec<_>>();
    /// assert_eq!(entities.len(), 2);
    /// assert!(entities.contains(&(a, 123, true)));
    /// assert!(entities.contains(&(b, 456, false)));
    /// ```
    #[inline]
    pub fn query_mut<Q: WorldQuery>(&mut self) -> QueryIter<'_, '_, Q, ()> {
        // SAFE: unique mutable access
        unsafe { self.query_unchecked() }
    }

    #[inline]
    pub fn query_filtered_mut<Q: WorldQuery, F: QueryFilter>(&mut self) -> QueryIter<'_, '_, Q, F> {
        // SAFE: unique mutable access
        unsafe { self.query_unchecked() }
    }

    /// Efficiently iterate over all entities that have certain components
    ///
    /// Calling `iter` on the returned value yields `(Entity, Q)` tuples, where `Q` is some query
    /// type. A query type is `&T`, `&mut T`, a tuple of query types, or an `Option` wrapping a
    /// query type, where `T` is any component type. Components queried with `&mut` must only appear
    /// once. Entities which do not have a component type referenced outside of an `Option` will be
    /// skipped.
    ///
    /// Entities are yielded in arbitrary order.
    ///
    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn query_unchecked<Q: WorldQuery, F: QueryFilter>(&self) -> QueryIter<'_, '_, Q, F> {
        QueryIter::new(&self.archetypes)
    }

    // /// Like `query`, but instead of returning a single iterator it returns a "batched iterator",
    // /// where each batch is `batch_size`. This is generally used for parallel iteration.
    // ///
    // /// # Safety
    // /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    // /// have unique access to the components they query.
    // #[inline]
    // pub unsafe fn query_batched_unchecked<Q: WorldQuery, F: QueryFilter>(
    //     &self,
    //     batch_size: usize,
    // ) -> BatchedIter<'_, Q, F> {
    //     BatchedIter::new(&self.archetypes, batch_size)
    // }

    /// Borrow the `T` component of `entity`
    #[inline]
    pub fn get<T: Component>(&self, entity: Entity) -> Result<&'_ T, ComponentError> {
        let location = self.entities.get(entity)?;
        // SAFE: location is valid
        unsafe { self.get_at_location_unchecked(entity, location) }
    }

    /// Mutably borrow the `T` component of `entity`
    #[inline]
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Result<Mut<'_, T>, ComponentError> {
        // SAFE: unique access to self
        unsafe { self.get_mut_unchecked(entity) }
    }

    /// Borrow the `T` component at the given location, without safety checks
    /// # Safety
    /// This does not check that the location is within bounds of the archetype.
    #[inline]
    pub unsafe fn get_at_location_unchecked<T: Component>(
        &self,
        entity: Entity,
        location: Location,
    ) -> Result<&T, ComponentError> {
        let components = self.components();
        let component_id = components
            .get_id(TypeId::of::<T>())
            .ok_or_else(ComponentError::missing_component::<T>)?;
        // SAFE: component_id exist and is therefore valid
        let component_info = unsafe { components.get_info_unchecked(component_id) };
        match component_info.storage_type {
            StorageType::Archetype => {
                unsafe {
                    // SAFE: valid locations point to valid archetypes
                    let archetype = self.archetypes().get_unchecked(location.archetype);
                    let components = archetype
                        .get::<T>()
                        .ok_or_else(ComponentError::missing_component::<T>)?;
                    Ok(&*components.as_ptr().add(location.index as usize))
                }
            }
            StorageType::SparseSet => {
                let component = self
                    .sparse_sets()
                    .get(component_id)
                    .and_then(|sparse_set| sparse_set.get_component(entity))
                    .ok_or_else(ComponentError::missing_component::<T>)?;
                // SAFE: component is of type T
                unsafe { Ok(&*component.cast::<T>()) }
            }
        }
    }

    /// Borrow the `T` component at the given location, without safety checks
    /// # Safety
    /// This does not check that the location is within bounds of the archetype.
    /// It also does not check for mutable access correctness. To be safe, make sure this is the only
    /// thing accessing this entity's T component.
    #[inline]
    pub unsafe fn get_mut_at_location_unchecked<T: Component>(
        &self,
        entity: Entity,
        location: Location,
    ) -> Result<Mut<T>, ComponentError> {
        let components = self.components();
        let component_id = components
            .get_id(TypeId::of::<T>())
            .ok_or_else(ComponentError::missing_component::<T>)?;
        // SAFE: component_id exist and is therefore valid
        let component_info = unsafe { components.get_info_unchecked(component_id) };
        match component_info.storage_type {
            StorageType::Archetype => {
                unsafe {
                    // SAFE: valid locations point to valid archetypes
                    let archetype = self.archetypes().get_unchecked(location.archetype);
                    let (components, type_state) = archetype
                        .get_with_type_state::<T>()
                        .ok_or_else(ComponentError::missing_component::<T>)?;
                    Ok(Mut {
                        value: &mut *components.as_ptr().add(location.index as usize),
                        flags: &mut *type_state
                            .component_flags()
                            .as_ptr()
                            .add(location.index as usize),
                    })
                }
            }
            StorageType::SparseSet => {
                let set = self
                    .sparse_sets()
                    .get(component_id)
                    .ok_or_else(ComponentError::missing_component::<T>)?;
                let component = set
                    .get_component(entity)
                    .ok_or_else(ComponentError::missing_component::<T>)?;
                // SAFE: component exists, therefore it has flags
                let flags = unsafe { set.get_component_flags_unchecked_mut(entity) };
                unsafe {
                    Ok(Mut {
                        // SAFE: component is of type T
                        value: unsafe { &mut *component.cast::<T>() },
                        flags,
                    })
                }
            }
        }
    }

    pub fn clear_trackers(&mut self) {
        self.archetypes.clear_trackers();
    }

    pub fn removed<C: Component>(&self) -> &[Entity] {
        self.archetypes.removed::<C>()
    }

    /// Despawn all entities
    ///
    /// Preserves allocated storage for reuse.
    pub fn clear(&mut self) {
        self.archetypes.clear();
        self.entities.clear();
    }

    fn flush(&mut self) {
        self.archetypes.flush_entities(&mut self.entities);
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "World")
    }
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

/// Errors that arise when accessing components
#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ComponentError {
    /// The entity was already despawned
    #[error("Entity does not exist.")]
    NoSuchEntity,
    /// The entity did not have a requested component
    #[error("Entity does not have the given component {0:?}.")]
    MissingComponent(&'static str),
}
impl From<NoSuchEntity> for ComponentError {
    fn from(NoSuchEntity: NoSuchEntity) -> Self {
        ComponentError::NoSuchEntity
    }
}

impl ComponentError {
    pub fn missing_component<T: Component>() -> Self {
        ComponentError::MissingComponent(std::any::type_name::<T>())
    }
}

/// Entity IDs created by `World::spawn_batch`
pub struct SpawnBatchIter<'a, I>
where
    I: Iterator,
    I::Item: Bundle,
{
    inner: I,
    entities: &'a mut Entities,
    archetype_id: ArchetypeId,
    archetype: &'a mut Archetype,
}

impl<I> Drop for SpawnBatchIter<'_, I>
where
    I: Iterator,
    I::Item: Bundle,
{
    fn drop(&mut self) {
        for _ in self {}
    }
}

impl<I> Iterator for SpawnBatchIter<'_, I>
where
    I: Iterator,
    I::Item: Bundle,
{
    type Item = Entity;

    fn next(&mut self) -> Option<Entity> {
        let components = self.inner.next()?;
        let entity = self.entities.alloc();
        unsafe {
            let index = self.archetype.allocate(entity);
            components.put(|ptr, ty, size| {
                self.archetype
                    .put_component(ptr, ty, size, index, ComponentFlags::ADDED);
                true
            });
            self.entities.meta[entity.id as usize].location = Location {
                archetype: self.archetype_id,
                index,
            };
        }
        Some(entity)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<I, T> ExactSizeIterator for SpawnBatchIter<'_, I>
where
    I: ExactSizeIterator<Item = T>,
    T: Bundle,
{
    fn len(&self) -> usize {
        self.inner.len()
    }
}
