use crate::core::{
    Access, Archetype, ArchetypeComponentId, Bundle, Component, ComponentFlags, ComponentId,
    ComponentSparseSet, Entity, FetchState, StorageType, Table, Tables, World,
};
use bevy_ecs3_macros::all_tuples;
use std::{marker::PhantomData, ptr};

pub trait QueryFilter: Sized {
    type State: FetchState;
    const DANGLING: Self;
    unsafe fn init(world: &World, state: &Self::State) -> Self;
    fn is_dense(&self) -> bool;
    /// Adjusts internal state to account for the next [Table]. This will always be called on tables that match this [QueryFilter]
    /// # Safety
    /// `table` must be from the [World] [QueryFilter::init] was called on. 
    unsafe fn set_table(&mut self, table: &Table);
    /// Adjusts internal state to account for the next [Archetype]. This will always be called on archetypes that match this [QueryFilter]
    /// # Safety
    /// `archetype` and `tables` must be from the [World] [QueryFilter::init] was called on. 
    unsafe fn set_archetype(&mut self, archetype: &Archetype, tables: &Tables);
    /// Returns true if the entity at the given `table_row` matches this filter. This must always be called after [set_table] with a `table_row`
    /// in the range of the current [Table]
    /// # Safety
    /// Must always be called _after_ [QueryFilter::set_table]. `table_row` must be in the range of the current table
    unsafe fn matches_table_entity(&self, table_row: usize) -> bool;
    /// Returns true if the entity at the given `archetype_index` matches this filter. This must always be called after [set_archetype] with an `archetype_index`
    /// in the range of the current [Archetype]
    /// # Safety
    /// Must always be called _after_ [QueryFilter::set_archetype]. `archetype_index` must be in the range of the current archetype
    unsafe fn matches_archetype_entity(&self, archetype_index: usize) -> bool;
}

pub struct Or<T>(pub T);

/// Filter that retrieves components of type `T` that have either been mutated or added since the start of the frame.
pub struct With<T> {
    storage_type: StorageType,
    marker: PhantomData<T>,
}
pub struct WithState<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    // NOTE: PhantomData<fn()-> T> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> T>,
}

// SAFE: no component access or archetype component access
unsafe impl<T: Component> FetchState for WithState<T> {
    fn init(world: &mut World) -> Self {
        let component_id = world.components.get_or_insert_id::<T>();
        // SAFE: ComponentInfo was just created above
        let component_info = unsafe { world.components.get_info_unchecked(component_id) };
        Self {
            component_id,
            storage_type: component_info.storage_type(),
            marker: PhantomData,
        }
    }

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {}

    #[inline]
    fn update_archetype_component_access(
        &self,
        _archetype: &Archetype,
        _access: &mut Access<ArchetypeComponentId>,
    ) {
    }

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        archetype.contains(self.component_id)
    }

    fn matches_table(&self, table: &Table) -> bool {
        table.has_column(self.component_id)
    }
}

impl<T: Component> QueryFilter for With<T> {
    type State = WithState<T>;

    const DANGLING: Self = Self {
        storage_type: StorageType::Table,
        marker: PhantomData,
    };

