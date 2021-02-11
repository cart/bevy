use crate::{
    core::{
        Access, Archetype, ArchetypeComponentId, Component, ComponentFlags, ComponentId,
        ComponentSparseSet, Entity, Mut, StorageType, Table, World,
    },
    smaller_tuples_too,
};
use std::{any::{TypeId, type_name}, marker::PhantomData, ptr::{self, NonNull}};

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
    unsafe fn next_table_dense(&mut self, table: &Table);
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
    fn init(_world: &World) -> Option<Self> {
        Some(Self)
    }

    fn update_component_access(&self, _access: &mut Access<ComponentId>) {}

    fn update_archetype_component_access(
        &self,
        _archetype: &Archetype,
        _access: &mut Access<ArchetypeComponentId>,
    ) {
    }

    #[inline]
    fn matches_archetype(&self, _archetype: &Archetype) -> bool {
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

    unsafe fn init(_world: &World, _state: &Self::State) -> Self {
        Self::DANGLING
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        self.entities = table.entities().as_ptr();
    }

    #[inline]
    unsafe fn next_table_dense(&mut self, table: &Table) {
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

pub struct FetchRead<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    components: NonNull<T>,
    entities: *const Entity,
    sparse_set: *const ComponentSparseSet,
}

unsafe impl<T> ReadOnlyFetch for FetchRead<T> {}

impl<'w, T: Component> Fetch<'w> for FetchRead<T> {
    type Item = &'w T;
    type State = ReadState<T>;

    const DANGLING: Self = Self {
        component_id: ComponentId::new(usize::MAX),
        storage_type: StorageType::Table,
        components: NonNull::dangling(),
        entities: ptr::null::<Entity>(),
        sparse_set: ptr::null::<ComponentSparseSet>(),
    };

    fn matches_table(&self, table: &Table) -> bool {
        match self.storage_type {
            StorageType::Table => table.has_column(self.component_id),
            // any table could have any sparse set component
            StorageType::SparseSet => true,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        let mut value = Self {
            component_id: state.component_id,
            storage_type: state.storage_type,
            ..Self::DANGLING
        };
        if state.storage_type == StorageType::SparseSet {
            value.sparse_set = world
                .storages()
                .sparse_sets
                .get_unchecked(state.component_id);
        }
        value
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        match self.storage_type {
            StorageType::Table => {
                self.components = table
                    .get_column_unchecked(self.component_id)
                    .get_ptr()
                    .cast::<T>();
            }
            StorageType::SparseSet => self.entities = table.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn next_table_dense(&mut self, table: &Table) {
        self.components = table
            .get_column_unchecked(self.component_id)
            .get_ptr()
            .cast::<T>();
    }

    #[inline]
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
        match self.storage_type {
            StorageType::Table => Some(&*self.components.as_ptr().add(index)),
            StorageType::SparseSet => {
                let entity = *self.entities.add(index);
                (*self.sparse_set).get(entity).map(|c| &*c.cast::<T>())
            }
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, index: usize) -> Self::Item {
        &*self.components.as_ptr().add(index)
    }
}

impl<T: Component> WorldQuery for &mut T {
    type Fetch = FetchWrite<T>;
    type State = WriteState<T>;
}

pub struct FetchWrite<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    components: NonNull<T>,
    entities: *const Entity,
    sparse_set: *const ComponentSparseSet,
    flags: *mut ComponentFlags,
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
        access.add_write(self.component_id);
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

    const DANGLING: Self = Self {
        component_id: ComponentId::new(usize::MAX),
        storage_type: StorageType::Table,
        components: NonNull::dangling(),
        entities: ptr::null::<Entity>(),
        sparse_set: ptr::null::<ComponentSparseSet>(),
        flags: ptr::null_mut::<ComponentFlags>(),
    };

    fn matches_table(&self, table: &Table) -> bool {
        match self.storage_type {
            StorageType::Table => table.has_column(self.component_id),
            // any table could have any sparse set component
            StorageType::SparseSet => true,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        let mut value = Self {
            component_id: state.component_id,
            storage_type: state.storage_type,
            ..Self::DANGLING
        };
        if state.storage_type == StorageType::SparseSet {
            value.sparse_set = world
                .storages()
                .sparse_sets
                .get_unchecked(state.component_id);
        }
        value
    }

    #[inline]
    unsafe fn next_table(&mut self, table: &Table) {
        match self.storage_type {
            StorageType::Table => {
                let column = table.get_column_unchecked(self.component_id);
                self.components = column.get_ptr().cast::<T>();
                self.flags = column.get_flags_mut_ptr();
            }
            StorageType::SparseSet => self.entities = table.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn next_table_dense(&mut self, table: &Table) {
        let column = table.get_column_unchecked(self.component_id);
        self.components = column.get_ptr().cast::<T>();
        self.flags = column.get_flags_mut_ptr();
    }

    #[inline]
    unsafe fn try_fetch(&mut self, index: usize) -> Option<Self::Item> {
        match self.storage_type {
            StorageType::Table => Some(Mut {
                value: &mut *self.components.as_ptr().add(index),
                flags: &mut *self.flags.add(index),
            }),
            StorageType::SparseSet => {
                let entity = *self.entities.add(index);
                (*self.sparse_set).get_with_flags(entity).map(|(c, f)| Mut {
                    value: &mut *c.cast::<T>(),
                    flags: &mut *f,
                })
            }
        }
    }

    #[inline]
    unsafe fn fetch(&mut self, index: usize) -> Self::Item {
        Mut {
            value: &mut *self.components.as_ptr().add(index),
            flags: &mut *self.flags.add(index),
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

    fn matches_archetype(&self, _archetype: &Archetype) -> bool {
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
    unsafe fn next_table_dense(&mut self, table: &Table) {
        self.matches = self.fetch.matches_table(table);
        if self.matches {
            self.fetch.next_table_dense(table);
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
        #[allow(non_snake_case)]
        impl<'a, $($name: Fetch<'a>),*> Fetch<'a> for ($($name,)*) {
            type Item = ($($name::Item,)*);
            type State = ($($name::State,)*);

            const DANGLING: Self = ($($name::DANGLING,)*);

            unsafe fn init(_world: &World, state: &Self::State) -> Self {
                let ($($name,)*) = state;
                ($($name::init(_world, $name),)*)
            }


            #[inline]
            fn is_dense(&self) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.is_dense())*
            }

            fn matches_table(&self, _table: &Table) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_table(_table))*
            }

            #[inline]
            unsafe fn next_table(&mut self, _table: &Table) {
                let ($($name,)*) = self;
                $($name.next_table(_table);)*
            }

            #[inline]
            unsafe fn next_table_dense(&mut self, _table: &Table) {
                let ($($name,)*) = self;
                $($name.next_table_dense(_table);)*
            }

            #[inline]
            unsafe fn fetch(&mut self, _index: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.fetch(_index),)*)
            }

            #[inline]
            unsafe fn try_fetch(&mut self, _index: usize) -> Option<Self::Item> {
                let ($($name,)*) = self;
                Some(($($name.try_fetch(_index)?,)*))
            }
        }

        #[allow(non_snake_case)]
        impl<$($name: FetchState),*> FetchState for ($($name,)*) {
            fn init(_world: &World) -> Option<Self> {
                Some(($($name::init(_world)?,)*))
            }

            fn update_component_access(&self, _access: &mut Access<ComponentId>) {
                let ($($name,)*) = self;
                $($name.update_component_access(_access);)*
            }

            fn update_archetype_component_access(&self, _archetype: &Archetype, _access: &mut Access<ArchetypeComponentId>) {
                let ($($name,)*) = self;
                $($name.update_archetype_component_access(_archetype, _access);)*
            }

            fn matches_archetype(&self, _archetype: &Archetype) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_archetype(_archetype))*
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
