// Copyright 2019 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// modified by Bevy contributors

use std::{
    any::{type_name, TypeId},
    fmt, mem,
    ptr::NonNull,
};

use crate::{core::{ComponentError, TypeInfo, Component}, smaller_tuples_too};

/// A dynamically typed collection of components
///
/// See [Bundle]
pub trait DynamicBundle: 'static {
    /// Obtain the fields' TypeInfos, in order that DynamicBundle::put will be called
    #[doc(hidden)]
    fn type_info(&self) -> Vec<TypeInfo>;
    /// Allow a callback to move all components out of the bundle
    ///
    /// Must invoke `f` only with a valid pointer, its type, and the pointee's size. A `false`
    /// return value indicates that the value was not moved and should be dropped.
    #[doc(hidden)]
    unsafe fn put(self, f: impl FnMut(*mut u8, TypeId, usize) -> bool);
}
/// A statically typed collection of components
///
/// See [DynamicBundle]
pub trait Bundle: DynamicBundle {
    /// Obtain the fields' TypeInfos, in order that DynamicBundle::get will be called
    #[doc(hidden)]
    fn static_type_info() -> Vec<TypeInfo>;

    /// Construct `Self` by moving components out of pointers fetched by `f`
    ///
    /// # Safety
    ///
    /// `f` must produce pointers to the expected fields. The implementation must not read from any
    /// pointers if any call to `f` returns `None`.
    #[doc(hidden)]
    unsafe fn get(
        f: impl FnMut(TypeId, usize) -> Option<NonNull<u8>>,
    ) -> Result<Self, ComponentError>
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
            unsafe fn put(self, mut f: impl FnMut(*mut u8, TypeId, usize) -> bool) {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = self;
                $(
                    if f(
                        (&mut $name as *mut $name).cast::<u8>(),
                        TypeId::of::<$name>(),
                        mem::size_of::<$name>()
                    ) {
                        mem::forget($name)
                    }
                )*
            }
        }

        impl<$($name: Component),*> Bundle for ($($name,)*) {
            fn static_type_info() -> Vec<TypeInfo> {
                vec![$(TypeInfo::of::<$name>()),*]
            }

            #[allow(unused_variables, unused_mut)]
            unsafe fn get(mut f: impl FnMut(TypeId, usize) -> Option<NonNull<u8>>) -> Result<Self, ComponentError> {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = ($(
                    f(TypeId::of::<$name>(), mem::size_of::<$name>()).ok_or_else(ComponentError::missing_component::<$name>)?
                        .as_ptr()
                        .cast::<$name>(),)*
                );
                Ok(($($name.read(),)*))
            }
        }
    }
}

smaller_tuples_too!(tuple_impl, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);
