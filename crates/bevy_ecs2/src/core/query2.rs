use std::{any::TypeId, ptr::NonNull};

use fixedbitset::{FixedBitSet, Ones};

use crate::{Archetype, ArchetypeId, Archetypes, ArchetypesGeneration, Component, ComponentFlags, ComponentId, ComponentSparseSet, Entity, Mut, SparseSets, StorageType, World};

pub trait WorldQuery {
    // TODO: try hoisting this up
    type Fetch: for<'a> Fetch<'a>;
}

pub trait Fetch<'w>: Sized {
    type Item;
    fn update_archetypes(&mut self, archetypes: &Archetypes, query_state: &mut QueryState);
    unsafe fn init(world: &World, query_state: &mut QueryState) -> Self;
    fn next_archetype(&mut self, archetype: &Archetype);
    unsafe fn fetch(&mut self, archetype_index: usize) -> Option<Self::Item>;
}

pub struct QueryState {
    matched_archetypes: FixedBitSet,
    ignored_archetypes: FixedBitSet,
    accessed_archetypes: FixedBitSet,
    // archetype_read_components: FixedBitSet,
    // archetype_write_components: FixedBitSet,
    archetypes_generation: ArchetypesGeneration,
    // global_read_components: FixedBitSet,
    // global_write_components: FixedBitSet,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            accessed_archetypes: FixedBitSet::default(),
            matched_archetypes: FixedBitSet::default(),
            ignored_archetypes: FixedBitSet::default(),
            // archetype_read_components: FixedBitSet::default(),
            // archetype_write_components: FixedBitSet::default(),
            archetypes_generation: ArchetypesGeneration(u32::MAX),
            // global_read_components: FixedBitSet::default(),
            // global_write_components: FixedBitSet::default(),
        }
    }
}

impl QueryState {
    #[inline]
    fn update_archetypes(&mut self, archetypes: &Archetypes) -> bool {
        // PERF: could consume archetype change events here to avoid updating all archetypes whenever a new archetype is added
        if self.archetypes_generation == archetypes.generation() {
            false
        } else {
            self.matched_archetypes.grow(archetypes.len());
            self.ignored_archetypes.grow(archetypes.len());
            self.accessed_archetypes.grow(archetypes.len());
            self.matched_archetypes.clear();
            self.ignored_archetypes.clear();
            true
        }
    }

    #[inline]
    fn accessed_archetypes(&self) -> &FixedBitSet {
        &self.accessed_archetypes
    }

    #[inline]
    fn access_archetype(&mut self, archetype_id: ArchetypeId) {
        self.matched_archetypes.set(archetype_id.index(), true);
    }

    #[inline]
    fn ignore_archetype(&mut self, archetype_id: ArchetypeId) {
        self.ignored_archetypes.set(archetype_id.index(), true);
    }

    #[inline]
    fn access_all_archetypes(&mut self, archetypes: &Archetypes) {
        self.matched_archetypes.set_range(0..archetypes.len(), true);
    }

    #[inline]
    fn update_accessed_archetypes(&mut self) {
        self.accessed_archetypes.clear();
        self.accessed_archetypes
            .union_with(&self.matched_archetypes);
        self.accessed_archetypes
            .difference_with(&self.ignored_archetypes);
    }
}

pub struct QueryIter<'w, 's, Q: WorldQuery> {
    archetypes: &'w Archetypes,
    archetype_id_iter: Ones<'s>,
    fetch: Q::Fetch,
    archetype_len: usize,
    archetype_index: usize,
}

impl<'w, 's, Q: WorldQuery> QueryIter<'w, 's, Q> {
    pub unsafe fn new(world: &'w World, query_state: &'s mut QueryState) -> Self {
        let mut fetch = <Q::Fetch as Fetch>::init(world, query_state);
        let archetypes = world.archetypes();
        if query_state.update_archetypes(archetypes) {
            fetch.update_archetypes(archetypes, query_state);
            query_state.update_accessed_archetypes();
        }

        QueryIter {
            fetch,
            archetypes,
            archetype_id_iter: query_state.accessed_archetypes.ones(),
            archetype_len: 0,
            archetype_index: 0,
        }
    }
}

