use std::any::TypeId;

use crate::core::{
    Bundle, Component, ComponentId, DynamicBundle, Entity, EntityLocation, Mut, StorageType, World,
};

pub struct EntityRef<'w> {
    world: &'w World,
    entity: Entity,
    location: EntityLocation,
}

impl<'w> EntityRef<'w> {
    #[inline]
    pub(crate) fn new(world: &'w World, entity: Entity, location: EntityLocation) -> Self {
        Self {
            world,
            entity,
            location,
        }
    }

    #[inline]
    pub fn id(&self) -> Entity {
        self.entity
    }

    pub fn get<T: Component>(&self) -> Option<&'w T> {
        // SAFE: entity location is valid and returned component is of type T
        unsafe {
            get_type_at_location_unchecked(
                self.world,
                TypeId::of::<T>(),
                self.entity,
                self.location,
            )
            .map(|value| &*value.cast::<T>())
        }
    }
}

pub struct EntityMut<'w> {
    world: &'w mut World,
    entity: Entity,
    location: EntityLocation,
}

impl<'w> EntityMut<'w> {
    #[inline]
    pub(crate) fn new(world: &'w mut World, entity: Entity, location: EntityLocation) -> Self {
        EntityMut {
            world,
            entity,
            location,
        }
    }

    #[inline]
    pub fn id(&self) -> Entity {
        self.entity
    }

    pub fn get<T: Component>(&self) -> Option<&'w T> {
        // SAFE: entity location is valid and returned component is of type T
        unsafe {
            get_type_at_location_unchecked(
                self.world,
                TypeId::of::<T>(),
                self.entity,
                self.location,
            )
            .map(|value| &*value.cast::<T>())
        }
    }

    pub fn get_mut<T: Component>(&mut self) -> Option<Mut<'w, T>> {
        // SAFE: world access is unique, entity location is valid, and returned component is of type T
        unsafe {
            get_type_at_location_unchecked(
                self.world,
                TypeId::of::<T>(),
                self.entity,
                self.location,
            )
            .map(|value| Mut {
                value: &mut *value.cast::<T>(),
            })
        }
    }

    // TODO: factor out non-generic part to cut down on monomorphization (just check perf) 
    pub fn add_bundle<T: DynamicBundle>(&mut self, bundle: T) -> &mut Self {
        let entities = &mut self.world.entities;
        let archetypes = &mut self.world.archetypes;
        let components = &mut self.world.components;
        let storages = &mut self.world.storages;

        let bundle_info = self
            .world
            .bundles
            .get_info_dynamic(archetypes, components, storages, &bundle);

        // SAFE: archetype was created if it didn't already exist
        let archetype = unsafe { archetypes.get_unchecked_mut(bundle_info.archetype_id) };
        let entity = entities.alloc();
        unsafe {
            let entity_location = archetype.allocate(entity, storages);
            self.location = entity_location;
            entities.meta[entity.id as usize].location = entity_location;
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
        self
    }

    pub fn remove_bundle<T: Bundle>(&mut self) -> Option<T> {
        todo!();
    }

    pub fn remove_bundle_one_by_one<T: Bundle>(&mut self) {
        todo!();
    }

    pub fn add<T: Component>(&mut self, value: T) -> &mut Self {
        todo!();
        self
    }

    pub fn remove<T: Component>(&mut self) -> Option<T> {
        todo!();
    }

    pub fn despawn(self) {
        let world = self.world;
        world.flush();
        let location = world.entities.free(self.entity);
        let (table_row, moved_entity) = {
            // SAFE: entity is live and is contained in an archetype that exists
            let archetype = unsafe { world.archetypes.get_unchecked_mut(location.archetype_id) };
            let table_row = archetype.swap_remove(location.index);

            // SAFE: tables stored in archetypes always exist
            let table = unsafe {
                world
                    .storages
                    .tables
                    .get_unchecked_mut(archetype.table_id())
            };

            for component_id in archetype.sparse_set_components() {
                // SAFE: component_ids stored in live archetypes are guaranteed to exist
                let sparse_set =
                    unsafe { world.storages.sparse_sets.get_mut_unchecked(*component_id) };
                sparse_set.remove_component(self.entity);
            }
            // SAFE: table rows stored in archetypes always exist
            let moved_entity = unsafe { table.swap_remove(table_row) };
            (table_row, moved_entity)
        };

        if let Some(moved_entity) = moved_entity {
            // PERF: entity is guaranteed to exist. we could skip a check here
            let moved_location = world.entities.get(moved_entity).unwrap();
            // SAFE: entity is live and is contained in an archetype that exists
            unsafe {
                let archetype = world
                    .archetypes
                    .get_unchecked_mut(moved_location.archetype_id);
                archetype.set_entity_table_row_unchecked(moved_location.index, table_row);
            };
        }
    }
}

/// This does not check that the location is within bounds of the archetype.
#[inline]
unsafe fn get_at_location_unchecked(
    world: &World,
    component_id: ComponentId,
    entity: Entity,
    location: EntityLocation,
) -> Option<*mut u8> {
    let components = &world.components;
    // SAFE: component_id exists and is therefore valid
    let component_info = components.get_info_unchecked(component_id);
    // SAFE: valid locations point to valid archetypes
    let archetype = world.archetypes.get_unchecked(location.archetype_id);
    match component_info.storage_type() {
        StorageType::Table => {
            let table = world.storages.tables.get_unchecked(archetype.table_id());
            // SAFE: archetypes will always point to valid columns
            let components = table.get_column_unchecked(component_id);
            let table_row = archetype.entity_table_row_unchecked(location.index);
            // SAFE: archetypes only store valid table_rows and the stored component type is T
            Some(components.get_unchecked(table_row))
        }
        StorageType::SparseSet => {
            let sparse_sets = &world.storages.sparse_sets;
            sparse_sets
                .get(component_id)
                .and_then(|sparse_set| sparse_set.get_component(entity))
        }
    }
}

/// This does not check that the location is within bounds of the archetype.
// TODO: remove inlining to cut down on monomorphization (just measure perf first)
#[inline]
unsafe fn get_type_at_location_unchecked(
    world: &World,
    type_id: TypeId,
    entity: Entity,
    location: EntityLocation,
) -> Option<*mut u8> {
    let components = &world.components;
    let component_id = components.get_id(type_id)?;
    get_at_location_unchecked(world, component_id, entity, location)
}
