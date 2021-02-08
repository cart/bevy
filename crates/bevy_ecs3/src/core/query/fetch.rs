use crate::{
    core::{
        Access, Archetype, ArchetypeComponentId, Component, ComponentFlags, ComponentId,
        ComponentSparseSet, Entity, Mut, StorageType, Table, Tables, World,
    },
    smaller_tuples_too,
};
use std::{
    any::TypeId,
    marker::PhantomData,
    ptr::{self, NonNull},
};

pub trait WorldQuery: Send + Sync {
    type Fetch: for<'a> Fetch<'a, State = Self::State>;
    type State: FetchState;
}

pub trait Fetch<'w>: Sized {
    const DANGLING: Self;
    type Item;
    type State: FetchState;
    unsafe fn init(world: &World, state: &Self::State) -> Self;
    fn matches_table(&self, table: &Table) -> bool;
    fn is_dense(&self) -> bool;
    unsafe fn next_table(&mut self, table: &Table);
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item>;
    unsafe fn fetch(&mut self, index: usize) -> Self::Item;
}

/// State used to construct a Fetch. This will be cached inside QueryState, so it is best to move as much data /
// computation here as possible to reduce the cost of constructing Fetch.
pub trait FetchState: Sized {
    fn init(world: &World) -> Option<Self>;
    fn update_component_access(&self, access: &mut Access<ComponentId>);
    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    );
    fn matches_archetype(&self, archetype: &Archetype) -> bool;
}

pub struct ReadState<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    marker: PhantomData<T>,
}

impl<T: Component> FetchState for ReadState<T> {
    fn init(world: &World) -> Option<Self> {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>())?;
        // SAFE: component_id exists if there is a TypeId pointing to it
        let component_info = unsafe { components.get_info_unchecked(component_id) };
        Some(ReadState {
            component_id: component_info.id(),
            storage_type: component_info.storage_type(),
            marker: PhantomData,
        })
    }

    fn update_component_access(&self, access: &mut Access<ComponentId>) {
        access.add_read(self.component_id)
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if let Some(archetype_component_id) =
            archetype.get_archetype_component_id(self.component_id)
        {
            access.add_read(archetype_component_id);
        }
    }

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        archetype.contains(self.component_id)
    }
}

/// A fetch that is read only. This should only be implemented for read-only fetches.
pub unsafe trait ReadOnlyFetch {}

impl WorldQuery for Entity {
    type Fetch = FetchEntity;
    type State = EntityState;
}

pub struct FetchEntity {
    entities: *const Entity,
}

unsafe impl ReadOnlyFetch for FetchEntity {}

pub struct EntityState;

impl FetchState for EntityState {
    fn init(world: &World) -> Option<Self> {
        Some(Self)
    }

    fn update_component_access(&self, access: &mut Access<ComponentId>) {}

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
    }

    #[inline]
    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        true
    }
}

impl<'w> Fetch<'w> for FetchEntity {
    type Item = Entity;
    type State = EntityState;

    const DANGLING: Self = FetchEntity {
        entities: std::ptr::null::<Entity>(),
    };

    fn matches_table(&self, _table: &Table) -> bool {
        true
    }

    #[inline]
    fn is_dense(&self) -> bool {
        true
    }

    unsafe fn init(_world: &World, state: &Self::State) -> Self {
        Self::DANGLING
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
    type State = ReadState<T>;
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
    type State = ReadState<T>;

    const DANGLING: Self = Self::Table {
        component_id: ComponentId::new(usize::MAX),
        components: NonNull::dangling(),
        tables: ptr::null::<Tables>(),
    };

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

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        match state.storage_type {
            StorageType::Table => Self::Table {
                component_id: state.component_id,
                components: NonNull::dangling(),
                tables: (&world.storages().tables) as *const Tables,
            },
            StorageType::SparseSet => Self::SparseSet {
                component_id: state.component_id,
                entities: std::ptr::null::<Entity>(),
                sparse_set: world
                    .storages()
                    .sparse_sets
                    .get_unchecked(state.component_id),
            },
        }
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
    type State = WriteState<T>;
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

pub struct WriteState<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    marker: PhantomData<T>,
}

impl<T: Component> FetchState for WriteState<T> {
    fn init(world: &World) -> Option<Self> {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>())?;
        // SAFE: component_id exists if there is a TypeId pointing to it
        let component_info = unsafe { components.get_info_unchecked(component_id) };
        Some(WriteState {
            component_id: component_info.id(),
            storage_type: component_info.storage_type(),
            marker: PhantomData,
        })
    }