    unsafe fn init(_world: &World, state: &Self::State) -> Self {
        Self {
            storage_type: state.storage_type,
            marker: PhantomData,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        self.storage_type == StorageType::Table
    }

    #[inline]
    unsafe fn set_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn set_archetype(&mut self, _archetype: &Archetype, _tables: &Tables) {}

    #[inline]
    unsafe fn matches_archetype_entity(&self, _archetype_index: usize) -> bool {
        true
    }

    #[inline]
    unsafe fn matches_table_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct Without<T> {
    storage_type: StorageType,
    marker: PhantomData<T>,
}

pub struct WithoutState<T> {
    component_id: ComponentId,
    storage_type: StorageType,
    // NOTE: PhantomData<fn()-> T> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> T>,
}

// SAFE: no component access or archetype component access
unsafe impl<T: Component> FetchState for WithoutState<T> {
    fn init(world: &mut World) -> Self {
        let component_id = world.components.get_or_insert_id::<T>();
        // SAFE: ComponentInfo was just created above
        let component_info = unsafe { world.components.get_info_unchecked(component_id) };
        Self {
            component_id,
            storage_type: component_info.storage_type(),
            marker: PhantomData,
        }
    }

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {}

    #[inline]
    fn update_archetype_component_access(
        &self,
        _archetype: &Archetype,
        _access: &mut Access<ArchetypeComponentId>,
    ) {
    }

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        !archetype.contains(self.component_id)
    }

    fn matches_table(&self, table: &Table) -> bool {
        !table.has_column(self.component_id)
    }
}

impl<T: Component> QueryFilter for Without<T> {
    type State = WithoutState<T>;

    const DANGLING: Self = Self {
        storage_type: StorageType::Table,
        marker: PhantomData,
    };

    unsafe fn init(_world: &World, state: &Self::State) -> Self {
        Self {
            storage_type: state.storage_type,
            marker: PhantomData,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        self.storage_type == StorageType::Table
    }

    #[inline]
    unsafe fn set_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn set_archetype(&mut self, _archetype: &Archetype, _tables: &Tables) {}

    #[inline]
    unsafe fn matches_archetype_entity(&self, _archetype_index: usize) -> bool {
        true
    }

    #[inline]
    unsafe fn matches_table_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct WithBundle<T: Bundle> {
    is_dense: bool,
    marker: PhantomData<T>,
}

pub struct WithBundleState<T: Bundle> {
    component_ids: Vec<ComponentId>,
    is_dense: bool,
    // NOTE: PhantomData<fn()-> T> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> T>,
}

// SAFE: no component access or archetype component access
unsafe impl<T: Bundle> FetchState for WithBundleState<T> {
    fn init(world: &mut World) -> Self {
        let bundle_info = world.bundles.init_info::<T>(&mut world.components);
        let components = &world.components;
        Self {
            component_ids: bundle_info.component_ids.clone(),
            is_dense: !bundle_info.component_ids.iter().any(|id| unsafe {
                components.get_info_unchecked(*id).storage_type() != StorageType::Table
            }),
            marker: PhantomData,
        }
    }

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {}

    #[inline]
    fn update_archetype_component_access(
        &self,
        _archetype: &Archetype,
        _access: &mut Access<ArchetypeComponentId>,
    ) {
    }

    fn matches_archetype(&self, archetype: &Archetype) -> bool {
        self.component_ids.iter().all(|id| archetype.contains(*id))
    }

    fn matches_table(&self, table: &Table) -> bool {
        self.component_ids.iter().all(|id| table.has_column(*id))
    }
}

impl<T: Bundle> QueryFilter for WithBundle<T> {
    type State = WithBundleState<T>;

    const DANGLING: Self = Self {
        is_dense: true,
        marker: PhantomData,
    };

    unsafe fn init(_world: &World, state: &Self::State) -> Self {
        Self {
            is_dense: state.is_dense,
            marker: PhantomData,
        }
    }

    #[inline]
    fn is_dense(&self) -> bool {
        self.is_dense
    }

    #[inline]
    unsafe fn set_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn set_archetype(&mut self, _archetype: &Archetype, _tables: &Tables) {}

    #[inline]
    unsafe fn matches_archetype_entity(&self, _archetype_index: usize) -> bool {
        true
    }

    #[inline]
    unsafe fn matches_table_entity(&self, _table_row: usize) -> bool {
        true
    }
}

macro_rules! impl_query_filter_tuple {
    ($($filter: ident),*) => {
        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        impl<$($filter: QueryFilter),*> QueryFilter for ($($filter,)*) {
            type State = ($($filter::State,)*);
            const DANGLING: Self = ($($filter::DANGLING,)*);

            unsafe fn init(world: &World, state: &Self::State) -> Self {
                let ($($filter,)*) = state;
                ($($filter::init(world, $filter),)*)
            }

            #[inline]
            fn is_dense(&self) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.is_dense())*
            }

            #[inline]
            unsafe fn set_table(&mut self, table: &Table) {
                let ($($filter,)*) = self;
                $($filter.set_table(table);)*
            }

            #[inline]
            unsafe fn set_archetype(&mut self, archetype: &Archetype, tables: &Tables) {
                let ($($filter,)*) = self;
                $($filter.set_archetype(archetype, tables);)*
            }

            #[inline]
            unsafe fn matches_table_entity(&self, table_row: usize) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.matches_table_entity(table_row))*
            }