impl<'w, 's, Q: WorldQuery> Iterator for QueryIter<'w, 's, Q> {
    type Item = <Q::Fetch as Fetch<'w>>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            loop {
                if self.archetype_index == self.archetype_len {
                    let archetype_id = ArchetypeId(self.archetype_id_iter.next()? as u32);
                    let archetype = self.archetypes.get_unchecked(archetype_id);
                    self.fetch.next_archetype(archetype);
                    self.archetype_len = archetype.len();
                    self.archetype_index = 0;
                    continue;
                }

                let item = self.fetch.fetch(self.archetype_index);
                self.archetype_index += 1;
                return item;
            }
        }
    }
}

impl<T: Component> WorldQuery for &T {
    type Fetch = FetchRead<T>;
}

pub enum FetchRead<T> {
    Archetype(NonNull<T>),
    SparseSet {
        entities: NonNull<Entity>,
        sparse_set: *mut ComponentSparseSet,
    },
}

impl<'w, T: Component> Fetch<'w> for FetchRead<T> {
    type Item = &'w T;

    fn update_archetypes(&mut self, archetypes: &Archetypes, query_state: &mut QueryState) {
        match self {
            Self::Archetype(_) => {
                for archetype in archetypes.iter() {
                    if archetype.has_type(TypeId::of::<T>()) {
                        query_state.access_archetype(archetype.id());
                    } else {
                        query_state.ignore_archetype(archetype.id());
                    }
                }
            }
            Self::SparseSet { .. } => {
                query_state.access_all_archetypes(archetypes);
            }
        }
    }

    unsafe fn init(world: &World, query_state: &mut QueryState) -> Self {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>()).unwrap();
        let component_info = components.get_info(component_id).unwrap();
        match component_info.storage_type {
            StorageType::Archetype => Self::Archetype(NonNull::dangling()),
            StorageType::SparseSet => Self::SparseSet {
                entities: NonNull::dangling(),
                sparse_set: world.sparse_sets().get_unchecked(component_id).unwrap(),
            },
        }
    }

    #[inline]
    fn next_archetype(&mut self, archetype: &Archetype) {
        match self {
            Self::Archetype(components) => {
                *components = archetype.get::<T>().unwrap();
            }
            Self::SparseSet { entities, .. } => *entities = archetype.entities(),
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, archetype_index: usize) -> Option<Self::Item> {
        match self {
            Self::Archetype(components) => Some(&*components.as_ptr().add(archetype_index)),
            Self::SparseSet {
                entities,
                sparse_set,
            } => {
                let entity = *entities.as_ptr().add(archetype_index);
                (**sparse_set)
                    .get_component(entity)
                    .map(|value| &*value.cast::<T>())
            }
        }
    }
}

impl<T: Component> WorldQuery for &mut T {
    type Fetch = FetchWrite<T>;
}

pub enum FetchWrite<T> {
    Archetype {
        components: NonNull<T>,
        flags: NonNull<ComponentFlags>,
    },
    SparseSet(NonNull<ComponentSparseSet>),
}

impl<'w, T: Component> Fetch<'w> for FetchWrite<T> {
    type Item = Mut<'w, T>;

    fn update_archetypes(&mut self, archetypes: &Archetypes, query_state: &mut QueryState) {
        for archetype in archetypes.iter() {
            if archetype.has_type(TypeId::of::<T>()) {
                query_state.access_archetype(archetype.id());
            } else {
                query_state.ignore_archetype(archetype.id());
            }
        }
    }

