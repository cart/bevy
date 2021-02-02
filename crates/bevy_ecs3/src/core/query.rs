use fixedbitset::FixedBitSet;

use crate::{
    core::{
        Archetype, ArchetypeGeneration, ArchetypeId, Archetypes, Component, ComponentFlags,
        ComponentId, ComponentSparseSet, Entity, Mut, QueryFilter, StorageType, Table, TableId,
        Tables, World,
    },
    smaller_tuples_too,
};
use std::{
    any::TypeId,
    num::Wrapping,
    ops::Range,
    ptr::{self, NonNull},
};

pub trait WorldQuery {
    type Fetch: for<'a> Fetch<'a>;
}

pub trait Fetch<'w>: Sized {
    const DANGLING: Self;
    type Item;
    unsafe fn init(world: &World) -> Option<Self>;
    fn matches_archetype(&self, archetype: &Archetype) -> bool;
    fn matches_table(&self, table: &Table) -> bool;
    fn is_dense(&self) -> bool;
    unsafe fn next_table(&mut self, table: &Table);
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item>;
    unsafe fn fetch(&mut self, index: usize) -> Self::Item;
}

/// A fetch that is read only. This should only be implemented for read-only fetches.
pub unsafe trait ReadOnlyFetch {}

pub struct QueryState {
    archetype_generation: ArchetypeGeneration,
    matched_tables: FixedBitSet,
    // NOTE: we maintain both a TableId bitset and a vec because iterating the vec is faster
    matched_table_ids: Vec<TableId>,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_tables: FixedBitSet::default(),
            matched_table_ids: Vec::new(),
        }
    }
}

impl QueryState {
    #[inline]
    fn update_archetypes(&mut self, archetypes: &Archetypes) -> Range<usize> {
        let old_generation = self.archetype_generation;
        self.archetype_generation = archetypes.generation();
        if old_generation == self.archetype_generation {
            0..0
        } else {
            if old_generation.value() == usize::MAX {
                0..archetypes.len()
            } else {
                old_generation.value()..archetypes.len()
            }
        }
    }
}

/// Iterates the entities that match a given query.
/// This iterator is less efficient than StatefulQueryIter. It must scan each archetype to check for a match.
/// It won't necessarily do a linear scan of Tables, which can affect its cache-friendly.
pub struct QueryIter<'w, Q: WorldQuery, F: QueryFilter> {
    world: &'w World,
    archetypes: &'w Archetypes,
    tables: &'w Tables,
    current_archetype: Wrapping<u32>,
    fetch: Q::Fetch,
    filter: F,
    archetype_len: usize,
    archetype_index: usize,
}

impl<'w, Q: WorldQuery, F: QueryFilter> QueryIter<'w, Q, F> {
    /// SAFETY: ensure that the given `query_state` is only used with this exact [QueryIter] type   
    pub unsafe fn with_state<'s>(
        self,
        query_state: &'s mut QueryState,
    ) -> StatefulQueryIter<'w, 's, Q, F> {
        StatefulQueryIter::new_internal(
            self.archetypes,
            self.tables,
            self.fetch,
            self.filter,
            query_state,
        )
    }

    pub fn get(mut self, entity: Entity) -> Option<<Q::Fetch as Fetch<'w>>::Item> {
        // SAFE: Queries can only be created in ways that honor rust's mutability rules. This consumes the query, which prevents aliased access.
        unsafe {
            let location = self.world.entities.get(entity)?;
            // SAFE: live entities always exist in an archetype
            let archetype = self.archetypes.get_unchecked(location.archetype_id);
            if !self.fetch.matches_archetype(archetype) || !self.filter.matches_archetype(archetype)
            {
                return None;
            }

            let table = self.tables.get_unchecked(archetype.table_id());
            self.fetch.next_table(table);
            self.filter.next_table(table);
            let table_row = archetype.entity_table_row_unchecked(location.index);
            if self.filter.matches_entity(table_row) {
                Some(self.fetch.fetch(table_row))
            } else {
                None
            }
        }
    }
}

