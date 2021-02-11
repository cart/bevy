use crate::core::{
    Access, Archetype, ArchetypeComponentId, Bundle, Component, ComponentFlags, ComponentId,
    ComponentSparseSet, Entity, FetchState, StorageType, Table, Tables, World,
};
use std::{any::TypeId, marker::PhantomData, ptr};

pub trait QueryFilter: Sized {
    type State: FetchState;
    const DANGLING: Self;
    unsafe fn init(world: &World, state: &Self::State) -> Self;
    unsafe fn next_table(&mut self, table: &Table);
    unsafe fn matches_entity(&self, table_row: usize) -> bool;
}

impl QueryFilter for () {
    type State = ();

    const DANGLING: Self = ();

    #[inline]
    unsafe fn init(_world: &World, _state: &Self::State) -> Self {
        ()
    }

    #[inline]
    unsafe fn next_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn matches_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct Or<T>(pub T);

/// Filter that retrieves components of type `T` that have either been mutated or added since the start of the frame.
pub struct With<T> {
    marker: PhantomData<T>,
}
pub struct WithState<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: Component> FetchState for WithState<T> {
    fn init(world: &mut World) -> Self {
        let component_id = world.components.get_or_insert_id::<T>();
        Self {
            component_id,
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
}

impl<T: Component> QueryFilter for With<T> {
    type State = WithState<T>;

    const DANGLING: Self = Self {
        marker: PhantomData,
    };

    unsafe fn init(_world: &World, _state: &Self::State) -> Self {
        Self {
            marker: PhantomData,
        }
    }

    #[inline]
    unsafe fn next_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn matches_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct Without<T> {
    marker: PhantomData<T>,
}

pub struct WithoutState<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: Component> FetchState for WithoutState<T> {
    fn init(world: &mut World) -> Self {
        let component_id = world.components.get_or_insert_id::<T>();
        Self {
            component_id,
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
}

impl<T: Component> QueryFilter for Without<T> {
    type State = WithoutState<T>;

    const DANGLING: Self = Self {
        marker: PhantomData,
    };

    unsafe fn init(_world: &World, _state: &Self::State) -> Self {
        Self {
            marker: PhantomData,
        }
    }

    #[inline]
    unsafe fn next_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn matches_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct WithBundle<T: Bundle> {
    marker: PhantomData<T>,
}

pub struct WithBundleState<T: Bundle> {
    component_ids: Vec<ComponentId>,
    marker: PhantomData<T>,
}

impl<T: Bundle> FetchState for WithBundleState<T> {
    fn init(world: &mut World) -> Self {
        let bundle_info = world.bundles.init_info::<T>(&mut world.components);
        Self {
            component_ids: bundle_info.component_ids.clone(),
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
        for component_id in self.component_ids.iter().cloned() {
            if !archetype.contains(component_id) {
                return false;
            }
        }

        true
    }
}

impl<T: Bundle> QueryFilter for WithBundle<T> {
    type State = WithBundleState<T>;

    const DANGLING: Self = Self {
        marker: PhantomData,
    };

    unsafe fn init(_world: &World, _state: &Self::State) -> Self {
        Self {
            marker: PhantomData,
        }
    }

    #[inline]
    unsafe fn next_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn matches_entity(&self, _table_row: usize) -> bool {
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
            unsafe fn next_table(&mut self, table: &Table) {
                let ($($filter,)*) = self;
                $($filter.next_table(table);)*
            }

            #[inline]
            unsafe fn matches_entity(&self, offset: usize) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.matches_entity(offset))*
            }
        }
    };
}

macro_rules! impl_flag_filter {
    (
        $(#[$meta:meta])*
        $name: ident, $state_name: ident, $($flags: expr),+) => {
        $(#[$meta])*
        pub enum $name<T> {
            Table {
                component_id: ComponentId,
                flags: *mut ComponentFlags,
                tables: *const Tables,
                marker: PhantomData<T>,
            },
            SparseSet {
                component_id: ComponentId,
                entities: *const Entity,
                sparse_set: *const ComponentSparseSet,
            },
        }

        pub struct $state_name<T> {
            component_id: ComponentId,
            storage_type: StorageType,
            marker: PhantomData<T>,
        }

        impl<T: Component> FetchState for $state_name<T> {
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
        }



        impl<T: Component> QueryFilter for $name<T> {
            type State = $state_name<T>;
            const DANGLING: Self = Self::Table {
                component_id: ComponentId::new(usize::MAX),
                flags: ptr::null_mut::<ComponentFlags>(),
                tables: ptr::null::<Tables>(),
                marker: PhantomData,
            };

            unsafe fn init(world: &World, state: &Self::State) -> Self {
                match state.storage_type {
                    StorageType::Table => Self::Table {
                        component_id: state.component_id,
                        flags: ptr::null_mut::<ComponentFlags>(),
                        tables: (&world.storages().tables) as *const Tables,
                        marker: PhantomData,
                    },
                    StorageType::SparseSet => Self::SparseSet {
                        component_id: state.component_id,
                        entities: std::ptr::null::<Entity>(),
                        sparse_set: world.storages().sparse_sets.get_unchecked(state.component_id),
                    },
                }
            }

            unsafe fn next_table(&mut self, table: &Table) {
                match self {
                    Self::Table {
                        component_id,
                        flags,
                        ..
                    } => {
                        *flags = table
                            .get_column_unchecked(*component_id)
                            .get_flags_mut_ptr();
                    }
                    Self::SparseSet { entities, .. } => *entities = table.entities().as_ptr(),
                }
            }

            unsafe fn matches_entity(&self, table_row: usize) -> bool {
                match self {
                    Self::Table { flags, .. } => {
                        false $(|| (*flags.add(table_row)).contains($flags))+
                    }
                    Self::SparseSet {
                        entities,
                        sparse_set,
                        ..
                    } => {
                        let entity = *entities.add(table_row);
                        let flags = (*(**sparse_set).get_flags_unchecked(entity));
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

impl_query_filter_tuple!(A);
impl_query_filter_tuple!(A, B);
impl_query_filter_tuple!(A, B, C);
impl_query_filter_tuple!(A, B, C, D);
impl_query_filter_tuple!(A, B, C, D, E);
impl_query_filter_tuple!(A, B, C, D, E, F);
impl_query_filter_tuple!(A, B, C, D, E, F, G);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
