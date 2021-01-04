use std::{any::TypeId, fmt};

use bevy_utils::HashMap;

use crate::{
    Archetype, ArchetypeId, Archetypes, BatchedIter, Bundle, Component, ComponentFlags,
    DynamicBundle, Entity, EntityFilter, Fetch, Location, MissingComponent, Mut, NoSuchEntity,
    QueryFilter, QueryIter, ReadOnlyFetch, SparseSets, StorageType, TypeInfo, WorldQuery,
};

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
    #[inline]
    fn get_dynamic<'a, T: DynamicBundle>(
        &'a mut self,
        bundle: &T,
        components: &mut Components,
        archetypes: &mut Archetypes,
    ) -> &'a BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let type_info = bundle.type_info();
                Self::get_bundle_info(type_info, components, archetypes)
            })
    }

    #[inline]
    fn get_static<'a, T: Bundle>(
        &'a mut self,
        components: &mut Components,
        archetypes: &mut Archetypes,
    ) -> &'a BundleInfo {
        self.bundle_info
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                let type_info = T::static_type_info();
                Self::get_bundle_info(type_info, components, archetypes)
            })
    }

    #[inline]
    fn get_bundle_info(
        mut type_info: Vec<TypeInfo>,
        components: &mut Components,
        archetypes: &mut Archetypes,
    ) -> BundleInfo {
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

    pub fn spawn(&mut self, bundle: impl DynamicBundle) -> Entity {
        self.flush();
        let bundle_info =
            self.bundle_infos
                .get_dynamic(&bundle, &mut self.components, &mut self.archetypes);
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

        let bundle_info = self
            .bundle_infos
            .get_static::<I::Item>(&mut self.components, &mut self.archetypes);

        let archetype = self.archetypes.get_mut(bundle_info.archetype).unwrap();
        let length = upper.unwrap_or(lower);
        archetype.reserve(length);
        self.entities.reserve(length as u32);
        SpawnBatchIter {
            inner: iter,
            entities: &mut self.entities,
            archetype_id: bundle_info.archetype.0,
            archetype,
        }
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
    pub fn query<Q: WorldQuery>(&self) -> QueryIter<'_, Q, ()>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_unchecked() }
    }

    #[inline]
    pub fn query_filtered<Q: WorldQuery, F: QueryFilter>(&self) -> QueryIter<'_, Q, F>
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
    pub fn query_mut<Q: WorldQuery>(&mut self) -> QueryIter<'_, Q, ()> {
        // SAFE: unique mutable access
        unsafe { self.query_unchecked() }
    }

    #[inline]
    pub fn query_filtered_mut<Q: WorldQuery, F: QueryFilter>(&mut self) -> QueryIter<'_, Q, F> {
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
    pub unsafe fn query_unchecked<Q: WorldQuery, F: QueryFilter>(&self) -> QueryIter<'_, Q, F> {
        QueryIter::new(&self.archetypes)
    }

    /// Prepare a read only query against a single entity
    ///
    /// Handy for accessing multiple components simultaneously.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let a = world.spawn((123, true, "abc"));
    /// // The returned query must outlive the borrow made by `get`
    /// let (number, flag) = world.query_one::<(&i32, &bool)>(a).unwrap();
    /// assert_eq!(*number, 123);
    /// ```
    #[inline]
    pub fn query_one<Q: WorldQuery>(
        &self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, NoSuchEntity>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_one_unchecked::<Q, ()>(entity) }
    }

    #[inline]
    pub fn query_one_filtered<Q: WorldQuery, F: QueryFilter>(
        &self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, NoSuchEntity>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_one_unchecked::<Q, F>(entity) }
    }

    /// Prepare a query against a single entity
    ///
    /// Handy for accessing multiple components simultaneously.
    ///
    /// # Example
    /// ```
    /// # use bevy_ecs::*;
    /// let mut world = World::new();
    /// let a = world.spawn((123, true, "abc"));
    /// // The returned query must outlive the borrow made by `get`
    /// let (mut number, flag) = world.query_one_mut::<(&mut i32, &bool)>(a).unwrap();
    /// if *flag { *number *= 2; }
    /// assert_eq!(*number, 246);
    /// ```
    #[inline]
    pub fn query_one_mut<Q: WorldQuery>(
        &mut self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, NoSuchEntity> {
        // SAFE: unique mutable access to world
        unsafe { self.query_one_unchecked::<Q, ()>(entity) }
    }

    #[inline]
    pub fn query_one_filtered_mut<Q: WorldQuery, F: QueryFilter>(
        &mut self,
        entity: Entity,
    ) -> Result<<Q::Fetch as Fetch>::Item, NoSuchEntity> {
        // SAFE: unique mutable access to world
        unsafe { self.query_one_unchecked::<Q, F>(entity) }
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
        let loc = self.entities.get(entity)?;
        let archetype = self.archetypes.get(ArchetypeId(loc.archetype)).unwrap();
        let matches_filter = F::get_entity_filter(archetype)
            .map(|entity_filter| entity_filter.matches_entity(loc.index))
            .unwrap_or(false);
        if matches_filter {
            <Q::Fetch as Fetch>::get(archetype, 0)
                .map(|fetch| fetch.fetch(loc.index))
                .ok_or(NoSuchEntity)
        } else {
            Err(NoSuchEntity)
        }
    }

    /// Like `query`, but instead of returning a single iterator it returns a "batched iterator",
    /// where each batch is `batch_size`. This is generally used for parallel iteration.
    ///
    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn query_batched_unchecked<Q: WorldQuery, F: QueryFilter>(
        &self,
        batch_size: usize,
    ) -> BatchedIter<'_, Q, F> {
        BatchedIter::new(&self.archetypes, batch_size)
    }

    /// Borrow the `T` component at the given location, without safety checks
    /// # Safety
    /// This does not check that the location is within bounds of the archetype.
    pub unsafe fn get_at_location_unchecked<T: Component>(
        &self,
        location: Location,
    ) -> Result<&T, ComponentError> {
        if location.archetype == 0 {
            return Err(MissingComponent::new::<T>().into());
        }
        let archetype = self
            .archetypes
            .get(ArchetypeId(location.archetype))
            .unwrap();
        Ok(&*archetype
            .get::<T>()
            .ok_or_else(MissingComponent::new::<T>)?
            .as_ptr()
            .add(location.index as usize))
    }

    /// Borrow the `T` component at the given location, without safety checks
    /// # Safety
    /// This does not check that the location is within bounds of the archetype.
    /// It also does not check for mutable access correctness. To be safe, make sure this is the only
    /// thing accessing this entity's T component.
    pub unsafe fn get_mut_at_location_unchecked<T: Component>(
        &self,
        location: Location,
    ) -> Result<Mut<T>, ComponentError> {
        if location.archetype == 0 {
            return Err(MissingComponent::new::<T>().into());
        }
        let archetype = self
            .archetypes
            .get(ArchetypeId(location.archetype))
            .unwrap();
        Ok(Mut::new(archetype, location.index)?)
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
