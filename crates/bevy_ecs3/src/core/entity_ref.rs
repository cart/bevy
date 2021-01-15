use crate::core::{
    ArchetypeId, Archetypes, Bundle, BundleInfo, Component, ComponentId, Components, DynamicBundle,
    Entity, EntityLocation, Mut, StorageType, Storages, World,
};
use std::any::TypeId;

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
            get_component_with_type(self.world, TypeId::of::<T>(), self.entity, self.location)
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
            get_component_with_type(self.world, TypeId::of::<T>(), self.entity, self.location)
                .map(|value| &*value.cast::<T>())
        }
    }

    pub fn get_mut<T: Component>(&mut self) -> Option<Mut<'w, T>> {
        // SAFE: world access is unique, entity location is valid, and returned component is of type T
        unsafe {
            get_component_with_type(self.world, TypeId::of::<T>(), self.entity, self.location).map(
                |value| Mut {
                    value: &mut *value.cast::<T>(),
                },
            )
        }
    }

    // TODO: factor out non-generic part to cut down on monomorphization (just check perf)
    pub fn insert_bundle<T: DynamicBundle>(&mut self, bundle: T) -> &mut Self {
        let entity = self.entity;
        let entities = &mut self.world.entities;
        let archetypes = &mut self.world.archetypes;
        let components = &mut self.world.components;
        let storages = &mut self.world.storages;

        let bundle_info = self.world.bundles.get_info_dynamic(components, &bundle);

        // SAFE: component ids in `bundle_info` and self.location are valid
        let entity_location = unsafe {
            allocate_add_bundle_to_entity(
                archetypes,
                storages,
                components,
                bundle_info,
                entity,
                self.location,
            )
        };
        // TODO: ensure entities[meta].locaton = entity_lcoation
        self.location = entity_location;

        // SAFE: archetype was created if it didn't already exist
        let archetype = unsafe { archetypes.get_unchecked_mut(entity_location.archetype_id) };
        unsafe {
            self.location = entity_location;
            entities.meta[self.entity.id as usize].location = entity_location;
            // NOTE: put is called on each component in "bundle order". bundle_info.component_ids are also in "bundle order"
            let mut bundle_component = 0;
            bundle.put(|component_ptr| {
                // SAFE: component_id was initialized by get_dynamic_bundle_info
                let component_id = *bundle_info.component_ids.get_unchecked(bundle_component);
                let component_info = components.get_info_unchecked(component_id);
                match component_info.storage_type() {
                    StorageType::Table => {
                        let table = storages.tables.get_unchecked_mut(archetype.table_id());
                        table.put_component_unchecked(
                            component_id,
                            archetype.entity_table_row_unchecked(entity_location.index),
                            component_ptr,
                        );
                    }
                    StorageType::SparseSet => {
                        let sparse_set = storages.sparse_sets.get_mut(component_id).unwrap();
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

    pub fn insert<T: Component>(&mut self, value: T) -> &mut Self {
        self.insert_bundle((value,))
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

/// SAFETY: `entity_location` must be within bounds of an archetype that exists.
#[inline]
unsafe fn get_component(
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

/// SAFETY: `entity_location` must be within bounds of an archetype that exists.
// TODO: remove inlining to cut down on monomorphization (just measure perf first)
#[inline]
unsafe fn get_component_with_type(
    world: &World,
    type_id: TypeId,
    entity: Entity,
    location: EntityLocation,
) -> Option<*mut u8> {
    let components = &world.components;
    let component_id = components.get_id(type_id)?;
    get_component(world, component_id, entity, location)
}

/// Adds a bundle to the given archetype and returns the resulting archetype. This could be the same [ArchetypeId],
/// in the event that adding the given bundle does not result in an Archetype change. Results are cached in the
/// Archetype Graph to avoid redundant work.
/// SAFETY: `archetype_id` must exist and components in `bundle_info` must exist
unsafe fn add_bundle_to_archetype(
    archetypes: &mut Archetypes,
    storages: &mut Storages,
    components: &mut Components,
    archetype_id: ArchetypeId,
    bundle_info: &BundleInfo,
) -> ArchetypeId {
    let new_archetype_id = {
        let current_archetype = archetypes.get_unchecked_mut(archetype_id);
        current_archetype.edges().get_add_bundle(bundle_info.id)
    };
    if let Some(new_archetype_id) = new_archetype_id {
        new_archetype_id
    } else {
        let mut new_table_components = Vec::new();
        let mut new_sparse_set_components = Vec::new();

        {
            let current_archetype = archetypes.get_unchecked_mut(archetype_id);
            for component_id in bundle_info.component_ids.iter().cloned() {
                if !current_archetype.contains(component_id) {
                    let component_info = components.get_info_unchecked(component_id);
                    match component_info.storage_type() {
                        StorageType::Table => new_table_components.push(component_id),
                        StorageType::SparseSet => {
                            storages.sparse_sets.get_or_insert(component_info);
                            new_sparse_set_components.push(component_id)
                        },
                    }
                }
            }
        }

        if new_table_components.len() == 0 && new_sparse_set_components.len() == 0 {
            // the archetype does not change when we add this bundle
            archetype_id
        } else {
            // the archetype changes when we add this bundle. prepare the new archetype and storages
            let (table_id, table_components, sparse_set_components) = {
                let current_archetype = archetypes.get_unchecked_mut(archetype_id);
                let (table_id, table_components) = if new_table_components.len() == 0 {
                    // if there are no new table components, we can keep using this table
                    (
                        current_archetype.table_id(),
                        current_archetype.table_components().clone(),
                    )
                } else {
                    new_table_components.extend(current_archetype.table_components());
                    // sort to ignore order while hashing
                    new_table_components.sort();
                    // SAFE: all component ids in `new_table_components` exist
                    let table_id = storages
                        .tables
                        .get_id_or_insert(&new_table_components, components);

                    (table_id, new_table_components)
                };

                let sparse_set_components = if new_sparse_set_components.len() == 0 {
                    current_archetype.sparse_set_components().clone()
                } else {
                    new_sparse_set_components.extend(current_archetype.sparse_set_components());
                    // sort to ignore order while hashing
                    new_sparse_set_components.sort();
                    new_sparse_set_components
                };
                (table_id, table_components, sparse_set_components)
            };
            let new_archetype_id =
                archetypes.get_id_or_insert(table_id, table_components, sparse_set_components);
            let current_archetype = archetypes.get_unchecked_mut(archetype_id);
            // add an edge from the old archetype to the new archetype
            current_archetype
                .edges_mut()
                .set_add_bundle(bundle_info.id, new_archetype_id);
            new_archetype_id
        }
    }
}

/// SAFETY: `entity_location` must be valid and components in `bundle_info` must exist
unsafe fn allocate_add_bundle_to_entity(
    archetypes: &mut Archetypes,
    storages: &mut Storages,
    components: &mut Components,
    bundle_info: &BundleInfo,
    entity: Entity,
    location: EntityLocation,
) -> EntityLocation {
    let new_archetype_id = add_bundle_to_archetype(
        archetypes,
        storages,
        components,
        location.archetype_id,
        bundle_info,
    );
    if new_archetype_id == location.archetype_id {
        location
    } else {
        // SAFE: archetypes are valid and are not equal (due to the check above)
        // TODO: is this needed?
        let (old_archetype, new_archetype) =
            archetypes.get_2_mut_unchecked(location.archetype_id, new_archetype_id);

        let old_table_row = old_archetype.swap_remove(location.index);
        if old_archetype.table_id() == new_archetype.table_id() {
            new_archetype.allocate(entity, old_table_row)
        } else {
            let (old_table, new_table) = storages
                .tables
                .get_2_mut_unchecked(old_archetype.table_id(), new_archetype.table_id());
            let new_table_row = old_table.move_to_superset_unchecked(old_table_row, new_table);
            new_archetype.allocate(entity, new_table_row)
        }

        // Sparse set components are intentionally ignored here. They don't need to move
    }
}
