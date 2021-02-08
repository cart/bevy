use crate::core::{SparseSetIndex, TypeInfo};
use bitflags::bitflags;
use std::{alloc::Layout, any::TypeId, collections::hash_map::Entry};
use thiserror::Error;

pub trait Component: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Component for T {}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum StorageType {
    Table,
    SparseSet,
}

#[derive(Debug)]
pub struct ComponentInfo {
    id: ComponentId,
    type_id: TypeId,
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
}

pub struct ComponentDescriptor {
    pub storage_type: StorageType,
    pub type_id: TypeId,
    pub layout: Layout,
    pub drop: unsafe fn(*mut u8),
}

impl ComponentDescriptor {
    pub fn of<T: Component>(storage_type: StorageType) -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }
        Self {
            storage_type,
            type_id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop: drop_ptr::<T>,
        }
    }
}

#[derive(Debug, Default)]
pub struct Components {
    components: Vec<ComponentInfo>,
    indices: std::collections::HashMap<TypeId, usize, fxhash::FxBuildHasher>,
}

#[derive(Debug, Error)]
pub enum ComponentsError {
    #[error("A component of type {0:?} already exists")]
    ComponentAlreadyExists(TypeId),
}

impl Components {
    pub(crate) fn add(&mut self, descriptor: &ComponentDescriptor) -> Result<ComponentId, ComponentsError> {
        let index_entry = self.indices.entry(descriptor.type_id);
        if let Entry::Occupied(_) = index_entry {
            return Err(ComponentsError::ComponentAlreadyExists(descriptor.type_id));
        }
        let index = self.components.len();
        self.components.push(ComponentInfo {
            storage_type: descriptor.storage_type,
            type_id: descriptor.type_id,
            id: ComponentId(index),
            drop: descriptor.drop,
            layout: descriptor.layout,
        });
        self.indices.insert(descriptor.type_id, index);

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

    pub fn get_with_type_info(&mut self, type_info: &TypeInfo) -> ComponentId {
        let components = &mut self.components;
        let index = self.indices.entry(type_info.id()).or_insert_with(|| {
            let index = components.len();
            components.push(ComponentInfo {
                storage_type: StorageType::Table,
                type_id: type_info.id(),
                id: ComponentId(index),
                drop: type_info.drop(),
                layout: type_info.layout(),
            });

            index
        });

        ComponentId(*index)
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
