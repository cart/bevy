use crate::core::{Access, Archetype, ArchetypeComponentId, Bundle, BundleInfo, Component, ComponentFlags, ComponentId, ComponentSparseSet, Entity, StorageType, Table, Tables, World};
use std::{any::TypeId, marker::PhantomData, ptr};

pub trait QueryFilter: Sized {
    const DANGLING: Self;
    unsafe fn init(world: &World) -> Option<Self>;
    fn update_component_access(&self, access: &mut Access<ComponentId>);
    fn update_archetype_component_access(
        &self,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    );
    unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool;
    unsafe fn next_table(&mut self, table: &Table);
    unsafe fn matches_entity(&self, table_row: usize) -> bool;
}

impl QueryFilter for () {
    const DANGLING: Self = ();

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {
    }

    #[inline]
    fn update_archetype_component_access(&self, _archetype: &Archetype, _access: &mut Access<ArchetypeComponentId>) {
        
    }

    #[inline]
    unsafe fn init(_world: &World) -> Option<Self> {
        Some(())
    }

    #[inline]
    unsafe fn matches_archetype(&self, _archetype: &Archetype) -> bool {
        true
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
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: Component> QueryFilter for With<T> {
    const DANGLING: Self = Self {
        component_id: ComponentId::new(usize::MAX),
        marker: PhantomData,
    };

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {
    }

    #[inline]
    fn update_archetype_component_access(&self, _archetype: &Archetype, _access: &mut Access<ArchetypeComponentId>) {
        
    }

    unsafe fn init(world: &World) -> Option<Self> {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>())?;
        Some(Self {
            component_id,
            marker: Default::default(),
        })
    }

    #[inline]
    unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool {
        archetype.contains(self.component_id)
    }

    #[inline]
    unsafe fn next_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn matches_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct Without<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: Component> QueryFilter for Without<T> {
    const DANGLING: Self = Self {
        component_id: ComponentId::new(usize::MAX),
        marker: PhantomData,
    };

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {
    }

    #[inline]
    fn update_archetype_component_access(&self, _archetype: &Archetype, _access: &mut Access<ArchetypeComponentId>) {
        
    }

    unsafe fn init(world: &World) -> Option<Self> {
        let components = world.components();
        let component_id = components.get_id(TypeId::of::<T>())?;
        Some(Self {
            component_id,
            marker: Default::default(),
        })
    }

    #[inline]
    unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool {
        !archetype.contains(self.component_id)
    }

    #[inline]
    unsafe fn next_table(&mut self, _table: &Table) {}

    #[inline]
    unsafe fn matches_entity(&self, _table_row: usize) -> bool {
        true
    }
}

pub struct WithBundle<T: Bundle> {
    bundle_info: *const BundleInfo,
    marker: PhantomData<T>,
}


impl<T: Bundle> QueryFilter for WithBundle<T> {
    const DANGLING: Self = Self {
        bundle_info: ptr::null::<BundleInfo>(),
        marker: PhantomData,
    };

    #[inline]
    fn update_component_access(&self, _access: &mut Access<ComponentId>) {
    }

    #[inline]
    fn update_archetype_component_access(&self, _archetype: &Archetype, _access: &mut Access<ArchetypeComponentId>) {
        
    }

    unsafe fn init(world: &World) -> Option<Self> {
        let bundles = world.bundles();
        let bundle_id = bundles.get_id(TypeId::of::<T>())?;
        let bundle_info = bundles.get(bundle_id)? as *const BundleInfo;
        Some(Self {
            bundle_info,
            marker: Default::default(),
        })
    }

    #[inline]
    unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool {
        for component_id in (&*self.bundle_info).component_ids.iter().cloned() {
            if !archetype.contains(component_id) {
                return false;
            }
        }

        true
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
            const DANGLING: Self = ($($filter::DANGLING,)*);
            unsafe fn init(world: &World) -> Option<Self> {
                Some(($($filter::init(world)?,)*))
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn update_component_access(&self, access: &mut Access<ComponentId>) {
                let ($($filter,)*) = self;
                $($filter.update_component_access(access);)*
            }

            #[allow(unused_variables)]
            #[allow(non_snake_case)]
            fn update_archetype_component_access(&self, archetype: &Archetype, access: &mut Access<ArchetypeComponentId>) {
                let ($($filter,)*) = self;
                $($filter.update_archetype_component_access(archetype, access);)*
            }

            #[inline]
            unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.matches_archetype(archetype))*
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
        $name: ident, $($flags: expr),+) => {
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

        impl<T: Component> QueryFilter for $name<T> {
            const DANGLING: Self = Self::Table {
                component_id: ComponentId::new(usize::MAX),
                flags: ptr::null_mut::<ComponentFlags>(),
                tables: ptr::null::<Tables>(),
                marker: PhantomData,
            };

            #[inline]
            fn update_component_access(&self, access: &mut Access<ComponentId>) {
                match self {
                    Self::Table { component_id, .. } => access.add_read(*component_id),
                    Self::SparseSet { component_id, .. } => access.add_read(*component_id),
                }
            }

            #[inline]
            fn update_archetype_component_access(
                &self,
                archetype: &Archetype,
                access: &mut Access<ArchetypeComponentId>,
            ) {
                let component_id = match self {
                    Self::Table { component_id, .. } => *component_id,
                    Self::SparseSet { component_id, .. } => *component_id,
                };

                if let Some(archetype_component_id) = archetype.get_archetype_component_id(component_id) {
                    access.add_read(archetype_component_id);
                }
            }


            unsafe fn init(world: &World) -> Option<Self> {
                let components = world.components();
                let component_id = components.get_id(TypeId::of::<T>())?;
                let component_info = components.get_info_unchecked(component_id);
                Some(match component_info.storage_type() {
                    StorageType::Table => Self::Table {
                        component_id,
                        flags: ptr::null_mut::<ComponentFlags>(),
                        tables: (&world.storages().tables) as *const Tables,
                        marker: PhantomData,
                    },
                    StorageType::SparseSet => Self::SparseSet {
                        component_id,
                        entities: std::ptr::null::<Entity>(),
                        sparse_set: world.storages().sparse_sets.get_unchecked(component_id),
                    },
                })
            }

            unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool {
                match self {
                    Self::Table { component_id, .. } => archetype.contains(*component_id),
                    Self::SparseSet { component_id, .. } => archetype.contains(*component_id),
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
    ComponentFlags::ADDED
);

impl_flag_filter!(
    /// Filter that retrieves components of type `T` that have been mutated since the start of the frame.
    /// Added components do not count as mutated.
    Mutated,
    ComponentFlags::MUTATED
);

impl_flag_filter!(
    /// Filter that retrieves components of type `T` that have been added or mutated since the start of the frame
    Changed,
    ComponentFlags::ADDED, ComponentFlags::MUTATED
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
impl_query_filter_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);
