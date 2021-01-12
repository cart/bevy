use std::{any::TypeId, collections::HashMap, fmt};
use thiserror::Error;

use crate::core::{
    Archetype, ArchetypeId, Archetypes, Bundle, Component, ComponentFlags, ComponentId, Components,
    DynamicBundle, Entities, Entity, EntityLocation, Fetch, Mut, NoSuchEntity, QueryFilter,
    QueryIter, QueryState, ReadOnlyFetch, SparseSets, StorageType, Tables, TypeInfo, WorldQuery,
};

use super::Storages;

struct BundleInfo {
    archetype_id: ArchetypeId,
    component_ids: Vec<ComponentId>,
}

pub struct World {
    entities: Entities,
    components: Components,
    archetypes: Archetypes,
    storages: Storages,
    bundles: Bundles,
    empty_archetype_id: ArchetypeId,
}

impl World {
    pub fn new() -> World {
        let components = Components::default();
        let mut storages = Storages::default();
        // SAFE: no component ids passed in
        let empty_table_id = unsafe { storages.tables.get_id_or_insert(&[], &components) };
        let mut archetypes = Archetypes::default();
        let empty_archetype_id =
            archetypes.get_id_or_insert(empty_table_id, Vec::new(), Vec::new());
        World {
            archetypes,
            components,
            storages,
            empty_archetype_id,
            entities: Entities::default(),
            bundles: Bundles::default(),
        }
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

    pub fn spawn<T: DynamicBundle>(&mut self, bundle: T) -> Entity {
        self.flush();
        let components = &mut self.components;
        let storages = &mut self.storages;
        let bundle_info =
            self.bundles
                .get_info_dynamic(&mut self.archetypes, components, storages, &bundle);

        // SAFE: archetype was created if it didn't already exist
        let archetype = unsafe { self.archetypes.get_unchecked_mut(bundle_info.archetype_id) };
        let entity = self.entities.alloc();
        unsafe {
            let entity_location = archetype.allocate(entity, storages);
            self.entities.meta[entity.id as usize].location = entity_location;
            let table = storages.tables.get_unchecked_mut(archetype.table_id());
            let sparse_sets = &mut storages.sparse_sets;
            // NOTE: put is called on each component in "bundle order". bundle_info.component_ids are also in "bundle order"
            let mut bundle_component = 0;
            bundle.put(|component_ptr| {
                // SAFE: component_id was initialized by get_dynamic_bundle_info
                let component_id = *bundle_info.component_ids.get_unchecked(bundle_component);
                let component_info = components.get_info_unchecked(component_id);
                match component_info.storage_type() {
                    StorageType::Table => {
                        table.put_component_unchecked(
                            component_id,
                            archetype.entity_table_row_unchecked(entity_location.index),
                            component_ptr,
                        );
                    }
                    StorageType::SparseSet => {
                        let sparse_set = sparse_sets.get_mut(component_id).unwrap();
                        sparse_set.put_component(entity, component_ptr);
                    }
                }
                bundle_component += 1;
            });
        }
        entity
    }

    // /// Efficiently spawn a large number of entities with the same components
    // ///
    // /// Faster than calling `spawn` repeatedly with the same components.
    // ///
    // /// # Example
    // /// ```
    // /// # use bevy_ecs::*;
    // /// let mut world = World::new();
    // /// let entities = world.spawn_batch((0..1_000).map(|i| (i, "abc"))).collect::<Vec<_>>();
    // /// for i in 0..1_000 {
    // ///     assert_eq!(*world.get::<i32>(entities[i]).unwrap(), i as i32);
    // /// }
    // /// ```
    // pub fn spawn_batch<I>(&mut self, iter: I) -> SpawnBatchIter<'_, I::IntoIter>
    // where
    //     I: IntoIterator,
    //     I::Item: Bundle,
    // {
    //     // Ensure all entity allocations are accounted for so `self.entities` can realloc if
    //     // necessary
    //     self.flush();

    //     let iter = iter.into_iter();
    //     let (lower, upper) = iter.size_hint();

    //     let bundle_info = self.get_static_bundle_archetype::<I::Item>();

    //     let archetype = self.archetypes.get_mut(bundle_info.archetype).unwrap();
    //     let length = upper.unwrap_or(lower);
    //     archetype.reserve(length);
    //     self.entities.reserve(length as u32);
    //     SpawnBatchIter {
    //         inner: iter,
    //         entities: &mut self.entities,
    //         archetype_id: bundle_info.archetype,
    //         archetype,
    //     }
    // }

    pub fn despawn(&mut self, entity: Entity) -> Result<(), NoSuchEntity> {
        self.flush();

        let location = self.entities.free(entity)?;
        let (table_row, moved_entity) = {
            // SAFE: entity is live and is contained in an archetype that exists
            let archetype = unsafe { self.archetypes.get_unchecked_mut(location.archetype_id) };
            let table_row = archetype.swap_remove(location.index);

            // SAFE: tables stored in archetypes always exist
            let table = unsafe { self.storages.tables.get_unchecked_mut(archetype.table_id()) };

            for component_id in archetype.sparse_set_components() {
                // SAFE: component_ids stored in live archetypes are guaranteed to exist
                let sparse_set =
                    unsafe { self.storages.sparse_sets.get_mut_unchecked(*component_id) };
                sparse_set.remove_component(entity);
            }
            // SAFE: table rows stored in archetypes always exist
            let moved_entity = unsafe { table.swap_remove(table_row) };
            (table_row, moved_entity)
        };

        if let Some(moved_entity) = moved_entity {
            // PERF: entity is guaranteed to exist. we could skip a check here
            let moved_location = self.entities.get(moved_entity).unwrap();
            // SAFE: entity is live and is contained in an archetype that exists
            unsafe {
                let archetype = self
                    .archetypes
                    .get_unchecked_mut(moved_location.archetype_id);
                archetype.set_entity_table_row_unchecked(moved_location.index, table_row);
            };
        }
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
        panic!()
        // QueryIter::new(&self.archetypes)
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
        location: EntityLocation,
    ) -> Result<&T, ComponentError> {
        let components = self.components();
        let component_id = components
            .get_id(TypeId::of::<T>())
            .ok_or_else(ComponentError::missing_component::<T>)?;
        // SAFE: component_id exists and is therefore valid
        let component_info = components.get_info_unchecked(component_id);
        // SAFE: valid locations point to valid archetypes
        let archetype = self.archetypes().get_unchecked(location.archetype_id);
        match component_info.storage_type() {
            StorageType::Table => {
                let table = self.storages.tables.get_unchecked(archetype.table_id());
                // SAFE: archetypes will always point to valid columns
                let components = table.get_column_unchecked(component_id);
                let table_row = archetype.entity_table_row_unchecked(location.index);
                // SAFE: archetypes only store valid table_rows and the stored component type is T
                Ok(components.get_type_unchecked(table_row))
            }
            StorageType::SparseSet => {
                let sparse_sets = &self.storages.sparse_sets;
                let component = sparse_sets
                    .get(component_id)
                    .and_then(|sparse_set| sparse_set.get_component(entity))
                    .ok_or_else(ComponentError::missing_component::<T>)?;
                // SAFE: component is of type T
                Ok(&*component.cast::<T>())
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
        location: EntityLocation,
    ) -> Result<Mut<T>, ComponentError> {
        let components = self.components();
        let component_id = components
            .get_id(TypeId::of::<T>())
            .ok_or_else(ComponentError::missing_component::<T>)?;
        // SAFE: component_id exists and is therefore valid
        let component_info = components.get_info_unchecked(component_id);
        // SAFE: valid locations point to valid archetypes
        let archetype = self.archetypes().get_unchecked(location.archetype_id);
        match component_info.storage_type() {
            StorageType::Table => {
                let table = self.storages.tables.get_unchecked(archetype.table_id());
                // SAFE: archetypes will always point to valid columns (caller verifies that mutability rules are not violated)
                let table_components = table.get_column_unchecked_mut(component_id);
                // SAFE: archetype entities always have valid table locations
                let table_row = archetype.entity_table_row_unchecked(location.index);
                // SAFE: archetypes only store valid table_rows and the stored component type is T
                Ok(Mut {
                    value: table_components.get_type_mut_unchecked(table_row),
                })
            }
            StorageType::SparseSet => {
                let sparse_sets = &self.storages.sparse_sets;
                let set = sparse_sets
                    .get(component_id)
                    .ok_or_else(ComponentError::missing_component::<T>)?;
                let component = set
                    .get_component(entity)
                    .ok_or_else(ComponentError::missing_component::<T>)?;
                // SAFE: component exists, therefore it has flags
                // let flags = unsafe { set.get_component_flags_unchecked_mut(entity) };
                Ok(Mut {
                    // SAFE: component is of type T
                    value: &mut *component.cast::<T>(),
                    // flags,
                })
            }
        }
    }

    fn flush(&mut self) {
        // SAFE: empty archetype is initialized when the world is constructed
        unsafe {
            let empty_archetype = self.archetypes.get_unchecked_mut(self.empty_archetype_id);
            let storages = &mut self.storages;
            self.entities.flush(|entity, location| {
                *location = empty_archetype.allocate(entity, storages);
            });
        }
    }
}

#[derive(Default)]
struct Bundles {
    bundle_info: HashMap<TypeId, BundleInfo>,
}

impl Bundles {
    fn get_info_dynamic<T: DynamicBundle>(
        &mut self,
        archetypes: &mut Archetypes,
        components: &mut Components,
        storages: &mut Storages,
        bundle: &T,
    ) -> &BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let type_info = bundle.type_info();
                Self::initialize_bundle(&type_info, archetypes, components, storages)
            })
    }

