use std::{any::TypeId, collections::HashMap, ptr::NonNull};

use crate::{
    core::{
        ArchetypeId, Archetypes, Component, ComponentError, ComponentId, Components, SparseSets,
        StorageType, Tables, TypeInfo,
    },
    smaller_tuples_too,
};

/// A dynamically typed ordered collection of components
///
/// See [Bundle]
pub trait DynamicBundle: 'static {
    /// Gets this [DynamicBundle]'s components type info, in the order of this bundle's Components
    fn type_info(&self) -> Vec<TypeInfo>;

    /// Calls `func` on each value, in the order of this bundle's Components
    #[doc(hidden)]
    unsafe fn put(self, func: impl FnMut(*mut u8));
}
/// A statically typed ordered collection of components
///
/// See [DynamicBundle]
pub trait Bundle: DynamicBundle {
    /// Gets this [Bundle]'s components type info, in the order of this bundle's Components
    fn static_type_info() -> Vec<TypeInfo>;

    /// Calls `func`, which should return data for each component in the bundle, in the order of this bundle's Components
    unsafe fn get(func: impl FnMut() -> Option<NonNull<u8>>) -> Result<Self, ComponentError>
    where
        Self: Sized;
}

macro_rules! tuple_impl {
    ($($name: ident),*) => {
        impl<$($name: Component),*> DynamicBundle for ($($name,)*) {
            fn type_info(&self) -> Vec<TypeInfo> {
                Self::static_type_info()
            }

            #[allow(unused_variables, unused_mut)]
            unsafe fn put(self, mut func: impl FnMut(*mut u8)) {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = self;
                $(
                    func((&mut $name as *mut $name).cast::<u8>());
                )*
            }
        }

        impl<$($name: Component),*> Bundle for ($($name,)*) {
            fn static_type_info() -> Vec<TypeInfo> {
                vec![$(TypeInfo::of::<$name>()),*]
            }

            #[allow(unused_variables, unused_mut)]
            unsafe fn get(mut func: impl FnMut() -> Option<NonNull<u8>>) -> Result<Self, ComponentError> {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = ($(
                    func().ok_or_else(ComponentError::missing_component::<$name>)?
                        .as_ptr()
                        .cast::<$name>(),)*
                );
                Ok(($($name.read(),)*))
            }
        }
    }
}

smaller_tuples_too!(tuple_impl, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);