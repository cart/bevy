use crate::core::{SparseSetIndex, TypeInfo};
use bitflags::bitflags;
use std::{alloc::Layout, any::TypeId, collections::hash_map::Entry};
use thiserror::Error;

pub trait Component: 'static {}
impl<T: 'static> Component for T {}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum StorageType {
    Table,
    SparseSet,
}

impl Default for StorageType {
    fn default() -> Self {
        StorageType::Table
    }
}

#[derive(Debug)]
pub struct ComponentInfo {
    name: String,
    id: ComponentId,
    type_id: TypeId,
    // SAFETY: this must remain private. it should only be set to "true" if this component is actually Send + Sync
    is_send: bool,
    layout: Layout,
    drop: unsafe fn(*mut u8),
    storage_type: StorageType,
}

impl ComponentInfo {
    #[inline]
    pub fn id(&self) -> ComponentId {
        self.id
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    #[inline]
    pub fn drop(&self) -> unsafe fn(*mut u8) {
        self.drop
    }

    #[inline]
    pub fn storage_type(&self) -> StorageType {
        self.storage_type
    }

    fn new(id: ComponentId, descriptor: ComponentDescriptor) -> Self {
        ComponentInfo {
            id,
            name: descriptor.name,
            storage_type: descriptor.storage_type,
            type_id: descriptor.type_id,
            is_send: descriptor.is_send,
            drop: descriptor.drop,
            layout: descriptor.layout,
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub struct ComponentId(usize);

impl ComponentId {
    #[inline]
    pub const fn new(index: usize) -> ComponentId {
        ComponentId(index)
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0
    }
}

impl SparseSetIndex for ComponentId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.index()
    }

    fn get_sparse_set_index(value: usize) -> Self {
        Self(value)
    }
}

pub struct ComponentDescriptor {
    name: String,
    storage_type: StorageType,
    // SAFETY: this must remain private. it should only be set to "false" if this component is actually Send + Sync
    is_send: bool,
    type_id: TypeId,
    layout: Layout,
    drop: unsafe fn(*mut u8),
}

unsafe fn drop_ptr<T>(x: *mut u8) {
    x.cast::<T>().drop_in_place()
}

impl ComponentDescriptor {
    pub fn new<T: Component + Send + Sync>(storage_type: StorageType) -> Self {
        Self {
            is_send: true,
            ..Self::new_non_send::<T>(storage_type)
        }
    }

    pub fn new_non_send<T: Component>(storage_type: StorageType) -> Self {
        Self {
            name: std::any::type_name::<T>().to_string(),
            storage_type,
            is_send: false,
            type_id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop: drop_ptr::<T>,
        }
    }

    #[inline]
    pub fn storage_type(&self) -> StorageType {
        self.storage_type
    }

    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl From<TypeInfo> for ComponentDescriptor {
    fn from(type_info: TypeInfo) -> Self {
        Self {
            name: type_info.type_name().to_string(),
            storage_type: StorageType::default(),
            is_send: type_info.is_send(),
            type_id: type_info.type_id(),
            drop: type_info.drop(),
            layout: type_info.layout(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Components {
    components: Vec<ComponentInfo>,
    indices: std::collections::HashMap<TypeId, usize, fxhash::FxBuildHasher>,
    resource_indices: std::collections::HashMap<TypeId, usize, fxhash::FxBuildHasher>,
}

#[derive(Debug, Error)]
pub enum ComponentsError {
    #[error("A component of type {0:?} already exists")]
    ComponentAlreadyExists(TypeId),
}

impl Components {
    pub(crate) fn add(
        &mut self,
        descriptor: ComponentDescriptor,
    ) -> Result<ComponentId, ComponentsError> {
        let index_entry = self.indices.entry(descriptor.type_id);
        if let Entry::Occupied(_) = index_entry {
            return Err(ComponentsError::ComponentAlreadyExists(descriptor.type_id));
        }
        let index = self.components.len();
        self.indices.insert(descriptor.type_id(), index);
        self.components
            .push(ComponentInfo::new(ComponentId(index), descriptor.into()));

        Ok(ComponentId(index))
    }

    #[inline]
    pub fn get_info(&self, id: ComponentId) -> Option<&ComponentInfo> {
        self.components.get(id.0)
    }

    #[inline]
    pub unsafe fn get_info_unchecked(&self, id: ComponentId) -> &ComponentInfo {
        self.components.get_unchecked(id.0)
    }

    #[inline]
    pub fn get_id(&self, type_id: TypeId) -> Option<ComponentId> {
        self.indices.get(&type_id).map(|index| ComponentId(*index))
    }

    #[inline]
    pub fn get_resource_id(&self, type_id: TypeId) -> Option<ComponentId> {
        self.resource_indices
            .get(&type_id)
            .map(|index| ComponentId(*index))
    }

    #[inline]
    pub fn get_or_insert_resource_id<T: Component + Send + Sync>(&mut self) -> ComponentId {
        self.get_or_insert_resource_with(TypeId::of::<T>(), || TypeInfo::of::<T>())
    }

    #[inline]
    pub fn get_or_insert_non_send_resource_id<T: Component>(&mut self) -> ComponentId {
        self.get_or_insert_resource_with(TypeId::of::<T>(), || TypeInfo::of_non_send::<T>())
    }

    #[inline]
    fn get_or_insert_resource_with(
        &mut self,
        type_id: TypeId,
        func: impl FnOnce() -> TypeInfo,
    ) -> ComponentId {
        let components = &mut self.components;
        let index = self.resource_indices.entry(type_id).or_insert_with(|| {
            let type_info = func();
            let index = components.len();
            components.push(ComponentInfo::new(ComponentId(index), type_info.into()));
            index
        });

        ComponentId(*index)
    }

    #[inline]
    pub(crate) fn get_or_insert_with(
        &mut self,
        type_id: TypeId,
        func: impl FnOnce() -> TypeInfo,
    ) -> ComponentId {
        let components = &mut self.components;
        let index = self.indices.entry(type_id).or_insert_with(|| {
            let type_info = func();
            let index = components.len();
            components.push(ComponentInfo::new(ComponentId(index), type_info.into()));
            index
        });

        ComponentId(*index)
    }

    #[inline]
    pub fn get_or_insert_id<T: Component + Send + Sync>(&mut self) -> ComponentId {
        self.get_or_insert_with(TypeId::of::<T>(), || TypeInfo::of::<T>())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.components.len()
    }
}

bitflags! {
    pub struct ComponentFlags: u8 {
        const ADDED = 1;
        const MUTATED = 2;
    }
}