    fn update_component_access(&self, access: &mut Access<ComponentId>) {
        access.add_write(self.component_id)
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if let Some(archetype_component_id) =
            archetype.get_archetype_component_id(self.component_id)
        {
            access.add_write(archetype_component_id);
        }
    }

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        archetype.contains(self.component_id)
    }
}

impl<'w, T: Component> Fetch<'w> for FetchWrite<T> {
    type Item = Mut<'w, T>;
    type State = WriteState<T>;

    const DANGLING: Self = Self::Table {
        component_id: ComponentId::new(usize::MAX),
        components: NonNull::dangling(),
        flags: ptr::null_mut::<ComponentFlags>(),
        tables: ptr::null::<Tables>(),
    };

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

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        match state.storage_type {
            StorageType::Table => Self::Table {
                component_id: state.component_id,
                components: NonNull::dangling(),
                flags: ptr::null_mut(),
                tables: (&world.storages().tables) as *const Tables,
            },
            StorageType::SparseSet => Self::SparseSet {
                component_id: state.component_id,
                entities: std::ptr::null::<Entity>(),
                sparse_set: world
                    .storages()
                    .sparse_sets
                    .get_unchecked(state.component_id),
            },
        }
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
    type State = OptionState<T::State>;
}

pub struct FetchOption<T> {
    fetch: T,
    matches: bool,
}

unsafe impl<T: ReadOnlyFetch> ReadOnlyFetch for FetchOption<T> {}

pub struct OptionState<T: FetchState> {
    state: T,
}

impl<T: FetchState> FetchState for OptionState<T> {
    fn init(world: &World) -> Option<Self> {
        Some(Self {
            state: T::init(world)?,
        })
    }

    fn update_component_access(&self, access: &mut Access<ComponentId>) {
        self.state.update_component_access(access);
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        self.state
            .update_archetype_component_access(archetype, access)
    }

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        true
    }
}

impl<'w, T: Fetch<'w>> Fetch<'w> for FetchOption<T> {
    type Item = Option<T::Item>;
    type State = OptionState<T::State>;

    const DANGLING: Self = Self {
        fetch: T::DANGLING,
        matches: false,
    };

    fn matches_table(&self, _table: &Table) -> bool {
        true
    }

    #[inline]
    fn is_dense(&self) -> bool {
        // option queries must always use try_fetch
        false
    }

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        Self {
            fetch: T::init(world, &state.state),
            matches: false,
        }
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
            type State = ($($name::State,)*);

            const DANGLING: Self = ($($name::DANGLING,)*);

            #[allow(unused_variables)]
            unsafe fn init(world: &World, state: &Self::State) -> Self {
                let ($($name,)*) = state;
                ($($name::init(world, $name),)*)
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

        impl<$($name: FetchState),*> FetchState for ($($name,)*) {
            fn init(world: &World) -> Option<Self> {
                Some(($($name::init(world)?,)*))
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn update_component_access(&self, access: &mut Access<ComponentId>) {
                let ($($name,)*) = self;
                $($name.update_component_access(access);)*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn update_archetype_component_access(&self, archetype: &Archetype, access: &mut Access<ArchetypeComponentId>) {
                let ($($name,)*) = self;
                $($name.update_archetype_component_access(archetype, access);)*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn matches_archetype(&self, archetype: &Archetype) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_archetype(archetype))*
            }

        }

        impl<$($name: WorldQuery),*> WorldQuery for ($($name,)*) {
            type Fetch = ($($name::Fetch,)*);
            type State = ($($name::State,)*);
        }

        unsafe impl<$($name: ReadOnlyFetch),*> ReadOnlyFetch for ($($name,)*) {}

    };
}

smaller_tuples_too!(tuple_impl, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);
