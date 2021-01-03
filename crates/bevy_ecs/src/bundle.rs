pub use bevy_ecs_macros::Bundle;

use crate::{
    component::{Component, ComponentFlags, ComponentId, Components, StorageType, TypeInfo},
    entity::Entity,
    storage::{SparseSetIndex, SparseSets, Table},
};
use bevy_ecs_macros::all_tuples;
use std::{any::TypeId, collections::HashMap};

/// A dynamically typed ordered collection of components
///
/// See [Bundle]
pub trait DynamicBundle: Send + Sync + 'static {
    /// Gets this [DynamicBundle]'s components type info, in the order of this bundle's Components
    fn type_info(&self) -> Vec<TypeInfo>;

    /// Calls `func` on each value, in the order of this bundle's Components. This will "mem::forget" the bundle
    /// fields, so callers are responsible for dropping the fields if that is desirable.
    fn put(self, func: impl FnMut(*mut u8));
}
/// A statically typed ordered collection of components
///
/// See [DynamicBundle]
pub trait Bundle: DynamicBundle {
    /// Gets this [Bundle]'s components type info, in the order of this bundle's Components
    fn static_type_info() -> Vec<TypeInfo>;

    /// Calls `func`, which should return data for each component in the bundle, in the order of this bundle's Components
    /// # Safety
    /// Caller must return data for each component in the bundle, in the order of this bundle's Components
    unsafe fn get(func: impl FnMut() -> *mut u8) -> Self
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
            fn put(self, mut func: impl FnMut(*mut u8)) {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = self;
                $(
                    func((&mut $name as *mut $name).cast::<u8>());
                    std::mem::forget($name);
                )*
            }
        }

        impl<$($name: Component),*> Bundle for ($($name,)*) {
            fn static_type_info() -> Vec<TypeInfo> {
                vec![$(TypeInfo::of::<$name>()),*]
            }

            #[allow(unused_variables, unused_mut)]
            unsafe fn get(mut func: impl FnMut() -> *mut u8) -> Self {
                #[allow(non_snake_case)]
                let ($(mut $name,)*) = (
                    $(func().cast::<$name>(),)*
                );
                ($($name.read(),)*)
            }
        }
    }
}

all_tuples!(tuple_impl, 0, 15, C);

#[derive(Debug, Clone, Copy)]
pub struct BundleId(usize);

impl BundleId {
    #[inline]
    pub fn index(self) -> usize {
        self.0
    }
}

impl SparseSetIndex for BundleId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.index()
    }

    fn get_sparse_set_index(value: usize) -> Self {
        Self(value)
    }
}

pub struct BundleInfo {
    pub(crate) id: BundleId,
    pub(crate) component_ids: Vec<ComponentId>,
    pub(crate) storage_types: Vec<StorageType>,
}

impl BundleInfo {
    /// # Safety
    /// table row must exist, entity must be valid
    #[inline]
    pub(crate) unsafe fn put_components<T: DynamicBundle>(
        &self,
        sparse_sets: &mut SparseSets,
        entity: Entity,
        table: &Table,
        table_row: usize,
        bundle_flags: &[ComponentFlags],
        bundle: T,
    ) {
        // NOTE: put is called on each component in "bundle order". bundle_info.component_ids are also in "bundle order"
        let mut bundle_component = 0;
        bundle.put(|component_ptr| {
            // SAFE: component_id was initialized by get_dynamic_bundle_info
            let component_id = *self.component_ids.get_unchecked(bundle_component);
            let flags = *bundle_flags.get_unchecked(bundle_component);
            match self.storage_types[bundle_component] {
                StorageType::Table => {
                    let column = table.get_column(component_id).unwrap();
                    column.set_unchecked(table_row, component_ptr);
                    column.get_flags_unchecked_mut(table_row).insert(flags);
                }
                StorageType::SparseSet => {
                    let sparse_set = sparse_sets.get_mut(component_id).unwrap();
                    sparse_set.insert(entity, component_ptr, flags);
                }
            }
            bundle_component += 1;
        });
    }

    #[inline]
    pub fn id(&self) -> BundleId {
        self.id
    }

    #[inline]
    pub fn components(&self) -> &[ComponentId] {
        &self.component_ids
    }

    #[inline]
    pub fn storage_types(&self) -> &[StorageType] {
        &self.storage_types
    }
}

#[derive(Default)]
pub struct Bundles {
    bundle_infos: Vec<BundleInfo>,
    bundle_ids: HashMap<TypeId, BundleId>,
}

impl Bundles {
    #[inline]
    pub fn get(&self, bundle_id: BundleId) -> Option<&BundleInfo> {
        self.bundle_infos.get(bundle_id.index())
    }

    #[inline]
    pub fn get_id(&self, type_id: TypeId) -> Option<BundleId> {
        self.bundle_ids.get(&type_id).cloned()
    }

    pub(crate) fn init_info_dynamic<T: DynamicBundle>(
        &mut self,
        components: &mut Components,
        bundle: &T,
    ) -> &BundleInfo {
        let bundle_infos = &mut self.bundle_infos;
        let id = self.bundle_ids.entry(TypeId::of::<T>()).or_insert_with(|| {
            let type_info = bundle.type_info();
            let id = BundleId(bundle_infos.len());
            let bundle_info =
                initialize_bundle(std::any::type_name::<T>(), &type_info, id, components);
            bundle_infos.push(bundle_info);
            id
        });
        // SAFE: index either exists, or was initialized
        unsafe { self.bundle_infos.get_unchecked(id.0) }
    }

    pub(crate) fn init_info<'a, T: Bundle>(
        &'a mut self,
        components: &mut Components,
    ) -> &'a BundleInfo {
        let bundle_infos = &mut self.bundle_infos;
        let id = self.bundle_ids.entry(TypeId::of::<T>()).or_insert_with(|| {
            let type_info = T::static_type_info();
            let id = BundleId(bundle_infos.len());
            let bundle_info =
                initialize_bundle(std::any::type_name::<T>(), &type_info, id, components);
            bundle_infos.push(bundle_info);
            id
        });
        // SAFE: index either exists, or was initialized
        unsafe { self.bundle_infos.get_unchecked(id.0) }
    }
}

fn initialize_bundle(
    bundle_type_name: &'static str,
    type_info: &[TypeInfo],
    id: BundleId,
    components: &mut Components,
) -> BundleInfo {
    let mut component_ids = Vec::new();
    let mut storage_types = Vec::new();

    for type_info in type_info {
        let component_id = components.get_or_insert_with(type_info.type_id(), || type_info.clone());
        // SAFE: get_with_type_info ensures info was created
        let info = unsafe { components.get_info_unchecked(component_id) };
        component_ids.push(component_id);
        storage_types.push(info.storage_type());
    }

    let mut deduped = component_ids.clone();
    deduped.sort();
    deduped.dedup();
    if deduped.len() != component_ids.len() {
        panic!("Bundle {} has duplicate components", bundle_type_name);
    }

    BundleInfo {
        id,
        component_ids,
        storage_types,
    }
}