            #[inline]
            unsafe fn matches_archetype_entity(&self, archetype_index: usize) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.matches_archetype_entity(archetype_index))*
            }
        }

        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        impl<$($filter: QueryFilter),*> QueryFilter for Or<($($filter,)*)> {
            type State = Or<($($filter::State,)*)>;
            const DANGLING: Self = Or(($($filter::DANGLING,)*));

            unsafe fn init(world: &World, state: &Self::State) -> Self {
                let ($($filter,)*) = &state.0;
                Or(($($filter::init(world, $filter),)*))
            }

            #[inline]
            fn is_dense(&self) -> bool {
                let ($($filter,)*) = &self.0;
                true $(&& $filter.is_dense())*
            }

            #[inline]
            unsafe fn set_table(&mut self, table: &Table) {
                let ($($filter,)*) = &mut self.0;
                $($filter.set_table(table);)*
            }

            #[inline]
            unsafe fn set_archetype(&mut self, archetype: &Archetype, tables: &Tables) {
                let ($($filter,)*) = &mut self.0;
                $($filter.set_archetype(archetype, tables);)*
            }


            #[inline]
            unsafe fn matches_table_entity(&self, table_row: usize) -> bool {
                let ($($filter,)*) = &self.0;
                false $(|| $filter.matches_table_entity(table_row))*
            }

            #[inline]
            unsafe fn matches_archetype_entity(&self, archetype_index: usize) -> bool {
                let ($($filter,)*) = &self.0;
                false $(|| $filter.matches_archetype_entity(archetype_index))*
            }
        }

        // SAFE: update_component_access and update_archetype_component_access are called for each item in the tuple
        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        unsafe impl<$($filter: FetchState),*> FetchState for Or<($($filter,)*)> {
            fn init(world: &mut World) -> Self {
                Or(($($filter::init(world),)*))
            }

            fn update_component_access(&self, access: &mut Access<ComponentId>) {
                let ($($filter,)*) = &self.0;
                $($filter.update_component_access(access);)*
            }

            fn update_archetype_component_access(&self, archetype: &Archetype, access: &mut Access<ArchetypeComponentId>) {
                let ($($filter,)*) = &self.0;
                $($filter.update_archetype_component_access(archetype, access);)*
            }

            fn matches_archetype(&self, archetype: &Archetype) -> bool {
                let ($($filter,)*) = &self.0;
                false $(|| $filter.matches_archetype(archetype))*
            }

            fn matches_table(&self, table: &Table) -> bool {
                let ($($filter,)*) = &self.0;
                false $(|| $filter.matches_table(table))*
            }
        }
    };
}

