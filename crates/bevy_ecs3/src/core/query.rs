use crate::{
    core::{
        Archetype, ArchetypeGeneration, ArchetypeId, Archetypes, Component, ComponentId,
        ComponentSparseSet, Entity, EntityFilter, FilterLock, Mut, QueryFilter, StorageType,
        Storages, Tables, World,
    },
    smaller_tuples_too,
};
use fixedbitset::{FixedBitSet, Ones};
use std::{any::TypeId, marker::PhantomData, ops::Range, ptr::NonNull};

pub trait WorldQuery {
    type Fetch: for<'a> Fetch<'a>;
}

// TODO: this should iterate tables instead of archetypes
pub trait Fetch<'w>: Sized {
    const DANGLING: Self;
    type Item;
    unsafe fn init(world: &World) -> Option<Self>;
    fn matches_archetype(&self, archetype: &Archetype) -> bool;
    unsafe fn next_archetype(&mut self, archetype: &Archetype);
    unsafe fn fetch(&mut self, archetype_index: usize) -> Self::Item;
}

/// A fetch that is read only. This should only be implemented for read-only fetches.
pub unsafe trait ReadOnlyFetch {}

pub struct QueryState {
    archetype_generation: ArchetypeGeneration,
    matched_archetypes: FixedBitSet,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            archetype_generation: ArchetypeGeneration::new(usize::MAX),
            matched_archetypes: FixedBitSet::default(),
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
            self.matched_archetypes.grow(archetypes.len());
            if old_generation.value() == usize::MAX {
                0..archetypes.len()
            } else {
                old_generation.value()..archetypes.len()
            }
        }
    }
}

pub struct QueryIter<'w, Q: WorldQuery, F: QueryFilter, FilterLock> {
    archetypes: &'w Archetypes,
    current_archetype: ArchetypeId,
    fetch: Q::Fetch,
    filter: F::EntityFilter,
    archetype_len: usize,
    archetype_index: usize,
    marker: PhantomData<FilterLock>,
}

impl<'w, Q: WorldQuery, F: QueryFilter, FilterLock> QueryIter<'w, Q, F, FilterLock> {
    pub unsafe fn new(world: &'w World) -> Self {
        let (fetch, current_archetype) = if let Some(fetch) = <Q::Fetch as Fetch>::init(world) {
            (fetch, ArchetypeId::new(0))
        } else {
            // could not fetch. this iterator will return None
            (
                <Q::Fetch as Fetch>::DANGLING,
                ArchetypeId::new(world.archetypes().len() as u32),
            )
        };
        QueryIter {
            fetch,
            current_archetype,
            archetypes: world.archetypes(),
            filter: <F::EntityFilter as EntityFilter>::DANGLING,
            archetype_len: 0,
            archetype_index: 0,
            marker: Default::default(),
        }
    }

    /// SAFETY: ensure that the given `query_state` is only used with this exact [QueryIter] type   
    pub unsafe fn with_state<'s>(
        self,
        query_state: &'s mut QueryState,
    ) -> StatefulQueryIter<'w, 's, Q, F> {
        let fetch = self.fetch;
        let archetypes = self.archetypes;
        let archetype_indices = query_state.update_archetypes(archetypes);
        for archetype_index in archetype_indices {
            // SAFE: ArchetypeGeneration is used to generate the range, and is by definition valid
            if fetch.matches_archetype(
                archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32)),
            ) {
                query_state.matched_archetypes.set(archetype_index, true);
            }
        }

        StatefulQueryIter {
            fetch,
            archetypes,
            filter: <F::EntityFilter as EntityFilter>::DANGLING,
            archetype_id_iter: query_state.matched_archetypes.ones(),
            archetype_len: 0,
            archetype_index: 0,
        }
    }
}