impl<'w, Q: WorldQuery> QueryIter<'w, Q, ()> {
    pub unsafe fn new(world: &'w World) -> Self {
        let (fetch, current_archetype) = if let Some(fetch) = <Q::Fetch as Fetch>::init(world) {
            // Start at "max" u32, so when we add 1 it will wrap around to 0
            (fetch, Wrapping(u32::MAX))
        } else {
            // could not fetch. this iterator will return None
            (
                <Q::Fetch as Fetch>::DANGLING,
                Wrapping(world.archetypes().len() as u32),
            )
        };
        QueryIter {
            world,
            fetch,
            current_archetype,
            archetypes: &world.archetypes,
            tables: &world.storages.tables,
            filter: <() as QueryFilter>::DANGLING,
            archetype_len: 0,
            archetype_index: 0,
        }
    }

    pub fn filter<F: QueryFilter>(mut self) -> QueryIter<'w, Q, F> {
        let filter = if let Some(filter) = unsafe { F::init(self.world) } {
            filter
        } else {
            self.archetype_index = self.archetype_len;
            self.current_archetype = Wrapping(self.world.archetypes().len() as u32);
            F::DANGLING
        };
        QueryIter {
            world: self.world,
            fetch: self.fetch,
            current_archetype: self.current_archetype,
            archetypes: self.archetypes,
            tables: self.tables,
            filter,
            archetype_len: self.archetype_len,
            archetype_index: self.archetype_index,
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> Iterator for QueryIter<'w, Q, F> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            loop {
                if self.archetype_index == self.archetype_len {
                    let next_index = self.current_archetype + Wrapping(1);
                    if next_index.0 as usize >= self.archetypes.len() {
                        return None;
                    }
                    self.current_archetype = next_index;
                    let archetype = self
                        .archetypes
                        .get_unchecked(ArchetypeId::new(self.current_archetype.0));
                    if !self.fetch.matches_archetype(archetype)
                        || !self.filter.matches_archetype(archetype)
                    {
                        continue;
                    }
                    let table = self.tables.get_unchecked(archetype.table_id());
                    self.fetch.next_table(table);
                    self.filter.next_table(table);
                    self.archetype_len = archetype.len();
                    self.archetype_index = 0;
                    continue;
                }

                let archetype = self
                    .archetypes
                    .get_unchecked(ArchetypeId::new(self.current_archetype.0));
                let table_row = archetype.entity_table_row_unchecked(self.archetype_index);
                if !self.filter.matches_entity(table_row) {
                    self.archetype_index += 1;
                    continue;
                }

                let item = self.fetch.fetch(table_row);
                self.archetype_index += 1;
                return Some(item);
            }
        }
    }
}

pub struct StatefulQueryIter<'w, 's, Q: WorldQuery, F: QueryFilter> {
    tables: &'w Tables,
    // TODO: try removing this for bitset iterator
    table_id_iter: std::slice::Iter<'s, TableId>,
    fetch: Q::Fetch,
    filter: F,
    table_len: usize,
    table_index: usize,
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> StatefulQueryIter<'w, 's, Q, F> {
    pub unsafe fn new(world: &'w World, query_state: &'s mut QueryState) -> Self {
        let fetch = <Q::Fetch as Fetch>::init(world);
        let filter = F::init(world);
        let (fetch, filter) = if let (Some(fetch), Some(filter)) = (fetch, filter) {
            (fetch, filter)
        } else {
            query_state.matched_tables.clear();
            query_state.matched_table_ids.clear();
            // could not fetch. this iterator will return None
            (<Q::Fetch as Fetch>::DANGLING, F::DANGLING)
        };
        StatefulQueryIter::new_internal(
            &world.archetypes,
            &world.storages.tables,
            fetch,
            filter,
            query_state,
        )
    }

    unsafe fn new_internal(
        archetypes: &'w Archetypes,
        tables: &'w Tables,
        fetch: Q::Fetch,
        filter: F,
        query_state: &'s mut QueryState,
    ) -> Self {
        query_state.matched_tables.grow(tables.len());
        let archetype_indices = query_state.update_archetypes(archetypes);
        for archetype_index in archetype_indices {
            let archetype = archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32));
            let table_index = archetype.table_id().index();
            // SAFE: ArchetypeGeneration is used to generate the range, and is by definition valid
            if !query_state.matched_tables.contains(table_index)
                && fetch.matches_archetype(archetype)
                && filter.matches_archetype(archetype)
            {
                query_state.matched_tables.set(table_index, true);
                query_state.matched_table_ids.push(archetype.table_id());
            }
        }
        StatefulQueryIter {
            fetch,
            tables,
            filter,
            table_id_iter: query_state.matched_table_ids.iter(),
            table_len: 0,
            table_index: 0,
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> Iterator for StatefulQueryIter<'w, 's, Q, F> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            if self.fetch.is_dense() {
                loop {
                    if self.table_index == self.table_len {
                        let table_id = self.table_id_iter.next()?;
                        let table = self.tables.get_unchecked(*table_id);
                        self.fetch.next_table(table);
                        self.filter.next_table(table);
                        self.table_len = table.len();
                        self.table_index = 0;
                        continue;
                    }

                    if !self.filter.matches_entity(self.table_index) {
                        self.table_index += 1;
                        continue;
                    }

                    let item = self.fetch.fetch(self.table_index);

                    self.table_index += 1;
                    return Some(item);
                }
            } else {
                loop {
                    if self.table_index == self.table_len {
                        let table_id = self.table_id_iter.next()?;
                        let table = self.tables.get_unchecked(*table_id);
                        self.fetch.next_table(table);
                        self.filter.next_table(table);
                        self.table_len = table.len();
                        self.table_index = 0;
                        continue;
                    }

                    if !self.filter.matches_entity(self.table_index) {
                        self.table_index += 1;
                        continue;
                    }

                    let item = self.fetch.try_fetch(self.table_index);
                    self.table_index += 1;
                    if item.is_none() {
                        continue;
                    }
                    return item;
                }
            }
        }
    }
}