macro_rules! impl_flag_filter {
    (
        $(#[$meta:meta])*
        $name: ident, $state_name: ident, $($flags: expr),+) => {
        $(#[$meta])*
        pub struct $name<T> {
            component_id: ComponentId,
            storage_type: StorageType,
            table_flags: *mut ComponentFlags,
            entity_table_rows: *const usize,
            marker: PhantomData<T>,
            entities: *const Entity,
            sparse_set: *const ComponentSparseSet,
        }

        pub struct $state_name<T> {
            component_id: ComponentId,
            storage_type: StorageType,
            // NOTE: PhantomData<fn()-> T> gives this safe Send/Sync impls
            marker: PhantomData<fn() -> T>,
        }

        // SAFE: this reads the T component. archetype component access and component access are updated to reflect that
        unsafe impl<T: Component> FetchState for $state_name<T> {
            fn init(world: &mut World) -> Self {
                let component_id = world.components.get_or_insert_id::<T>();
                // SAFE: component_id exists if there is a TypeId pointing to it
                let component_info = unsafe { world.components.get_info_unchecked(component_id) };
                Self {
                    component_id,
                    storage_type: component_info.storage_type(),
                    marker: PhantomData,
                }
            }

            #[inline]
            fn update_component_access(&self, access: &mut Access<ComponentId>) {
                access.add_read(self.component_id);
            }

            #[inline]
            fn update_archetype_component_access(
                &self,
                archetype: &Archetype,
                access: &mut Access<ArchetypeComponentId>,
            ) {
                if let Some(archetype_component_id) = archetype.get_archetype_component_id(self.component_id) {
                    access.add_read(archetype_component_id);
                }
            }

            fn matches_archetype(&self, archetype: &Archetype) -> bool {
                archetype.contains(self.component_id)
            }

            fn matches_table(&self, table: &Table) -> bool {
                table.has_column(self.component_id)
            }
        }



        impl<T: Component> QueryFilter for $name<T> {
            type State = $state_name<T>;
            const DANGLING: Self = Self {
                component_id: ComponentId::new(usize::MAX),
                storage_type: StorageType::Table,
                table_flags: ptr::null_mut::<ComponentFlags>(),
                entities: ptr::null::<Entity>(),
                entity_table_rows: ptr::null::<usize>(),
                sparse_set: ptr::null::<ComponentSparseSet>(),
                marker: PhantomData,
            };

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
            fn is_dense(&self) -> bool {
                self.storage_type == StorageType::Table
            }

            unsafe fn set_table(&mut self, table: &Table) {
                self.table_flags = table
                    .get_column_unchecked(self.component_id)
                    .get_flags_mut_ptr();
            }

            unsafe fn set_archetype(&mut self, archetype: &Archetype, tables: &Tables) {
                let table = tables.get_unchecked(archetype.table_id());
                match self.storage_type {
                    StorageType::Table => {
                        self.entity_table_rows = archetype.entity_table_rows().as_ptr();
                        self.table_flags = table
                            .get_column_unchecked(self.component_id)
                            .get_flags_mut_ptr();
                    }
                    StorageType::SparseSet => self.entities = archetype.entities().as_ptr(),
                }
            }

            unsafe fn matches_table_entity(&self, table_row: usize) -> bool {
                false $(|| (*self.table_flags.add(table_row)).contains($flags))+
            }

            unsafe fn matches_archetype_entity(&self, archetype_index: usize) -> bool {
                match self.storage_type {
                    StorageType::Table => {
                        let table_row = *self.entity_table_rows.add(archetype_index);
                        false $(|| (*self.table_flags.add(table_row)).contains($flags))+
                    }
                    StorageType::SparseSet => {
                        let entity = *self.entities.add(archetype_index);
                        let flags = (*(*self.sparse_set).get_flags_unchecked(entity));
                        false $(|| flags.contains($flags))+
                    }
                }
            }
        }
    };
}

impl_flag_filter!(
    /// Filter that retrieves components of type `T` that have been added since the start of the frame
    Added,
    AddedState,
    ComponentFlags::ADDED
);

impl_flag_filter!(
    /// Filter that retrieves components of type `T` that have been mutated since the start of the frame.
    /// Added components do not count as mutated.
    Mutated,
    MutatedState,
    ComponentFlags::MUTATED
);

impl_flag_filter!(
    /// Filter that retrieves components of type `T` that have been added or mutated since the start of the frame
    Changed,
    ChangedState,
    ComponentFlags::ADDED,
    ComponentFlags::MUTATED
);

all_tuples!(impl_query_filter_tuple, 0, 15, F);