impl<'w, Q: WorldQuery> QueryIter<'w, Q, (), ()> {
    pub fn filter<F: QueryFilter>(self) -> QueryIter<'w, Q, F, FilterLock> {
        QueryIter {
            fetch: self.fetch,
            current_archetype: self.current_archetype,
            archetypes: self.archetypes,
            filter: <F::EntityFilter as EntityFilter>::DANGLING,
            archetype_len: self.archetype_len,
            archetype_index: self.archetype_index,
            marker: Default::default(),
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter, FilterLock> Iterator
    for QueryIter<'w, Q, F, FilterLock>
{
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            loop {
                if self.archetype_index == self.archetype_len {
                    if self.current_archetype.index() as usize == self.archetypes.len() {
                        return None;
                    }
                    let archetype = self.archetypes.get_unchecked(self.current_archetype);
                    self.current_archetype = ArchetypeId::new(self.current_archetype.index() + 1);
                    if !self.fetch.matches_archetype(archetype) {
                        continue;
                    }
                    self.fetch.next_archetype(archetype);
                    self.archetype_len = archetype.len();
                    self.archetype_index = 0;
                    continue;
                }

                let item = self.fetch.fetch(self.archetype_index);
                self.archetype_index += 1;
                return Some(item);
            }
        }
    }
}

pub struct StatefulQueryIter<'w, 's, Q: WorldQuery, F: QueryFilter> {
    archetypes: &'w Archetypes,
    archetype_id_iter: Ones<'s>,
    fetch: Q::Fetch,
    filter: F::EntityFilter,
    archetype_len: usize,
    archetype_index: usize,
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> StatefulQueryIter<'w, 's, Q, F> {
    pub unsafe fn new(world: &'w World, query_state: &'s mut QueryState) -> Self {
        let archetypes = world.archetypes();
        let fetch = if let Some(fetch) = <Q::Fetch as Fetch>::init(world) {
            let archetype_indices = query_state.update_archetypes(archetypes);
            for archetype_index in archetype_indices {
                // SAFE: ArchetypeGeneration is used to generate the range, and is by definition valid
                if fetch.matches_archetype(
                    archetypes.get_unchecked(ArchetypeId::new(archetype_index as u32)),
                ) {
                    query_state.matched_archetypes.set(archetype_index, true);
                }
            }
            fetch
        } else {
            query_state.matched_archetypes.clear();
            // could not fetch. this iterator will return None
            <Q::Fetch as Fetch>::DANGLING
        };
        StatefulQueryIter {
            fetch,
            archetypes,
            filter: <F::EntityFilter as EntityFilter>::DANGLING,
            archetype_id_iter: query_state.matched_archetypes.ones(),
            archetype_len: 0,
            archetype_index: 0,
        }
    }
}

impl<'w, 's, Q: WorldQuery, F: QueryFilter> Iterator for StatefulQueryIter<'w, 's, Q, F> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            loop {
                if self.archetype_index == self.archetype_len {
                    let archetype_id = ArchetypeId::new(self.archetype_id_iter.next()? as u32);
                    let archetype = self.archetypes.get_unchecked(archetype_id);
                    self.fetch.next_archetype(archetype);
                    self.archetype_len = archetype.len();
                    self.archetype_index = 0;
                    continue;
                }

                let item = self.fetch.fetch(self.archetype_index);
                self.archetype_index += 1;
                return Some(item);
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

    unsafe fn init(_world: &World) -> Option<Self> {
        Some(Self::DANGLING)
    }

    #[inline]
    unsafe fn next_archetype(&mut self, archetype: &Archetype) {
        self.entities = archetype.entities().as_ptr();
    }

    #[inline]
    unsafe fn fetch(&mut self, archetype_index: usize) -> Self::Item {
        *self.entities.add(archetype_index)
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
        sparse_set: *mut ComponentSparseSet,
    },
}

unsafe impl<T> ReadOnlyFetch for FetchRead<T> {}

impl<'w, T: Component> Fetch<'w> for FetchRead<T> {
    type Item = &'w T;