impl WorldQuery for Entity {
    type Fetch = FetchEntity;
}

pub struct FetchEntity {
    entities: *const Entity,
}

unsafe impl ReadOnlyFetch for FetchEntity {}

impl<'w> Fetch<'w> for FetchEntity {
    type Item = Entity;

    const DANGLING: Self = FetchEntity {
        entities: std::ptr::null::<Entity>(),
    };

    fn matches_archetype(&self, _archetype: &Archetype) -> bool {
        true
    }

    fn matches_table(&self, _table: &Table) -> bool {
        true
    }

    #[inline]
    fn is_dense(&self) -> bool {
        true
    }

    unsafe fn init(_world: &World) -> Option<Self> {
        Some(Self::DANGLING)
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        self.entities = table.entities().as_ptr();
    }

    #[inline]
    unsafe fn fetch(&mut self, index: usize) -> Self::Item {
        *self.entities.add(index)
    }

    #[inline]
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
        Some(self.fetch(index))
    }
}

impl<T: Component> WorldQuery for &T {
    type Fetch = FetchRead<T>;
}

pub enum FetchRead<T> {
    Table {
        component_id: ComponentId,
        components: NonNull<T>,
        tables: *const Tables,
    },
    SparseSet {
        component_id: ComponentId,
        entities: *const Entity,
        sparse_set: *const ComponentSparseSet,
    },
}

unsafe impl<T> ReadOnlyFetch for FetchRead<T> {}

impl<'w, T: Component> Fetch<'w> for FetchRead<T> {
    type Item = &'w T;

