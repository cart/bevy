use std::{any::TypeId, fmt};

use bevy_utils::HashMap;

use crate::{Archetype, ArchetypeId, Archetypes, Bundle, Component, ComponentFlags, DynamicBundle, Entity, Fetch, Location, MissingComponent, Mut, NoSuchEntity, QueryFilter, QueryIter, SparseSets, StorageType, WorldQuery};

use super::{
    component::{ComponentId, Components},
    entities::Entities,
};

#[derive(Debug)]
struct BundleInfo {
    archetype: ArchetypeId,
    archetype_components: Vec<ComponentId>,
    sparse_set_components: Vec<ComponentId>,
}

#[derive(Default)]
struct BundleInfos {
    bundle_info: HashMap<TypeId, BundleInfo>,
}

impl BundleInfos {
    fn get<'a, T: DynamicBundle>(
        &'a mut self,
        bundle: &T,
        components: &mut Components,
        archetypes: &mut Archetypes,
    ) -> &'a BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let mut type_info = bundle.type_info();
                let mut archetype_components = Vec::new();
                let mut sparse_set_components = Vec::new();
                type_info.retain(|type_info| {
                    let component_id = components.init_type_info(type_info);
                    let component_info = components.get_info(component_id).unwrap();
                    match component_info.storage_type {
                        StorageType::Archetype => {
                            archetype_components.push(component_id);
                            true
                        }
                        StorageType::SparseSet => {
                            sparse_set_components.push(component_id);
                            false
                        }
                    }
                });

                let archetype_id = archetypes.get_or_insert_archetype(type_info);
                BundleInfo {
                    archetype: archetype_id,
                    archetype_components,
                    sparse_set_components,
                }
            })
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
    pub fn spawn(&mut self, bundle: impl DynamicBundle) -> Entity {
        self.flush();
        let bundle_info =
            self.bundle_infos
                .get(&bundle, &mut self.components, &mut self.archetypes);
        let archetype = self.archetypes.get_mut(bundle_info.archetype).unwrap();
        let entity = self.entities.alloc();
        unsafe {
            let index = archetype.allocate(entity);
            bundle.put(|ptr, ty, size| {
                // TODO: if arch add, otherwise add to sparse set
                // TODO: instead of using type, use index to cut down on hashing
                // TODO: sort by TypeId instead of alignment for clearer results?
                // TODO: use ComponentId instead of TypeId in archetype?
                archetype.put_dynamic(ptr, ty, size, index, ComponentFlags::ADDED);
                true
            });
            self.entities.meta[entity.id as usize].location = Location {
                // TODO: use ArchetypeId directly here
                archetype: bundle_info.archetype.0,
                index,
            };
        }
        entity
    }

    pub fn spawn_batch<I>(&mut self, iter: I) -> SpawnBatchIter<'_, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: Bundle,
    {
        todo!()
    }

    pub fn despawn(&mut self, entity: Entity) -> Result<(), NoSuchEntity> {
        todo!()
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

    /// Borrow the `T` component of `entity`
    #[inline]
    pub fn get<T: Component>(&self, entity: Entity) -> Result<&'_ T, ComponentError> {
        todo!()
    }

    /// Mutably borrow the `T` component of `entity`
    #[inline]
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Result<Mut<'_, T>, ComponentError> {
        todo!()
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
        let loc = self.entities.get(entity)?;
        if loc.archetype == 0 {
            return Err(MissingComponent::new::<T>().into());
        }
        Ok(Mut::new(
            self.archetypes.get(ArchetypeId(loc.archetype)).unwrap(),
            loc.index,
        )?)
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
    pub unsafe fn query_unchecked<Q: WorldQuery, F: QueryFilter>(&self) -> QueryIter<'_, Q, F> {
        QueryIter::new(&self.archetypes)
    }

        /// Prepare a query against a single entity, without checking the safety of mutable queries
    ///
    /// Handy for accessing multiple components simultaneously.
    ///
    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn query_one_unchecked<Q: WorldQuery, F: QueryFilter>(
        &self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, NoSuchEntity> {
        todo!()
    }

    pub fn clear_trackers(&mut self) {
        for archetype in self.archetypes.iter_mut() {
            archetype.clear_trackers();
        }

        // self.removed_components.clear();
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
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ComponentError {
    /// The entity was already despawned
    NoSuchEntity,
    /// The entity did not have a requested component
    MissingComponent(MissingComponent),
}

#[cfg(feature = "std")]
impl Error for ComponentError {}

impl fmt::Display for ComponentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ComponentError::*;
        match *self {
            NoSuchEntity => f.write_str("no such entity"),
            MissingComponent(ref x) => x.fmt(f),
        }
    }
}

impl From<NoSuchEntity> for ComponentError {
    fn from(NoSuchEntity: NoSuchEntity) -> Self {
        ComponentError::NoSuchEntity
    }
}

impl From<MissingComponent> for ComponentError {
    fn from(x: MissingComponent) -> Self {
        ComponentError::MissingComponent(x)
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
    archetype_id: u32,
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
                    .put_dynamic(ptr, ty, size, index, ComponentFlags::ADDED);
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


#[cfg(test)]
mod tests {
    use crate::World;

    struct A {
        x: u8,
        y: u32,
    }
    struct B(usize);

    #[test]
    fn test() {
        let mut world = World::default();
        world.spawn((A { x: 1, y: 2 }, B(3)));
    }
}