    fn get_info<T: Bundle>(
        &mut self,
        archetypes: &mut Archetypes,
        components: &mut Components,
        storages: &mut Storages,
    ) -> &BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let type_info = T::static_type_info();
                Self::initialize_bundle(&type_info, archetypes, components, storages)
            })
    }

    fn initialize_bundle(
        type_info: &[TypeInfo],
        archetypes: &mut Archetypes,
        components: &mut Components,
        storages: &mut Storages,
    ) -> BundleInfo {
        let mut table_components = Vec::new();
        let mut sparse_set_components = Vec::new();
        let mut component_ids = Vec::new();

        for type_info in type_info {
            let component_id = components.add_with_type_info(&type_info);
            component_ids.push(component_id);
            // SAFE: component info either previously existed or was just initialized
            let component_info = unsafe { components.get_info_unchecked(component_id) };
            match component_info.storage_type() {
                StorageType::SparseSet => {
                    sparse_set_components.push(component_id);
                    storages.sparse_sets.get_or_insert(component_info);
                }
                StorageType::Table => table_components.push(component_id),
            }
        }

        // sort to make hashes match across different orders
        table_components.sort();
        sparse_set_components.sort();

        // SAFE: component_ids were initialized above
        let table_id = unsafe {
            storages
                .tables
                .get_id_or_insert(&table_components, &components)
        };

        let archetype_id =
            archetypes.get_id_or_insert(table_id, table_components, sparse_set_components);
        BundleInfo {
            archetype_id,
            component_ids,
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

// /// Entity IDs created by `World::spawn_batch`
// pub struct SpawnBatchIter<'a, I>
// where
//     I: Iterator,
//     I::Item: Bundle,
// {
//     inner: I,
//     entities: &'a mut Entities,
//     archetype_id: ArchetypeId,
//     archetype: &'a mut Archetype,
// }

// impl<I> Drop for SpawnBatchIter<'_, I>
// where
//     I: Iterator,
//     I::Item: Bundle,
// {
//     fn drop(&mut self) {
//         for _ in self {}
//     }
// }

// impl<I> Iterator for SpawnBatchIter<'_, I>
// where
//     I: Iterator,
//     I::Item: Bundle,
// {
//     type Item = Entity;

//     fn next(&mut self) -> Option<Entity> {
//         let components = self.inner.next()?;
//         let entity = self.entities.alloc();
//         unsafe {
//             let index = self.archetype.allocate(entity);
//             components.put(|ptr, ty, size| {
//                 self.archetype
//                     .put_component(ptr, ty, size, index, ComponentFlags::ADDED);
//                 true
//             });
//             self.entities.meta[entity.id as usize].location = EntityLocation {
//                 archetype: self.archetype_id,
//                 index,
//             };
//         }
//         Some(entity)
//     }

//     fn size_hint(&self) -> (usize, Option<usize>) {
//         self.inner.size_hint()
//     }
// }

// impl<I, T> ExactSizeIterator for SpawnBatchIter<'_, I>
// where
//     I: ExactSizeIterator<Item = T>,
//     T: Bundle,
// {
//     fn len(&self) -> usize {
//         self.inner.len()
//     }
// }
