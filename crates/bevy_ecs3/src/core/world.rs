use crate::core::{
    add_bundle_to_archetype, entity_ref::EntityMut, Archetype, ArchetypeId, Archetypes, Bundle,
    Bundles, Component, Components, Entities, Entity, EntityRef, Mut, QueryIter, ReadOnlyFetch,
    SparseSets, Storages, Table, WorldQuery,
};
use std::fmt;

use super::BundleInfo;

#[derive(Default)]
pub struct World {
    pub(crate) entities: Entities,
    pub(crate) components: Components,
    pub(crate) archetypes: Archetypes,
    pub(crate) storages: Storages,
    pub(crate) bundles: Bundles,
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

    #[inline]
    pub fn query<Q: WorldQuery>(&self) -> QueryIter<'_, Q, ()>
    where
        Q::Fetch: ReadOnlyFetch,
    {
        // SAFE: read-only access to world and read only query prevents mutable access
        unsafe { self.query_unchecked() }
    }

    #[inline]
    pub fn query_mut<Q: WorldQuery>(&mut self) -> QueryIter<'_, Q, ()> {
        // SAFE: unique mutable access
        unsafe { self.query_unchecked() }
    }

    /// # Safety
    /// This does not check for mutable query correctness. To be safe, make sure mutable queries
    /// have unique access to the components they query.
    #[inline]
    pub unsafe fn query_unchecked<Q: WorldQuery>(&self) -> QueryIter<'_, Q, ()> {
        QueryIter::new(&self)
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
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "World")
    }
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

pub struct SpawnBatchIter<'w, I>
where
    I: Iterator,
    I::Item: Bundle,
{
    inner: I,
    entities: &'w mut Entities,
    archetype: &'w mut Archetype,
    table: &'w mut Table,
    sparse_sets: &'w mut SparseSets,
    bundle_info: &'w BundleInfo,
}

impl<'w, I> SpawnBatchIter<'w, I>
where
    I: Iterator,
    I::Item: Bundle,
{
    #[inline]
    fn new(world: &'w mut World, iter: I) -> Self {
        // Ensure all entity allocations are accounted for so `self.entities` can realloc if
        // necessary
        world.flush();

        let iter = iter.into_iter();
        let (lower, upper) = iter.size_hint();

        let bundle_info = world.bundles.init_info::<I::Item>(&mut world.components);

        let length = upper.unwrap_or(lower);
        // SAFE: empty archetype exists and bundle components were initialized above
        let archetype_id = unsafe {
            add_bundle_to_archetype(
                &mut world.archetypes,
                &mut world.storages,
                &mut world.components,
                ArchetypeId::empty_archetype(),
                bundle_info,
            )
        };
        // SAFE: archetype exists
        let archetype = unsafe { world.archetypes.get_unchecked_mut(archetype_id) };
        // SAFE: table exists
        let table = unsafe {
            world
                .storages
                .tables
                .get_unchecked_mut(archetype.table_id())
        };
        archetype.reserve(length);
        table.reserve(length);
        world.entities.reserve(length as u32);
        Self {
            inner: iter,
            entities: &mut world.entities,
            archetype,
            table,
            sparse_sets: &mut world.storages.sparse_sets,
            bundle_info,
        }
    }
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
        let bundle = self.inner.next()?;
        let entity = self.entities.alloc();
        unsafe {
            let table_row = self.table.allocate(entity);
            let location = self.archetype.allocate(entity, table_row);
            self.bundle_info.put_components(
                self.sparse_sets,
                entity,
                self.table,
                table_row,
                bundle,
            );
            self.entities.meta[entity.id as usize].location = location;
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
