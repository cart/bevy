use crate::{
    core::{
        Access, Archetype, ArchetypeComponentId, Component, ComponentFlags, ComponentId,
        ComponentSparseSet, Entity, Mut, StorageType, Table, Tables, World,
    },
    smaller_tuples_too,
};
use std::{
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
    /// Returns true if (and only if) every table of every archetype matched by this Fetch contains all of the matched components.
    /// This is used to select a more efficient "table iterator" for "dense" queries.
    /// If this returns true, [next_table] and [table_fetch] will be called for iterators
    /// If this returns false, [next_archetype] and [archetype_fetch] will be called for iterators
    fn is_dense(&self) -> bool;
    /// Adjusts internal state to account for the next [Archetype]. This will always be called on archetypes that match this [Fetch]
    unsafe fn next_archetype(&mut self, state: &Self::State, archetype: &Archetype, tables: &Tables);
    /// Adjusts internal state to account for the next [Table]. This will always be called on tables that match this [Fetch]
    unsafe fn next_table(&mut self, state: &Self::State, table: &Table);
    /// Fetch [Self::Item] for the given `archetype_index` in the current [Archetype]. This must always be called after [next_archetype] with an `archetype_index`
    /// in the range of the current [Archetype]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item;
    /// Fetch [Self::Item] for the given `table_row` in the current [Table]. This must always be called after [next_table] with a `table_row`
    /// in the range of the current [Table]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item;
}

/// State used to construct a Fetch. This will be cached inside QueryState, so it is best to move as much data /
// computation here as possible to reduce the cost of constructing Fetch.
pub trait FetchState: Sized {
    fn init(world: &mut World) -> Self;
    fn update_component_access(&self, access: &mut Access<ComponentId>);
    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    );
    fn matches_archetype(&self, archetype: &Archetype) -> bool;
    fn matches_table(&self, table: &Table) -> bool;
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
    fn init(_world: &mut World) -> Self {
        Self
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

    #[inline]
    fn matches_table(&self, _table: &Table) -> bool {
        true
    }
}

impl<'w> Fetch<'w> for FetchEntity {
    type Item = Entity;
    type State = EntityState;

    const DANGLING: Self = FetchEntity {
        entities: std::ptr::null::<Entity>(),
    };

    #[inline]
    fn is_dense(&self) -> bool {
        true
    }

    unsafe fn init(_world: &World, _state: &Self::State) -> Self {
        Self::DANGLING
    }

    #[inline]
    unsafe fn next_archetype(&mut self, _state: &Self::State, archetype: &Archetype, _tables: &Tables) {
        self.entities = archetype.entities().as_ptr();
    }

