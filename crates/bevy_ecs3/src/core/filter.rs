use crate::core::{
    Archetype, Bundle, BundleInfo, Component, ComponentFlags, ComponentId, Table, World,
};
use std::{
    any::TypeId,
    marker::PhantomData,
    ptr::{self, NonNull},
};

pub trait QueryFilter: Sized {
    const DANGLING: Self;
    unsafe fn init(world: &World) -> Option<Self>;
    unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool;
    unsafe fn next_table(&mut self, table: &Table);
    unsafe fn next_archetype(&mut self, archetype: &Archetype);
    unsafe fn matches_entity(&self, _offset: usize) -> bool;
}

impl QueryFilter for () {
    const DANGLING: Self = ();

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
    unsafe fn next_archetype(&mut self, _archetype: &Archetype) {}

    #[inline]
    unsafe fn matches_entity(&self, _offset: usize) -> bool {
        true
    }
}

pub struct Or<T>(pub T);

/// Query transformer that retrieves components of type `T` that have been mutated since the start of the frame.
/// Added components do not count as mutated.
pub struct Mutated<T>(NonNull<ComponentFlags>, PhantomData<T>);

/// Query transformer that retrieves components of type `T` that have been added since the start of the frame.
pub struct Added<T>(NonNull<ComponentFlags>, PhantomData<T>);

/// Query transformer that retrieves components of type `T` that have either been mutated or added since the start of the frame.
pub struct Changed<T>(NonNull<ComponentFlags>, PhantomData<T>);

// impl<T: Component> QueryFilter for Added<T> {
//     // fn access() -> QueryAccess {
//     //     QueryAccess::read::<T>()
//     // }

//     #[inline]
//     fn get_entity_filter(archetype: &Archetype) -> Option<Self::EntityFilter> {
//         todo!()
//         // archetype
//         //     .get_type_state(TypeId::of::<T>())
//         //     .map(|state| Added(state.component_flags(), Default::default()))
//     }
// }

// impl<T: Component> EntityFilter for Added<T> {
//     const DANGLING: Self = Added(NonNull::dangling(), PhantomData::<T>);

//     #[inline]
//     unsafe fn matches_entity(&self, offset: usize) -> bool {
//         (*self.0.as_ptr().add(offset)).contains(ComponentFlags::ADDED)
//     }
// }

// impl<T: Component> QueryFilter for Mutated<T> {
//     type EntityFilter = Self;

//     // fn access() -> QueryAccess {
//     //     QueryAccess::read::<T>()
//     // }

//     #[inline]
//     fn get_entity_filter(archetype: &Archetype) -> Option<Self::EntityFilter> {
//         todo!()
//         // archetype
//         //     .get_type_state(TypeId::of::<T>())
//         //     .map(|state| Mutated(state.component_flags(), Default::default()))
//     }
// }

// impl<T: Component> EntityFilter for Mutated<T> {
//     const DANGLING: Self = Mutated(NonNull::dangling(), PhantomData::<T>);

//     unsafe fn matches_entity(&self, offset: usize) -> bool {
//         (*self.0.as_ptr().add(offset)).contains(ComponentFlags::MUTATED)
//     }
// }

// impl<T: Component> QueryFilter for Changed<T> {
//     type EntityFilter = Self;

//     // fn access() -> QueryAccess {
//     //     QueryAccess::read::<T>()
//     // }

//     #[inline]
//     fn get_entity_filter(archetype: &Archetype) -> Option<Self::EntityFilter> {
//         todo!()
//         // archetype
//         //     .get_type_state(TypeId::of::<T>())
//         //     .map(|state| Changed(state.component_flags(), Default::default()))
//     }
// }

// impl<T: Component> EntityFilter for Changed<T> {
//     const DANGLING: Self = Changed(NonNull::dangling(), PhantomData::<T>);

//     #[inline]
//     unsafe fn matches_entity(&self, offset: usize) -> bool {
//         let flags = *self.0.as_ptr().add(offset);
//         flags.contains(ComponentFlags::ADDED) || flags.contains(ComponentFlags::MUTATED)
//     }
// }

pub struct With<T> {
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<T: Component> QueryFilter for With<T> {
    const DANGLING: Self = Self {
        component_id: ComponentId::new(usize::MAX),
        marker: PhantomData,
    };

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
    unsafe fn next_archetype(&mut self, _archetype: &Archetype) {}

    #[inline]
    unsafe fn matches_entity(&self, _offset: usize) -> bool {
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
    unsafe fn next_archetype(&mut self, _archetype: &Archetype) {}

    #[inline]
    unsafe fn matches_entity(&self, _offset: usize) -> bool {
        true
    }
}

pub struct WithType<T: Bundle> {
    bundle_info: *const BundleInfo,
    marker: PhantomData<T>,
}

impl<T: Bundle> QueryFilter for WithType<T> {
    const DANGLING: Self = Self {
        bundle_info: ptr::null::<BundleInfo>(),
        marker: PhantomData,
    };

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
    unsafe fn next_archetype(&mut self, _archetype: &Archetype) {}

    #[inline]
    unsafe fn matches_entity(&self, _offset: usize) -> bool {
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

            #[inline]
            unsafe fn matches_archetype(&self, archetype: &Archetype) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.matches_archetype(archetype))*
            }

            #[inline]
            unsafe fn next_table(&mut self, table: &Table) {}

            #[inline]
            unsafe fn next_archetype(&mut self, archetype: &Archetype) {}

            #[inline]
            unsafe fn matches_entity(&self, offset: usize) -> bool {
                let ($($filter,)*) = self;
                true $(&& $filter.matches_entity(offset))*
            }
        }
    };
}

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
