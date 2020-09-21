use crate::{alloc::vec::Vec, Entity, Location};
use bevy_utils::{HashMap, HashMapExt};
use std::{any::TypeId, fmt::Debug};

use crate::{borrow::AtomicBorrow, query::Fetch, Access, Component, Query};

struct VecComponentStorage<T> {
    storage: Vec<T>,
    meta: ComponentStorageMeta,
}

impl<T> Default for VecComponentStorage<T> {
    fn default() -> Self {
        Self {
            storage: Vec::default(),
            meta: ComponentStorageMeta::default(),
        }
    }
}

impl<T> ComponentStorage for VecComponentStorage<T>
where
    T: 'static,
{
    fn meta(&self) -> &ComponentStorageMeta {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut ComponentStorageMeta {
        &mut self.meta
    }

    fn get_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn get_pointer(&self) -> *const u8 {
        self.storage.as_ptr().cast::<u8>()
    }

    fn get_value(&self, index: usize) -> *const u8 {
        unsafe { self.storage.get_unchecked(index) as *const T as *const u8 }
    }

    fn insert(&mut self, value: *const u8) {
        assert!(self.storage.len() + 1 <= self.storage.capacity());
        unsafe {
            let index = self.storage.len();
            self.storage.set_len(index + 1);
            println!("{} {}", self.storage.len(), self.storage.capacity());
            std::ptr::copy_nonoverlapping(
                value.cast::<T>(),
                self.storage.as_mut_ptr().add(index),
                std::mem::size_of::<T>(),
            )
        }
    }

    fn reserve(&mut self, size: usize) {
        self.storage.reserve(size);
    }

    unsafe fn swap_remove(&mut self, index: usize, forget: bool) {
        let value = self.storage.swap_remove(index);
        if forget {
            std::mem::forget(value);
        }
    }

    fn clear(&mut self) {
        self.storage.clear()
    }

    fn len(&self) -> usize {
        self.storage.len()
    }

    fn capacity(&self) -> usize {
        self.storage.capacity()
    }
}

pub trait ComponentStorage {
    fn meta(&self) -> &ComponentStorageMeta;
    fn meta_mut(&mut self) -> &mut ComponentStorageMeta;
    fn get_type(&self) -> TypeId;
    fn get_pointer(&self) -> *const u8;
    fn get_value(&self, index: usize) -> *const u8;
    fn reserve(&mut self, size: usize);
    fn insert(&mut self, value: *const u8);
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
    unsafe fn swap_remove(&mut self, index: usize, forget: bool);
    fn clear(&mut self);
}

/// A collection of entities having the same component types
///
/// Accessing `Archetype`s is only required for complex dynamic scheduling. To manipulate entities,
/// go through the `World`.
pub struct Archetype {
    pub type_info: Vec<TypeInfo>,
    pub entities: Vec<Entity>,
    component_storages: Vec<Box<dyn ComponentStorage>>,
    pub type_indices: HashMap<TypeId, usize>,
    grow_size: usize,
}

impl Archetype {
    #[allow(missing_docs)]
    pub fn new(type_info: Vec<TypeInfo>) -> Self {
        Self::with_grow(type_info, 64)
    }

    #[allow(missing_docs)]
    pub fn with_grow(type_info: Vec<TypeInfo>, grow_size: usize) -> Self {
        // TODO: check for dupes
        let mut component_storages = Vec::with_capacity(type_info.len());
        let mut type_indices = HashMap::with_capacity(type_info.len());
        for ty in &type_info {
            type_indices.insert(ty.id, component_storages.len());
            component_storages.push((ty.get_storage)());
        }

        Self {
            type_info,
            entities: Vec::new(),
            component_storages,
            type_indices,
            grow_size,
        }
    }

    pub(crate) fn clear(&mut self) {
        for storage in self.component_storages.iter_mut() {
            storage.clear()
        }
        self.entities.clear();
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn has<T: Component>(&self) -> bool {
        self.has_type(TypeId::of::<T>())
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn has_type(&self, ty: TypeId) -> bool {
        self.type_indices.contains_key(&ty)
    }

    #[inline]
    fn type_index<T: Component>(&self) -> Option<usize> {
        self.type_index_dynamic(TypeId::of::<T>())
    }

    #[inline]
    fn type_index_dynamic(&self, type_id: TypeId) -> Option<usize> {
        self.type_indices.get(&type_id).cloned()
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn get_storage<T: Component>(&self) -> Option<&dyn ComponentStorage> {
        self.type_index::<T>()
            .map(|index| &*self.component_storages[index])
    }

    // TODO: rename for parity
    #[allow(missing_docs)]
    #[inline]
    pub fn get_storage_dynamic(&self, type_id: TypeId) -> Option<&Box<dyn ComponentStorage>> {
        self.type_index_dynamic(type_id)
            .map(|index| &self.component_storages[index])
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn get_storage_dynamic_mut(
        &mut self,
        type_id: TypeId,
    ) -> Option<&mut Box<dyn ComponentStorage>> {
        self.type_index_dynamic(type_id)
            .map(move |index| &mut self.component_storages[index])
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    #[allow(missing_docs)]
    pub fn iter_entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter()
    }

    pub(crate) fn get_entity(&self, index: usize) -> Entity {
        self.entities[index]
    }

    #[allow(missing_docs)]
    pub fn allocate(&mut self, entity: Entity) -> usize {
        if self.len() == self.entities.capacity() {
            self.reserve(self.grow_size);
        }

        self.entities.push(entity);
        println!("e {} {}", self.entities.len(), self.entities.capacity());
        for storage in self.component_storages.iter_mut() {
            println!(
                "s {:?} {} {}",
                storage.get_type(),
                storage.len(),
                storage.capacity()
            );
            storage.meta_mut().allocate();
        }

        self.len() - 1
    }

    fn capacity(&self) -> usize {
        self.entities.len()
    }

    #[allow(missing_docs)]
    pub fn clear_trackers(&mut self) {
        for storage in self.component_storages.iter_mut() {
            storage.meta_mut().clear_trackers();
        }
    }

    pub fn reserve(&mut self, count: usize) {
        self.entities.reserve(count);

        for storage in self.component_storages.iter_mut() {
            storage.reserve(count);
            storage.meta_mut().reserve(count);
        }
    }

    /// Returns the ID of the entity moved into `index`, if any
    pub(crate) fn remove(&mut self, index: usize) -> Option<Entity> {
        if index >= self.len() {
            panic!("entity index in archetype is out of bounds");
        }
        for storage in self.component_storages.iter_mut() {
            unsafe {
                storage.swap_remove(index, false);
            }
            let storage_meta = storage.meta_mut();
            storage_meta.added_entities.swap_remove(index);
            storage_meta.mutated_entities.swap_remove(index);
        }

        if self.entities.len() - 1 == index {
            self.entities.pop();
            None
        } else {
            self.entities.swap_remove(index);
            Some(self.entities[index])
        }
    }

    #[allow(missing_docs)]
    pub unsafe fn move_to<'a, 'b>(
        &'a mut self,
        location: &'b mut Location,
        archetype: &'b mut Archetype,
        drop_unused: bool,
    ) -> Option<Entity> {
        if location.index >= self.len() {
            panic!("entity index in archetype is out of bounds");
        }

        let target_index = archetype.allocate(self.entities[location.index]);
        let old_index = std::mem::replace(&mut location.index, target_index);
        for storage in self.component_storages.iter_mut() {
            if let Some(target_storage) = archetype.get_storage_dynamic_mut(storage.get_type()) {
                let value = storage.get_value(old_index);
                target_storage.insert(value);
                // forget the removed component because we copied it to the other archetype storage
                storage.swap_remove(old_index, true);
            } else {
                storage.swap_remove(old_index, !drop_unused);
            }
        }

        if self.entities.len() - 1 == old_index {
            self.entities.pop();
            None
        } else {
            self.entities.swap_remove(old_index);
            Some(self.entities[old_index])
        }
    }

    pub unsafe fn get_value<T: Component>(&self, index: usize) -> Option<T> {
        self.get_storage::<T>()
            .map(|storage| storage.get_value(index).cast::<T>().read())
    }

    pub fn insert<T: Component>(&mut self, value: T) {
        self.insert_dynamic(TypeId::of::<T>(), &value as *const T as *const u8);
        std::mem::forget(value);
    }

    fn insert_dynamic(&mut self, type_id: TypeId, value: *const u8) {
        self.get_storage_dynamic_mut(type_id).unwrap().insert(value);
    }

    /// How, if at all, `Q` will access entities in this archetype
    pub fn access<Q: Query>(&self) -> Option<Access> {
        Q::Fetch::access(self)
    }
}

#[allow(missing_docs)]
pub struct ComponentStorageMeta {
    pub borrow: AtomicBorrow,
    pub mutated_entities: Vec<bool>,
    pub added_entities: Vec<bool>,
}

impl Default for ComponentStorageMeta {
    fn default() -> Self {
        Self {
            borrow: AtomicBorrow::new(),
            mutated_entities: Vec::new(),
            added_entities: Vec::new(),
        }
    }
}

impl ComponentStorageMeta {
    #[allow(missing_docs)]
    pub fn clear_trackers(&mut self) {
        for mutated in self.mutated_entities.iter_mut() {
            *mutated = false;
        }

        for added in self.added_entities.iter_mut() {
            *added = false;
        }
    }

    fn reserve(&mut self, count: usize) {
        self.added_entities.reserve(count);
        self.mutated_entities.reserve(count);
    }

    pub fn allocate(&mut self) {
        self.added_entities.push(false);
        self.mutated_entities.push(false);
    }

    pub fn borrow(&self) {
        self.borrow.borrow();
    }

    pub fn borrow_mut(&self) {
        self.borrow.borrow_mut();
    }

    pub fn release(&self) {
        self.borrow.release();
    }

    pub fn release_mut(&self) {
        self.borrow.release_mut();
    }
}

#[derive(Clone, Debug)]
pub struct TypeInfo {
    id: TypeId,
    get_storage: fn() -> Box<dyn ComponentStorage>,
}

impl PartialEq for TypeInfo {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl TypeInfo {
    pub fn of<T: Component>() -> Self {
        TypeInfo {
            id: TypeId::of::<T>(),
            get_storage: || Box::new(VecComponentStorage::<T>::default()),
        }
    }

    #[inline]
    pub fn id(&self) -> TypeId {
        self.id
    }
}
