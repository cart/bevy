use crate::core::{
    Archetype, ArchetypeId, Archetypes, Bundle, BundleInfo, Component, ComponentId, Components,
    DynamicBundle, Entity, EntityLocation, Mut, StorageType, Storages, World,
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
    // TODO: move relevant methods to World (add/remove bundle)
    pub fn insert_bundle<T: DynamicBundle>(&mut self, bundle: T) -> &mut Self {
        let entity = self.entity;
        let entities = &mut self.world.entities;
        let archetypes = &mut self.world.archetypes;
        let components = &mut self.world.components;
        let storages = &mut self.world.storages;

        let bundle_info = self.world.bundles.init_info_dynamic(components, &bundle);
        let current_location = self.location;

        let new_location = unsafe {
            // SAFE: component ids in `bundle_info` and self.location are valid
            let new_archetype_id = add_bundle_to_archetype(
                archetypes,
                storages,
                components,
                self.location.archetype_id,
                bundle_info,
            );
            if new_archetype_id == current_location.archetype_id {
                current_location
            } else {
                let old_table_row;
                let old_table_id;
                {
                    let old_archetype = archetypes.get_unchecked_mut(current_location.archetype_id);
                    let result = old_archetype.swap_remove(current_location.index);
                    if let Some(swapped_entity) = result.swapped_entity {
                        // SAFE: entity is live and is contained in an archetype that exists
                        entities.meta[swapped_entity.id as usize].location = current_location;
                    }
                    old_table_row = result.table_row;
                    old_table_id = old_archetype.table_id()
                }
                let new_archetype = archetypes.get_unchecked_mut(new_archetype_id);

                if old_table_id == new_archetype.table_id() {
                    new_archetype.allocate(entity, old_table_row)
                } else {
                    let (old_table, new_table) = storages
                        .tables
                        .get_2_mut_unchecked(old_table_id, new_archetype.table_id());
                    // PERF: store "non bundle" components in edge, then just move those to avoid redundant copies
                    let move_result =
                        old_table.move_to_superset_unchecked(old_table_row, new_table);

                    let new_location = new_archetype.allocate(entity, move_result.new_row);
                    // if an entity was moved into this entity's table spot, update its table row
                    if let Some(swapped_entity) = move_result.swapped_entity {
                        // PERF: entity is guaranteed to exist. we could skip a check here
                        let swapped_location = entities.get(swapped_entity).unwrap();
                        // SAFE: entity is live and is contained in an archetype that exists
                        let archetype = archetypes.get_unchecked_mut(swapped_location.archetype_id);
                        archetype
                            .set_entity_table_row_unchecked(swapped_location.index, old_table_row);
                    }
                    new_location
                }

                // Sparse set components are intentionally ignored here. They don't need to move
            }
        };
        self.location = new_location;
        entities.meta[self.entity.id as usize].location = new_location;

        // SAFE: archetype was created if it didn't already exist
        let archetype = unsafe { archetypes.get_unchecked_mut(new_location.archetype_id) };
        // SAFE: table exists
        let table = unsafe { storages.tables.get_unchecked_mut(archetype.table_id()) };
        // SAFE: entity exists in archetype
        let table_row = unsafe { archetype.entity_table_row_unchecked(new_location.index) };
        // SAFE: table row is valid
        unsafe {
            bundle_info.put_components(&mut storages.sparse_sets, entity, table, table_row, bundle)
        };
        self
    }

    pub fn remove_bundle<T: Bundle>(&mut self) -> Option<T> {
        let archetypes = &mut self.world.archetypes;
        let storages = &mut self.world.storages;
        let components = &mut self.world.components;
        let entities = &mut self.world.entities;

        let bundle_info = self.world.bundles.init_info::<T>(components);
        let old_location = self.location;
        let new_archetype_id = unsafe {
            remove_bundle_from_archetype(
                archetypes,
                storages,
                components,
                old_location.archetype_id,
                bundle_info,
            )?
        };

        // PERF: consider removing empty bundles. then we could skip this check.
        if new_archetype_id == old_location.archetype_id {
            todo!("return T, which should be an empty bundle");
        }

        // SAFE: current entity archetype is valid
        let old_archetype = unsafe { archetypes.get_unchecked_mut(old_location.archetype_id) };
        let mut bundle_components = bundle_info.component_ids.iter().cloned();
        let entity = self.entity;
        // SAFE: bundle components are iterated in order, which guarantees that the component type matches
        let result = unsafe {
            T::get(|| {
                let component_id = bundle_components.next().unwrap();
                // SAFE: entity location is valid and table row is removed below
                remove_component(
                    components,
                    storages,
                    old_archetype,
                    component_id,
                    entity,
                    old_location,
                )
            })
        };

        let remove_result = old_archetype.swap_remove(old_location.index);
        if let Some(swapped_entity) = remove_result.swapped_entity {
            // SAFE: entity is live and is contained in an archetype that exists
            entities.meta[swapped_entity.id as usize].location = old_location;
        }
        let old_table_row = remove_result.table_row;
        let old_table_id = old_archetype.table_id();
        // SAFE: new archetype exists thanks to remove_bundle_from_archetype
        let new_archetype = unsafe { archetypes.get_unchecked_mut(new_archetype_id) };

        let new_location = if old_table_id == new_archetype.table_id() {
            unsafe { new_archetype.allocate(entity, old_table_row) }
        } else {
            // SAFE: tables stored in archetypes always exist and table ids are different
            let (old_table, new_table) = unsafe {
                storages
                    .tables
                    .get_2_mut_unchecked(old_table_id, new_archetype.table_id())
            };

            // SAFE: table_row exists. All "missing" components have been extracted into the bundle above and the caller takes ownership
            let move_result =
                unsafe { old_table.move_to_and_forget_missing_unchecked(old_table_row, new_table) };

            // SAFE: new_table_row is a valid position in new_archetype's table
            let new_location = unsafe { new_archetype.allocate(entity, move_result.new_row) };

            // if an entity was moved into this entity's table spot, update its table row
            if let Some(swapped_entity) = move_result.swapped_entity {
                // PERF: entity is guaranteed to exist. we could skip a check here
                let swapped_location = entities.get(swapped_entity).unwrap();
                // SAFE: entity is live and is contained in an archetype that exists
                unsafe {
                    let archetype = archetypes.get_unchecked_mut(swapped_location.archetype_id);
                    archetype.set_entity_table_row_unchecked(swapped_location.index, old_table_row);
                };
            }

            new_location
        };

        self.location = new_location;
        entities.meta[self.entity.id as usize].location = new_location;

        Some(result)
    }

    /// Remove any components in the bundle that the entity has.
    pub fn remove_bundle_intersection<T: Bundle>(&mut self) {
        todo!();
    }

    pub fn insert<T: Component>(&mut self, value: T) -> &mut Self {
        self.insert_bundle((value,))
    }

    pub fn remove<T: Component>(&mut self) -> Option<T> {
        self.remove_bundle::<(T,)>().map(|v| v.0)
    }

    pub fn despawn(self) {
        let world = self.world;
        world.flush();
        let location = world.entities.free(self.entity);
        let table_row;
        let moved_entity;
        {
            // SAFE: entity is live and is contained in an archetype that exists
            let archetype = unsafe { world.archetypes.get_unchecked_mut(location.archetype_id) };
            let remove_result = archetype.swap_remove(location.index);
            if let Some(swapped_entity) = remove_result.swapped_entity {
                // SAFE: entity is live and is contained in an archetype that exists
                world.entities.meta[swapped_entity.id as usize].location = location;
            }
            table_row = remove_result.table_row;

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
            moved_entity = unsafe { table.swap_remove(table_row) };
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

/// SAFETY: `entity_location` must be within bounds of the given archetype and `entity` must exist inside the archetype
#[inline]
unsafe fn get_component(
    world: &World,
    component_id: ComponentId,
    entity: Entity,
    location: EntityLocation,
) -> Option<*mut u8> {
    let archetype = world.archetypes.get_unchecked(location.archetype_id);
    // SAFE: component_id exists and is therefore valid
    let component_info = world.components.get_info_unchecked(component_id);
    match component_info.storage_type() {
        StorageType::Table => {
            let table = world.storages.tables.get_unchecked(archetype.table_id());
            // SAFE: archetypes will always point to valid columns
            let components = table.get_column_unchecked(component_id);
            let table_row = archetype.entity_table_row_unchecked(location.index);
            // SAFE: archetypes only store valid table_rows and the stored component type is T
            Some(components.get_unchecked(table_row))
        }
        StorageType::SparseSet => world
            .storages
            .sparse_sets
            .get(component_id)
            .and_then(|sparse_set| sparse_set.get_component(entity)),
    }
}

/// SAFETY: `entity_location` must be within bounds of the given archetype and `entity` must exist inside the archetype
/// The relevant table row must be removed separately
#[inline]
unsafe fn remove_component(
    components: &Components,
    storages: &mut Storages,
    archetype: &Archetype,
    component_id: ComponentId,
    entity: Entity,
    location: EntityLocation,
) -> *mut u8 {
    // SAFE: component_id exists and is therefore valid
    let component_info = components.get_info_unchecked(component_id);
    match component_info.storage_type() {
        StorageType::Table => {
            let table = storages.tables.get_unchecked(archetype.table_id());
            // SAFE: archetypes will always point to valid columns
            let components = table.get_column_unchecked(component_id);
            let table_row = archetype.entity_table_row_unchecked(location.index);
            // SAFE: archetypes only store valid table_rows and the stored component type is T
            components.get_unchecked(table_row)
        }
        StorageType::SparseSet => storages
            .sparse_sets
            .get_mut_unchecked(component_id)
            .remove_component_and_forget(entity)
            .unwrap(),
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
    let component_id = world.components.get_id(type_id)?;
    get_component(world, component_id, entity, location)
}

/// Adds a bundle to the given archetype and returns the resulting archetype. This could be the same [ArchetypeId],
/// in the event that adding the given bundle does not result in an Archetype change. Results are cached in the
/// Archetype Graph to avoid redundant work.
/// SAFETY: `archetype_id` must exist and components in `bundle_info` must exist
pub(crate) unsafe fn add_bundle_to_archetype(
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
                        }
                    }
                }
            }
        }

        if new_table_components.len() == 0 && new_sparse_set_components.len() == 0 {
            // the archetype does not change when we add this bundle
            archetype_id
        } else {
            let table_id;
            let table_components;
            let sparse_set_components;
            // the archetype changes when we add this bundle. prepare the new archetype and storages
            {
                let current_archetype = archetypes.get_unchecked_mut(archetype_id);
                table_components = if new_table_components.len() == 0 {
                    // if there are no new table components, we can keep using this table
                    table_id = current_archetype.table_id();
                    current_archetype.table_components().clone()
                } else {
                    new_table_components.extend(current_archetype.table_components());
                    // sort to ignore order while hashing
                    new_table_components.sort();
                    // SAFE: all component ids in `new_table_components` exist
                    table_id = storages
                        .tables
                        .get_id_or_insert(&new_table_components, components);

                    new_table_components
                };

                sparse_set_components = if new_sparse_set_components.len() == 0 {
                    current_archetype.sparse_set_components().clone()
                } else {
                    new_sparse_set_components.extend(current_archetype.sparse_set_components());
                    // sort to ignore order while hashing
                    new_sparse_set_components.sort();
                    new_sparse_set_components
                };
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

/// Removes a bundle from the given archetype and returns the resulting archetype (or None if the removal was invalid).
/// in the event that adding the given bundle does not result in an Archetype change. Results are cached in the
/// Archetype Graph to avoid redundant work.
/// SAFETY: `archetype_id` must exist and components in `bundle_info` must exist
unsafe fn remove_bundle_from_archetype(
    archetypes: &mut Archetypes,
    storages: &mut Storages,
    components: &mut Components,
    archetype_id: ArchetypeId,
    bundle_info: &BundleInfo,
) -> Option<ArchetypeId> {
    // check the archetype graph to see if the Bundle has been removed from this archetype in the past
    let remove_bundle_result = {
        // SAFE: entity location is valid and therefore the archetype exists
        let current_archetype = archetypes.get_unchecked_mut(archetype_id);
        current_archetype.edges().get_remove_bundle(bundle_info.id)
    };
    let result = if let Some(result) = remove_bundle_result {
        // this Bundle removal result is cached. just return that!
        result
    } else {
        let mut next_table_components;
        let mut next_sparse_set_components;
        let next_table_id;
        {
            // SAFE: entity location is valid and therefore the archetype exists
            let current_archetype = archetypes.get_unchecked_mut(archetype_id);
            let mut removed_table_components = Vec::new();
            let mut removed_sparse_set_components = Vec::new();
            for component_id in bundle_info.component_ids.iter().cloned() {
                if current_archetype.contains(component_id) {
                    // SAFE: bundle components were already initialized by bundles.get_info
                    let component_info = components.get_info_unchecked(component_id);
                    match component_info.storage_type() {
                        StorageType::Table => removed_table_components.push(component_id),
                        StorageType::SparseSet => removed_sparse_set_components.push(component_id),
                    }
                } else {
                    // a component in the bundle was not present in the entity's archetype, so this removal is invalid
                    // cache the result in the archetype graph
                    current_archetype
                        .edges_mut()
                        .set_remove_bundle(bundle_info.id, None);
                    return None;
                }
            }

            // sort removed components so we can do an efficient "sorted remove". archetype components are already sorted
            removed_table_components.sort();
            removed_sparse_set_components.sort();
            next_table_components = current_archetype.table_components().clone();
            next_sparse_set_components = current_archetype.sparse_set_components().clone();
            sorted_remove(&mut next_table_components, &removed_table_components);
            sorted_remove(
                &mut next_sparse_set_components,
                &removed_sparse_set_components,
            );

            next_table_id = if removed_table_components.len() == 0 {
                current_archetype.table_id()
            } else {
                // SAFE: all components in next_table_components exist
                storages
                    .tables
                    .get_id_or_insert(&next_table_components, components)
            };
        }

        let new_archetype_id = archetypes.get_id_or_insert(
            next_table_id,
            next_table_components,
            next_sparse_set_components,
        );
        Some(new_archetype_id)
    };
    // SAFE: entity location is valid and therefore the archetype exists
    let current_archetype = archetypes.get_unchecked_mut(archetype_id);
    // cache the result in an edge
    current_archetype
        .edges_mut()
        .set_remove_bundle(bundle_info.id, result);
    result
}

fn sorted_remove<T: Eq + Ord + Copy>(source: &mut Vec<T>, remove: &Vec<T>) {
    let mut remove_index = 0;
    source.retain(|value| {
        while remove_index < remove.len() && *value > remove[remove_index] {
            remove_index += 1;
        }

        if remove_index < remove.len() {
            *value != remove[remove_index]
        } else {
            true
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn sorted_remove() {
        let mut a = vec![1, 2, 3, 4, 5, 6, 7];
        let b = vec![1, 2, 3, 5, 7];
        super::sorted_remove(&mut a, &b);

        assert_eq!(a, vec![4, 6]);

        let mut a = vec![1];
        let b = vec![1];
        super::sorted_remove(&mut a, &b);

        assert_eq!(a, vec![]);

        let mut a = vec![1];
        let b = vec![2];
        super::sorted_remove(&mut a, &b);

        assert_eq!(a, vec![1]);
    }
}