    unsafe fn init(world: &World, query_state: &mut QueryState) -> Self {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>()).unwrap();
        let component_info = components.get_info(component_id).unwrap();
        match component_info.storage_type {
            StorageType::Archetype => Self::Archetype {
                components: NonNull::dangling(),
                flags: NonNull::dangling(),
            },
            StorageType::SparseSet => {
                panic!();
                // Self::SparseSet(world.sparse_sets().get(component_id).unwrap().as_ptr())
            }
        }
    }

    #[inline]
    fn next_archetype(&mut self, archetype: &Archetype) {
        if let Self::Archetype { components, flags } = self {
            archetype
                .get_with_type_state::<T>()
                .map(|(archetype_components, type_state)| {
                    *components = archetype_components;
                    *flags = type_state.component_flags();
                });
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, archetype_index: usize) -> Option<Self::Item> {
        match self {
            Self::Archetype { components, flags } => Some(Mut {
                value: &mut *components.as_ptr().add(archetype_index),
                flags: &mut *flags.as_ptr().add(archetype_index),
            }),
            Self::SparseSet(sparse_set_ptr) => panic!(),
        }
    }
}

macro_rules! tuple_impl {
    ($($name: ident),*) => {
        impl<'a, $($name: Fetch<'a>),*> Fetch<'a> for ($($name,)*) {
            type Item = ($($name::Item,)*);

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn update_archetypes(&mut self, archetypes: &Archetypes, query_state: &mut QueryState) {
                let ($($name,)*) = self;
                $($name.update_archetypes(archetypes, query_state);)*
            }

            #[allow(unused_variables)]
            unsafe fn init(world: &World, query_state: &mut QueryState) -> Self {
                ($($name::init(world, query_state),)*)
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn next_archetype(&mut self, archetype: &Archetype) {
                let ($($name,)*) = self;
                $($name.next_archetype(archetype);)*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            #[inline]
            unsafe fn fetch(&mut self, archetype_index: usize) -> Option<Self::Item> {
                let ($($name,)*) = self;
                Some(($($name.fetch(archetype_index)?,)*))
            }
        }

        impl<$($name: WorldQuery),*> WorldQuery for ($($name,)*) {
            type Fetch = ($($name::Fetch,)*);
        }

        // unsafe impl<$($name: ReadOnlyFetch),*> ReadOnlyFetch for ($($name,)*) {}

    };
}

smaller_tuples_too!(tuple_impl, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);

#[cfg(test)]
mod tests {
    use crate::{ComponentDescriptor, StorageType, World};

    use super::QueryState;

    #[derive(Debug, Eq, PartialEq)]
    struct A(usize);
    #[derive(Debug, Eq, PartialEq)]
    struct B(usize);

    #[test]
    fn query2() {
        let mut world = World::new();
        let mut query_state = QueryState::default();
        let e1 = world.spawn((A(1), B(1)));
        let e2 = world.spawn((A(2),));
        let values = world
            .query_with_state::<&A>(&mut query_state)
            .collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (a, mut b) in world.query_with_state::<(&A, &mut B)>(&mut query_state) {
            b.0 = 3;
        }
        let values = world
            .query_with_state::<&B>(&mut query_state)
            .collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }

#[test]
fn multi_storage_query() {
    let mut world = World::new();
    world
        .components_mut()
        .add(ComponentDescriptor::of::<A>(StorageType::SparseSet))
        .unwrap();

    let e1 = world.spawn((A(1), B(2)));
    let e2 = world.spawn((A(2),));

    let mut query_state = QueryState::default();
    let values = world
        .query_with_state::<&A>(&mut query_state)
        .collect::<Vec<&A>>();
    assert_eq!(values, vec![&A(1), &A(2)]);

    for (a, mut b) in world.query_with_state::<(&A, &mut B)>(&mut query_state) {
        b.0 = 3;
    }

    let values = world
        .query_with_state::<&B>(&mut query_state)
        .collect::<Vec<&B>>();
    assert_eq!(values, vec![&B(3)]);
}
}
