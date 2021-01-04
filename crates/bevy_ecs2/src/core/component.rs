use crate::TypeInfo;
use bevy_utils::HashMap;
use bitflags::bitflags;
use std::{alloc::Layout, any::TypeId, collections::hash_map::Entry};
use thiserror::Error;

/// Types that can be components, implemented automatically for all `Send + Sync + 'static` types
///
/// This is just a convenient shorthand for `Send + Sync + 'static`, and never needs to be
/// implemented manually.
pub trait Component: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Component for T {}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum StorageType {
    Archetype,
    SparseSet,
}

#[derive(Debug)]
pub struct ComponentInfo {
    pub type_id: TypeId,
    pub layout: Layout,
    pub drop: unsafe fn(*mut u8),
    pub storage_type: StorageType,
    pub id: ComponentId,
}

// TODO: needed?
// TODO: consider how this relates to DynamicComponents
#[derive(Debug, Copy, Clone)]
pub struct ComponentId(pub(crate) usize);

pub struct ComponentDescriptor {
    pub storage_type: StorageType,
    pub type_id: TypeId,
    pub layout: Layout,
    pub drop: unsafe fn(*mut u8),
}

impl ComponentDescriptor {
    pub fn of<T: Component>() -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }
        Self {
            storage_type: StorageType::Archetype,
            type_id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop: drop_ptr::<T>,
        }
    }
}

#[derive(Debug, Default)]
pub struct Components {
    components: Vec<ComponentInfo>,
    indices: HashMap<TypeId, usize>,
}

#[derive(Debug, Error)]
pub enum ComponentsError {
    #[error("A component of type {0:?} already exists")]
    ComponentAlreadyExists(TypeId),
}

impl Components {
    pub fn add(&mut self, descriptor: ComponentDescriptor) -> Result<ComponentId, ComponentsError> {
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

    pub fn get_info(&self, id: ComponentId) -> Option<&ComponentInfo> {
        self.components.get(id.0)
    }

    pub fn get_id(&self, type_id: TypeId) -> Option<ComponentId> {
        self.indices.get(&type_id).map(|index| ComponentId(*index))
    }

    pub fn init_type_info(&mut self, type_info: &TypeInfo) -> ComponentId {
        let components = &mut self.components;
        let index = self.indices.entry(type_info.id()).or_insert_with(|| {
            let index = components.len();
            components.push(ComponentInfo {
                storage_type: StorageType::Archetype,
                type_id: type_info.id(),
                id: ComponentId(index),
                drop: type_info.drop(),
                layout: type_info.layout(),
            });

            index
        });

        ComponentId(*index)
    }
}

bitflags! {
    pub struct ComponentFlags: u8 {
        const ADDED = 1;
        const MUTATED = 2;
    }
}
