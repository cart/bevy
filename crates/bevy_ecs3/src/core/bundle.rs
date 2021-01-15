use crate::{core::{Component, ComponentId, Components, SparseSetIndex, TypeInfo}, smaller_tuples_too};
use std::{any::TypeId, collections::HashMap, ptr::NonNull};

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
    unsafe fn get(func: impl FnMut() -> Option<NonNull<u8>>) -> Option<Self>
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
            unsafe fn get(mut func: impl FnMut() -> Option<NonNull<u8>>) -> Option<Self> {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = (
                    $(func()?.as_ptr().cast::<$name>(),)*
                );
                Some(($($name.read(),)*))
            }
        }
    }
}

smaller_tuples_too!(tuple_impl, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);

#[derive(Debug, Clone, Copy)]
pub struct BundleId(usize);

impl BundleId {
    #[inline]
    pub fn index(&self) -> usize {
        self.0
    }
}

impl SparseSetIndex for BundleId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.index()
    }
}

pub struct BundleInfo {
    pub(crate) id: BundleId,
    pub(crate) component_ids: Vec<ComponentId>,
}

#[derive(Default)]
pub(crate) struct Bundles {
    bundle_infos: Vec<BundleInfo>,
    bundle_ids: HashMap<TypeId, BundleId>,
}

impl Bundles {
    pub(crate) fn get_info_dynamic<T: DynamicBundle>(
        &mut self,
        components: &mut Components,
        bundle: &T,
    ) -> &BundleInfo {
        let mut bundle_infos = &mut self.bundle_infos;
        let id = self.bundle_ids.entry(TypeId::of::<T>()).or_insert_with(|| {
            let type_info = bundle.type_info();
            let id = BundleId(bundle_infos.len());
            let bundle_info = initialize_bundle(&type_info, id, components);
            bundle_infos.push(bundle_info);
            id
        });
        // SAFE: index either exists, or was initialized
        unsafe { self.bundle_infos.get_unchecked(id.0) }
    }

    pub(crate) fn get_info<T: Bundle>(&mut self, components: &mut Components) -> &BundleInfo {
        let mut bundle_infos = &mut self.bundle_infos;
        let id = self.bundle_ids.entry(TypeId::of::<T>()).or_insert_with(|| {
            let type_info = T::static_type_info();
            let id = BundleId(bundle_infos.len());
            let bundle_info = initialize_bundle(&type_info, id, components);
            bundle_infos.push(bundle_info);
            id
        });
        // SAFE: index either exists, or was initialized
        unsafe { self.bundle_infos.get_unchecked(id.0) }
    }
}

fn initialize_bundle(
    type_info: &[TypeInfo],
    id: BundleId,
    components: &mut Components,
) -> BundleInfo {
    let mut component_ids = Vec::new();

    for type_info in type_info {
        let component_id = components.get_with_type_info(&type_info);
        component_ids.push(component_id);
    }

    BundleInfo { id, component_ids }
}