    const DANGLING: Self = Self::Table {
        component_id: ComponentId::new(usize::MAX),
        components: NonNull::dangling(),
        tables: std::ptr::null::<Tables>(),
    };

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        match self {
            Self::Table { component_id, .. } => archetype.contains(*component_id),
            Self::SparseSet { component_id, .. } => archetype.contains(*component_id),
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
                sparse_set: world
                    .storages()
                    .sparse_sets
                    .get_unchecked(component_id)
                    .unwrap(),
            },
        })
    }

    #[inline]
    unsafe fn next_archetype(&mut self, archetype: &Archetype) {
        match self {
            Self::Table {
                component_id,
                components,
                tables,
                ..
            } => {
                *components = (&**tables)
                    .get_unchecked(archetype.table_id())
                    .get_column_unchecked(*component_id)
                    .data()
                    .cast::<T>();
            }
            Self::SparseSet { entities, .. } => *entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, archetype_index: usize) -> Self::Item {
        match self {
            Self::Table { components, .. } => &*components.as_ptr().add(archetype_index),
            Self::SparseSet {
                entities,
                sparse_set,
                ..
            } => {
                let entity = *entities.add(archetype_index);
                &*(**sparse_set).get_component_unchecked(entity).cast::<T>()
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
        tables: *const Tables,
    },
    SparseSet {
        component_id: ComponentId,
        entities: *const Entity,
        sparse_set: *mut ComponentSparseSet,
    },
}

unsafe impl<T> ReadOnlyFetch for FetchWrite<T> {}

impl<'w, T: Component> Fetch<'w> for FetchWrite<T> {
    type Item = Mut<'w, T>;

    const DANGLING: Self = Self::Table {
        component_id: ComponentId::new(usize::MAX),
        components: NonNull::dangling(),
        tables: std::ptr::null::<Tables>(),
    };

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        match self {
            Self::Table { component_id, .. } => archetype.contains(*component_id),
            Self::SparseSet { component_id, .. } => archetype.contains(*component_id),
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
                sparse_set: world
                    .storages()
                    .sparse_sets
                    .get_unchecked(component_id)
                    .unwrap(),
            },
        })
    }

    #[inline]
    unsafe fn next_archetype(&mut self, archetype: &Archetype) {
        match self {
            Self::Table {
                component_id,
                components,
                tables,
                ..
            } => {
                *components = (&**tables)
                    .get_unchecked(archetype.table_id())
                    .get_column_unchecked(*component_id)
                    .data()
                    .cast::<T>();
            }
            Self::SparseSet { entities, .. } => *entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, archetype_index: usize) -> Self::Item {
        match self {
            Self::Table { components, .. } => Mut {
                value: &mut *components.as_ptr().add(archetype_index),
            },
            Self::SparseSet {
                entities,
                sparse_set,
                ..
            } => {
                let entity = *entities.add(archetype_index);
                Mut {
                    value: &mut *(**sparse_set).get_component_unchecked(entity).cast::<T>(),
                }
            }
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
            fn matches_archetype(&self, archetype: &Archetype) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_archetype(archetype))*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            unsafe fn next_archetype(&mut self, archetype: &Archetype) {
                let ($($name,)*) = self;
                $($name.next_archetype(archetype);)*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            #[inline]
            unsafe fn fetch(&mut self, archetype_index: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.fetch(archetype_index),)*)
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
        let e1 = world.spawn().insert_bundle((A(1), B(1)));
        let e2 = world.spawn().insert_bundle((A(2),));
        let values = world.query::<&A>().collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (a, mut b) in world.query_mut::<(&A, &mut B)>() {
            b.0 = 3;
        }
        let values = world.query::<&B>().collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }

    #[test]
    fn stateful_query() {
        let mut world = World::new();
        let mut query_state = QueryState::default();
        let e1 = world.spawn().insert_bundle((A(1), B(1)));
        let e2 = world.spawn().insert_bundle((A(2),));
        unsafe {
            let values = world
                .query::<&A>()
                .with_state(&mut query_state)
                .collect::<Vec<&A>>();
            assert_eq!(values, vec![&A(1), &A(2)]);
        }

        unsafe {
            let mut query_state = QueryState::default();
            for (a, mut b) in world.query::<(&A, &mut B)>().with_state(&mut query_state) {
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
            .components_mut()
            .add(ComponentDescriptor::of::<A>(StorageType::SparseSet))
            .unwrap();

        let e1 = world.spawn().insert_bundle((A(1), B(2)));
        let e2 = world.spawn().insert_bundle((A(2),));

        let values = world.query::<&A>().collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (a, mut b) in world.query::<(&A, &mut B)>() {
            b.0 = 3;
        }

        let values = world.query::<&B>().collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }
}