    const DANGLING: Self = Self::Table {
        component_id: ComponentId::new(usize::MAX),
        components: NonNull::dangling(),
        tables: ptr::null::<Tables>(),
    };

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        match self {
            Self::Table { component_id, .. } => archetype.contains(*component_id),
            Self::SparseSet { component_id, .. } => archetype.contains(*component_id),
        }
    }

    fn matches_table(&self, table: &Table) -> bool {
        match self {
            Self::Table { component_id, .. } => table.has_column(*component_id),
            // any table could have any sparse set component
            Self::SparseSet { .. } => true,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        match self {
            Self::Table { .. } => true,
            Self::SparseSet { .. } => false,
        }
    }

    unsafe fn init(world: &World) -> Option<Self> {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>())?;
        let component_info = components.get_info_unchecked(component_id);
        Some(match component_info.storage_type() {
            StorageType::Table => Self::Table {
                component_id,
                components: NonNull::dangling(),
                tables: (&world.storages().tables) as *const Tables,
            },
            StorageType::SparseSet => Self::SparseSet {
                component_id,
                entities: std::ptr::null::<Entity>(),
                sparse_set: world.storages().sparse_sets.get_unchecked(component_id),
            },
        })
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        match self {
            Self::Table {
                component_id,
                components,
                ..
            } => {
                *components = table
                    .get_column_unchecked(*component_id)
                    .get_ptr()
                    .cast::<T>();
            }
            Self::SparseSet { entities, .. } => *entities = table.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
        match self {
            Self::Table { components, .. } => Some(&*components.as_ptr().add(index)),
            Self::SparseSet {
                entities,
                sparse_set,
                ..
            } => {
                let entity = *entities.add(index);
                (**sparse_set).get(entity).map(|c| &*c.cast::<T>())
            }
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, index: usize) -> Self::Item {
        match self {
            Self::Table { components, .. } => &*components.as_ptr().add(index),
            Self::SparseSet {
                entities,
                sparse_set,
                ..
            } => {
                let entity = *entities.add(index);
                &*(**sparse_set).get_unchecked(entity).cast::<T>()
            }
        }
    }
}

impl<T: Component> WorldQuery for &mut T {
    type Fetch = FetchWrite<T>;
}

pub enum FetchWrite<T> {
    Table {
        component_id: ComponentId,
        components: NonNull<T>,
        flags: *mut ComponentFlags,
        tables: *const Tables,
    },
    SparseSet {
        component_id: ComponentId,
        entities: *const Entity,
        sparse_set: *const ComponentSparseSet,
    },
}

unsafe impl<T> ReadOnlyFetch for FetchWrite<T> {}

impl<'w, T: Component> Fetch<'w> for FetchWrite<T> {
    type Item = Mut<'w, T>;

    const DANGLING: Self = Self::Table {
        component_id: ComponentId::new(usize::MAX),
        components: NonNull::dangling(),
        flags: ptr::null_mut::<ComponentFlags>(),
        tables: ptr::null::<Tables>(),
    };

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        match self {
            Self::Table { component_id, .. } => archetype.contains(*component_id),
            Self::SparseSet { component_id, .. } => archetype.contains(*component_id),
        }
    }

    fn matches_table(&self, table: &Table) -> bool {
        match self {
            Self::Table { component_id, .. } => table.has_column(*component_id),
            // any table could have any sparse set component
            Self::SparseSet { .. } => true,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        match self {
            Self::Table { .. } => true,
            Self::SparseSet { .. } => false,
        }
    }

    unsafe fn init(world: &World) -> Option<Self> {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>())?;
        let component_info = components.get_info_unchecked(component_id);
        Some(match component_info.storage_type() {
            StorageType::Table => Self::Table {
                component_id,
                components: NonNull::dangling(),
                flags: ptr::null_mut(),
                tables: (&world.storages().tables) as *const Tables,
            },
            StorageType::SparseSet => Self::SparseSet {
                component_id,
                entities: std::ptr::null::<Entity>(),
                sparse_set: world.storages().sparse_sets.get_unchecked(component_id),
            },
        })
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        match self {
            Self::Table {
                component_id,
                components,
                flags,
                ..
            } => {
                let column = table.get_column_unchecked(*component_id);
                *components = column.get_ptr().cast::<T>();
                *flags = column.get_flags_mut_ptr();
            }
            Self::SparseSet { entities, .. } => *entities = table.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
        match self {
            Self::Table {
                components, flags, ..
            } => Some(Mut {
                value: &mut *components.as_ptr().add(index),
                flags: &mut *flags.add(index),
            }),
            Self::SparseSet {
                entities,
                sparse_set,
                ..
            } => {
                let entity = *entities.add(index);
                (**sparse_set).get_with_flags(entity).map(|(c, f)| Mut {
                    value: &mut *c.cast::<T>(),
                    flags: &mut *f,
                })
            }
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, index: usize) -> Self::Item {
        match self {
            Self::Table {
                components, flags, ..
            } => Mut {
                value: &mut *components.as_ptr().add(index),
                flags: &mut *flags.add(index),
            },
            Self::SparseSet {
                entities,
                sparse_set,
                ..
            } => {
                let entity = *entities.add(index);
                let (value, flags) = (**sparse_set).get_with_flags_unchecked(entity);
                Mut {
                    value: &mut *value.cast::<T>(),
                    flags: &mut *flags,
                }
            }
        }
    }
}

impl<T: WorldQuery> WorldQuery for Option<T> {
    type Fetch = FetchOption<T::Fetch>;
}

pub struct FetchOption<T> {
    fetch: T,
    matches: bool,
}

unsafe impl<T: ReadOnlyFetch> ReadOnlyFetch for FetchOption<T> {}

impl<'w, T: Fetch<'w>> Fetch<'w> for FetchOption<T> {
    type Item = Option<T::Item>;

    const DANGLING: Self = Self {
        fetch: T::DANGLING,
        matches: false,
    };

    fn matches_archetype(&self, _archetype: &Archetype) -> bool {
        true
    }

    fn matches_table(&self, _table: &Table) -> bool {
        true
    }

    #[inline]
    fn is_dense(&self) -> bool {
        // option queries must always use try_fetch
        false
    }

    unsafe fn init(world: &World) -> Option<Self> {
        Some(Self {
            fetch: T::init(world)?,
            matches: false,
        })
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        self.matches = self.fetch.matches_table(table);
        if self.matches {
            self.fetch.next_table(table);
        }
    }

    #[inline]
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
        if self.matches {
            Some(self.fetch.try_fetch(index))
        } else {
            Some(None)
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, index: usize) -> Self::Item {
        if self.matches {
            Some(self.fetch.try_fetch(index)?)
        } else {
            None
        }
    }
}

macro_rules! tuple_impl {
    ($($name: ident),*) => {
        impl<'a, $($name: Fetch<'a>),*> Fetch<'a> for ($($name,)*) {
            type Item = ($($name::Item,)*);

            const DANGLING: Self = ($($name::DANGLING,)*);

            #[allow(unused_variables)]
            unsafe fn init(world: &World) -> Option<Self> {
                Some(($($name::init(world)?,)*))
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            #[inline]
            fn is_dense(&self) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.is_dense())*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn matches_archetype(&self, archetype: &Archetype) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_archetype(archetype))*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn matches_table(&self, table: &Table) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_table(table))*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            #[inline]
            unsafe fn next_table(&mut self, table: &Table) {
                let ($($name,)*) = self;
                $($name.next_table(table);)*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            #[inline]
            unsafe fn fetch(&mut self, index: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.fetch(index),)*)
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            #[inline]
            unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
                let ($($name,)*) = self;
                Some(($($name.try_fetch(index)?,)*))
            }
        }

        impl<$($name: WorldQuery),*> WorldQuery for ($($name,)*) {
            type Fetch = ($($name::Fetch,)*);
        }

        unsafe impl<$($name: ReadOnlyFetch),*> ReadOnlyFetch for ($($name,)*) {}

    };
}