    #[inline]
    unsafe fn next_table(&mut self, _state: &Self::State, table: &Table) {
        self.entities = table.entities().as_ptr();
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        *self.entities.add(table_row)
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        *self.entities.add(archetype_index)
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
    fn init(world: &mut World) -> Self {
        let component_id = world.components.get_or_insert_id::<T>();
        // SAFE: component_id exists if there is a TypeId pointing to it
        let component_info = unsafe { world.components.get_info_unchecked(component_id) };
        ReadState {
            component_id: component_info.id(),
            storage_type: component_info.storage_type(),
            marker: PhantomData,
        }
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

    fn matches_table(&self, table: &Table) -> bool {
        match self.storage_type {
            StorageType::Table => table.has_column(self.component_id),
            // any table could have any sparse set component
            StorageType::SparseSet => true,
        }
    }

}

pub struct FetchRead<T> {
    storage_type: StorageType,
    table_components: NonNull<T>,
    entity_table_rows: *const usize,
    entities: *const Entity,
    sparse_set: *const ComponentSparseSet,
}

unsafe impl<T> ReadOnlyFetch for FetchRead<T> {}

impl<'w, T: Component> Fetch<'w> for FetchRead<T> {
    type Item = &'w T;
    type State = ReadState<T>;

    const DANGLING: Self = Self {
        storage_type: StorageType::Table,
        table_components: NonNull::dangling(),
        entities: ptr::null::<Entity>(),
        entity_table_rows: ptr::null::<usize>(),
        sparse_set: ptr::null::<ComponentSparseSet>(),
    };

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        let mut value = Self {
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
    unsafe fn next_archetype(&mut self, state: &Self::State, archetype: &Archetype, tables: &Tables) {
        // SAFE: archetype tables always exist
        let table = tables.get_unchecked(archetype.table_id());
        match state.storage_type {
            StorageType::Table => {
                self.entity_table_rows = archetype.entity_table_rows().as_ptr();
                self.table_components = table
                    .get_column_unchecked(state.component_id)
                    .get_ptr()
                    .cast::<T>();
            }
            StorageType::SparseSet => self.entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn next_table(&mut self, state: &Self::State, table: &Table) {
        self.table_components = table
            .get_column_unchecked(state.component_id)
            .get_ptr()
            .cast::<T>();
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        // TODO: ensure table row index is looked up
        match self.storage_type {
            StorageType::Table => {
                let table_row = *self.entity_table_rows.add(archetype_index);
                &*self.table_components.as_ptr().add(table_row)
            }
            StorageType::SparseSet => {
                let entity = *self.entities.add(archetype_index);
                &*(*self.sparse_set).get_unchecked(entity).cast::<T>()
            }
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        &*self.table_components.as_ptr().add(table_row)
    }
}

impl<T: Component> WorldQuery for &mut T {
    type Fetch = FetchWrite<T>;
    type State = WriteState<T>;
}

pub struct FetchWrite<T> {
    storage_type: StorageType,
    table_components: NonNull<T>,
    table_flags: *mut ComponentFlags,
    entities: *const Entity,
    entity_table_rows: *const usize,
    sparse_set: *const ComponentSparseSet,
}

pub struct WriteState<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    marker: PhantomData<T>,
}

impl<T: Component> FetchState for WriteState<T> {
    fn init(world: &mut World) -> Self {
        let component_id = world.components.get_or_insert_id::<T>();
        // SAFE: component_id exists if there is a TypeId pointing to it
        let component_info = unsafe { world.components.get_info_unchecked(component_id) };
        WriteState {
            component_id: component_info.id(),
            storage_type: component_info.storage_type(),
            marker: PhantomData,
        }
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

    fn matches_table(&self, table: &Table) -> bool {
        match self.storage_type {
            StorageType::Table => table.has_column(self.component_id),
            // any table could have any sparse set component
            StorageType::SparseSet => true,
        }
    }
}

impl<'w, T: Component> Fetch<'w> for FetchWrite<T> {
    type Item = Mut<'w, T>;
    type State = WriteState<T>;

    const DANGLING: Self = Self {
        storage_type: StorageType::Table,
        table_components: NonNull::dangling(),
        entities: ptr::null::<Entity>(),
        entity_table_rows: ptr::null::<usize>(),
        sparse_set: ptr::null::<ComponentSparseSet>(),
        table_flags: ptr::null_mut::<ComponentFlags>(),
    };

    #[inline]
    fn is_dense(&self) -> bool {
        match self.storage_type {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    }

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        let mut value = Self {
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
    unsafe fn next_archetype(&mut self, state: &Self::State, archetype: &Archetype, tables: &Tables) {
        // SAFE: archetype tables always exist
        let table = tables.get_unchecked(archetype.table_id());
        match state.storage_type {
            StorageType::Table => {
                self.entity_table_rows = archetype.entity_table_rows().as_ptr();
                let column = table.get_column_unchecked(state.component_id);
                self.table_components = column.get_ptr().cast::<T>();
                self.table_flags = column.get_flags_mut_ptr();
            }
            StorageType::SparseSet => self.entities = archetype.entities().as_ptr(),
        }
    }

    #[inline]
    unsafe fn next_table(&mut self, state: &Self::State, table: &Table) {
        let column = table.get_column_unchecked(state.component_id);
        self.table_components = column.get_ptr().cast::<T>();
        self.table_flags = column.get_flags_mut_ptr();
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        // TODO: ensure table row index is looked up
        match self.storage_type {
            StorageType::Table => {
                let table_row = *self.entity_table_rows.add(archetype_index);
                Mut {
                    value: &mut *self.table_components.as_ptr().add(table_row),
                    flags: &mut *self.table_flags.add(table_row),
                }
            }
            StorageType::SparseSet => {
                let entity = *self.entities.add(archetype_index);
                let (component, flags) = (*self.sparse_set).get_with_flags_unchecked(entity);
                Mut {
                    value: &mut *component.cast::<T>(),
                    flags: &mut *flags,
                }
            }
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        Mut {
            value: &mut *self.table_components.as_ptr().add(table_row),
            flags: &mut *self.table_flags.add(table_row),
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
    fn init(world: &mut World) -> Self {
        Self {
            state: T::init(world),
        }
    }

    fn update_component_access(&self, access: &mut Access<ComponentId>) {
        self.state.update_component_access(access);
    }

    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        if self.state.matches_archetype(archetype) {
            self.state
                .update_archetype_component_access(archetype, access)
        }
    }

    fn matches_archetype(&self, _archetype: &Archetype) -> bool {
        true
    }

    fn matches_table(&self, _table: &Table) -> bool {
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

    #[inline]
    fn is_dense(&self) -> bool {
        self.fetch.is_dense()
    }

    unsafe fn init(world: &World, state: &Self::State) -> Self {
        Self {
            fetch: T::init(world, &state.state),
            matches: false,
        }
    }

    #[inline]
    unsafe fn next_archetype(&mut self, state: &Self::State, archetype: &Archetype, tables: &Tables) {
        self.matches = state.state.matches_archetype(archetype);
        if self.matches {
            self.fetch.next_archetype(&state.state, archetype, tables);
        }
    }

    #[inline]
    unsafe fn next_table(&mut self, state: &Self::State, table: &Table) {
        self.matches = state.state.matches_table(table);
        if self.matches {
            self.fetch.next_table(&state.state, table);
        }
    }

    #[inline]
    unsafe fn archetype_fetch(&mut self, archetype_index: usize) -> Self::Item {
        if self.matches {
            Some(self.fetch.archetype_fetch(archetype_index))
        } else {
            None
        }
    }

    #[inline]
    unsafe fn table_fetch(&mut self, table_row: usize) -> Self::Item {
        if self.matches {
            Some(self.fetch.table_fetch(table_row))
        } else {
            None
        }
    }
}

macro_rules! tuple_impl {
    ($(($name: ident, $state: ident)),*) => {
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

            #[inline]
            unsafe fn next_archetype(&mut self, _state: &Self::State, _archetype: &Archetype, _tables: &Tables) {
                let ($($name,)*) = self;
                let ($($state,)*) = _state;
                $($name.next_archetype($state, _archetype, _tables);)*
            }

            #[inline]
            unsafe fn next_table(&mut self, _state: &Self::State, _table: &Table) {
                let ($($name,)*) = self;
                let ($($state,)*) = _state;
                $($name.next_table($state, _table);)*
            }

            #[inline]
            unsafe fn table_fetch(&mut self, _table_row: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.table_fetch(_table_row),)*)
            }

            #[inline]
            unsafe fn archetype_fetch(&mut self, _archetype_index: usize) -> Self::Item {
                let ($($name,)*) = self;
                ($($name.archetype_fetch(_archetype_index),)*)
            }
        }

        #[allow(non_snake_case)]
        impl<$($name: FetchState),*> FetchState for ($($name,)*) {
            fn init(_world: &mut World) -> Self {
                ($($name::init(_world),)*)
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

            fn matches_table(&self, _table: &Table) -> bool {
                let ($($name,)*) = self;
                true $(&& $name.matches_table(_table))*
            }
        }

        impl<$($name: WorldQuery),*> WorldQuery for ($($name,)*) {
            type Fetch = ($($name::Fetch,)*);
            type State = ($($name::State,)*);
        }

        unsafe impl<$($name: ReadOnlyFetch),*> ReadOnlyFetch for ($($name,)*) {}

    };
}

smaller_tuples_too!(tuple_impl, (O, OS), (N, NS), (M, MS), (L, LS), (K, KS), (J, JS), (I, IS), (H, HS), (G, GS), (F, FS), (E, ES), (D, DS), (C, CS), (B, BS), (A, AS));