smaller_tuples_too!(tuple_impl, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);

#[cfg(test)]
mod tests {
    use crate::core::{ComponentDescriptor, QueryState, StorageType, World};

    #[derive(Debug, Eq, PartialEq)]
    struct A(usize);
    #[derive(Debug, Eq, PartialEq)]
    struct B(usize);

    #[test]
    fn query() {
        let mut world = World::new();
        world.spawn().insert_bundle((A(1), B(1)));
        world.spawn().insert_bundle((A(2),));
        let values = world.query::<&A>().collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (_a, mut b) in world.query_mut::<(&A, &mut B)>() {
            b.0 = 3;
        }
        let values = world.query::<&B>().collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }

    #[test]
    fn stateful_query() {
        let mut world = World::new();
        let mut query_state = QueryState::default();
        world.spawn().insert_bundle((A(1), B(1)));
        world.spawn().insert_bundle((A(2),));
        unsafe {
            let values = world
                .query::<&A>()
                .with_state(&mut query_state)
                .collect::<Vec<&A>>();
            assert_eq!(values, vec![&A(1), &A(2)]);
        }

        unsafe {
            let mut query_state = QueryState::default();
            for (_a, mut b) in world.query::<(&A, &mut B)>().with_state(&mut query_state) {
                b.0 = 3;
            }
        }

        unsafe {
            let mut query_state = QueryState::default();
            let values = world
                .query::<&B>()
                .with_state(&mut query_state)
                .collect::<Vec<&B>>();
            assert_eq!(values, vec![&B(3)]);
        }
    }

    #[test]
    fn multi_storage_query() {
        let mut world = World::new();
        world
            .register_component(ComponentDescriptor::of::<A>(StorageType::SparseSet))
            .unwrap();

        world.spawn().insert_bundle((A(1), B(2)));
        world.spawn().insert_bundle((A(2),));

        let values = world.query::<&A>().collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (_a, mut b) in world.query::<(&A, &mut B)>() {
            b.0 = 3;
        }

        let values = world.query::<&B>().collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }
}
