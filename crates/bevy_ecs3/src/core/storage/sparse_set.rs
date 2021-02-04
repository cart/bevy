use crate::core::{BlobVec, ComponentFlags, ComponentId, ComponentInfo, Entity};
use std::{cell::UnsafeCell, marker::PhantomData};

#[derive(Debug)]
pub struct SparseArray<I, V = I> {
    values: Vec<Option<V>>,
    marker: PhantomData<I>,
}

impl<I, V> Default for SparseArray<I, V> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            marker: Default::default(),
        }
    }
}

impl<I: SparseSetIndex, V> SparseArray<I, V> {
    #[inline]
    pub fn insert(&mut self, index: I, value: V) {
        let index = index.sparse_set_index();
        if index >= self.values.len() {
            self.values.resize_with(index + 1, || None);
        }
        self.values[index] = Some(value);
    }

    #[inline]
    pub fn contains(&self, index: I) -> bool {
        let index = index.sparse_set_index();
        self.values.get(index).map(|v| v.is_some()).unwrap_or(false)
    }

    #[inline]
    pub fn get(&self, index: I) -> Option<&V> {
        let index = index.sparse_set_index();
        self.values.get(index).map(|v| v.as_ref()).unwrap_or(None)
    }

    /// SAFETY: index must exist in the set
    #[inline]
    pub unsafe fn get_unchecked(&self, index: I) -> &V {
        let index = index.sparse_set_index();
        self.values.get_unchecked(index).as_ref().unwrap()
    }

    /// SAFETY: index must exist in the set
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: I) -> &mut V {
        let index = index.sparse_set_index();
        self.values.get_unchecked_mut(index).as_mut().unwrap()
    }

    /// SAFETY: index must exist in the set
    #[inline]
    pub unsafe fn remove_unchecked(&mut self, index: I) -> Option<V> {
        let index = index.sparse_set_index();
        self.values.get_unchecked_mut(index).take()
    }

    #[inline]
    pub fn remove(&mut self, index: I) -> Option<V> {
        let index = index.sparse_set_index();
        self.values.get_mut(index).and_then(|value| value.take())
    }

    #[inline]
    pub fn get_or_insert_with(&mut self, index: I, func: impl FnOnce() -> V) -> &mut V {
        let index = index.sparse_set_index();
        if index < self.values.len() {
            // SAFE: just checked bounds
            let value = unsafe { self.values.get_unchecked_mut(index) };
            if value.is_none() {
                *value = Some(func());
            }

            return value.as_mut().unwrap();
        }
        self.values.resize_with(index + 1, || None);
        // SAFE: just inserted
        unsafe {
            let value = self.values.get_unchecked_mut(index);
            *value = Some(func());
            value.as_mut().unwrap()
        }
    }
}

#[derive(Debug)]
pub struct ComponentSparseSet {
    dense: BlobVec,
    flags: UnsafeCell<Vec<ComponentFlags>>,
    entities: Vec<Entity>,
    sparse: SparseArray<Entity, usize>,
}

impl ComponentSparseSet {
    pub fn new(component_info: &ComponentInfo, capacity: usize) -> Self {
        Self {
            dense: BlobVec::new(component_info.layout(), component_info.drop(), capacity),
            flags: UnsafeCell::new(Vec::with_capacity(capacity)),
            entities: Vec::with_capacity(capacity),
            sparse: Default::default(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// SAFETY: The `value` pointer must point to a valid address that matches the internal BlobVec's Layout.
    /// Caller is responsible for ensuring the value is not dropped. This collection will drop the value when needed.
    pub unsafe fn insert(&mut self, entity: Entity, value: *mut u8, flags: ComponentFlags) {
        let dense = &mut self.dense;
        let entities = &mut self.entities;
        let flag_list = &mut *self.flags.get();
        let dense_index = *self.sparse.get_or_insert_with(entity, move || {
            flag_list.push(ComponentFlags::empty());
            entities.push(entity);
            dense.push_uninit();
            dense.len() - 1
        });
        // SAFE: dense_index exists thanks to the call above
        self.dense.set_unchecked(dense_index, value);
        (*self.flags.get())
            .get_unchecked_mut(dense_index)
            .insert(flags);
    }

    #[inline]
    pub fn contains(&self, entity: Entity) -> bool {
        self.sparse.contains(entity)
    }

    /// SAFETY: ensure the same entity is not accessed twice at the same time
    #[inline]
    pub fn get(&self, entity: Entity) -> Option<*mut u8> {
        self.sparse.get(entity).map(|dense_index| {
            // SAFE: if the sparse index points to something in the dense vec, it exists
            unsafe { self.dense.get_unchecked(*dense_index) }
        })
    }

    /// SAFETY: ensure the same entity is not accessed twice at the same time
    #[inline]
    pub unsafe fn get_with_flags(&self, entity: Entity) -> Option<(*mut u8, *mut ComponentFlags)> {
        let flags = &mut *self.flags.get();
        self.sparse.get(entity).map(move |dense_index| {
            let dense_index = *dense_index;
            // SAFE: if the sparse index points to something in the dense vec, it exists
            (
                self.dense.get_unchecked(dense_index),
                flags.get_unchecked_mut(dense_index) as *mut ComponentFlags,
            )
        })
    }

    /// SAFETY: ensure the same entity is not accessed twice at the same time
    #[inline]
    pub unsafe fn get_flags(&self, entity: Entity) -> Option<&mut ComponentFlags> {
        let flags = &mut *self.flags.get();
        self.sparse.get(entity).map(move |dense_index| {
            let dense_index = *dense_index;
            // SAFE: if the sparse index points to something in the dense vec, it exists
            flags.get_unchecked_mut(dense_index)
        })
    }

    /// SAFETY: `entity` must have a value stored in the set
    /// ensure the same entity is not accessed twice at the same time
    #[inline]
    pub unsafe fn get_unchecked(&self, entity: Entity) -> *mut u8 {
        let dense_index = self.sparse.get_unchecked(entity);
        self.dense.get_unchecked(*dense_index)
    }

    /// SAFETY: `entity` must have a value stored in the set
    /// ensure the same entity is not accessed twice at the same time
    #[inline]
    pub unsafe fn get_flags_unchecked(&self, entity: Entity) -> *mut ComponentFlags {
        let dense_index = self.sparse.get_unchecked(entity);
        (*self.flags.get()).as_mut_ptr().add(*dense_index)
    }

    /// SAFETY: `entity` must have a value stored in the set
    /// ensure the same entity is not accessed twice at the same time
    #[inline]
    pub unsafe fn get_with_flags_unchecked(
        &self,
        entity: Entity,
    ) -> (*mut u8, *mut ComponentFlags) {
        let dense_index = *self.sparse.get_unchecked(entity);
        (
            self.dense.get_unchecked(dense_index),
            (*self.flags.get()).as_mut_ptr().add(dense_index),
        )
    }

    /// SAFETY: it is the caller's responsibility to drop the returned ptr (if Some is returned).
    pub unsafe fn remove_and_forget(&mut self, entity: Entity) -> Option<*mut u8> {
        self.sparse.remove(entity).map(|dense_index| {
            (*self.flags.get()).swap_remove(dense_index);
            self.entities.swap_remove(dense_index);
            let is_last = dense_index == self.dense.len() - 1;
            let value = self.dense.swap_remove_and_forget_unchecked(dense_index);
            if !is_last {
                let swapped_entity = self.entities[dense_index];
                *self.sparse.get_unchecked_mut(swapped_entity) = dense_index;
            }
            value
        })
    }

    /// SAFETY: `entity` must have a value stored in the set
    pub fn remove(&mut self, entity: Entity) {
        if let Some(dense_index) = self.sparse.remove(entity) {
            // SAFE: unique access to self
            unsafe {
                (*self.flags.get()).swap_remove(dense_index);
            }
            self.entities.swap_remove(dense_index);
            let is_last = dense_index == self.dense.len() - 1;
            // SAFE: if the sparse index points to something in the dense vec, it exists
            unsafe { self.dense.swap_remove_unchecked(dense_index) }
            if !is_last {
                let swapped_entity = self.entities[dense_index];
                // SAFE: swapped entities must exist
                unsafe {
                    *self.sparse.get_unchecked_mut(swapped_entity) = dense_index;
                }
            }
        }
    }

    pub(crate) fn clear_flags(&mut self) {
        // SAFE: unique access to self
        let flags = unsafe { (*self.flags.get()).iter_mut() };
        for component_flags in flags {
            *component_flags = ComponentFlags::empty();
        }
    }
}

#[derive(Debug)]
pub struct SparseSet<I: SparseSetIndex, V: 'static> {
    dense: Vec<V>,
    indices: Vec<I>,
    sparse: SparseArray<I, usize>,
}

impl<I: SparseSetIndex, V> Default for SparseSet<I, V> {
    fn default() -> Self {
        Self::new(64)
    }
}

impl<I: SparseSetIndex, V> SparseSet<I, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            dense: Vec::with_capacity(capacity),
            indices: Vec::with_capacity(capacity),
            sparse: Default::default(),
        }
    }

    pub fn insert(&mut self, index: I, value: V) {
        if let Some(dense_index) = self.sparse.get(index.clone()).cloned() {
            unsafe {
                *self.dense.get_unchecked_mut(dense_index) = value;
            }
        } else {
            self.sparse.insert(index.clone(), self.dense.len());
            self.indices.push(index);
            self.dense.push(value);
        }

        // TODO: switch to this. it's faster but it has an invalid memory access on table_add_remove_many
        // let dense = &mut self.dense;
        // let indices = &mut self.indices;
        // let dense_index = *self.sparse.get_or_insert_with(index.clone(), move || {
        //     if dense.len() == dense.capacity() {
        //         dense.reserve(64);
        //         indices.reserve(64);
        //     }
        //     let len = dense.len();
        //     // SAFE: we set the index immediately
        //     unsafe {
        //         dense.set_len(len + 1);
        //         indices.set_len(len + 1);
        //     }
        //     len
        // });
        // // SAFE: index either already existed or was just allocated
        // unsafe {
        //     *self.dense.get_unchecked_mut(dense_index) = value;
        //     *self.indices.get_unchecked_mut(dense_index) = index;
        // }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    #[inline]
    pub fn contains(&self, index: I) -> bool {
        self.sparse.contains(index)
    }

    pub fn get(&self, index: I) -> Option<&V> {
        self.sparse.get(index).map(|dense_index| {
            // SAFE: if the sparse index points to something in the dense vec, it exists
            unsafe { self.dense.get_unchecked(*dense_index) }
        })
    }

    pub fn get_mut(&mut self, index: I) -> Option<&mut V> {
        let dense = &mut self.dense;
        self.sparse.get(index).map(move |dense_index| {
            // SAFE: if the sparse index points to something in the dense vec, it exists
            unsafe { dense.get_unchecked_mut(*dense_index) }
        })
    }

    /// SAFETY: `index` must have a value stored in the set
    #[inline]
    pub unsafe fn get_unchecked(&self, index: I) -> &V {
        let dense_index = self.sparse.get_unchecked(index);
        self.dense.get_unchecked(*dense_index)
    }

    /// SAFETY: `index` must have a value stored in the set and access must be unique
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: I) -> &mut V {
        let dense_index = self.sparse.get_unchecked(index);
        self.dense.get_unchecked_mut(*dense_index)
    }

    pub fn remove(&mut self, index: I) -> Option<V> {
        self.sparse.remove(index).map(|dense_index| {
            let is_last = dense_index == self.dense.len() - 1;
            let value = self.dense.swap_remove(dense_index);
            self.indices.swap_remove(dense_index);
            if !is_last {
                let swapped_index = self.indices[dense_index].clone();
                // SAFE: swapped entities must exist
                unsafe {
                    *self.sparse.get_unchecked_mut(swapped_index) = dense_index;
                }
            }
            value
        })
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.dense.iter()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.dense.iter_mut()
    }
}

pub trait SparseSetIndex: Clone {
    fn sparse_set_index(&self) -> usize;
}

impl SparseSetIndex for u8 {
    fn sparse_set_index(&self) -> usize {
        *self as usize
    }
}

impl SparseSetIndex for u16 {
    fn sparse_set_index(&self) -> usize {
        *self as usize
    }
}

impl SparseSetIndex for u32 {
    fn sparse_set_index(&self) -> usize {
        *self as usize
    }
}

impl SparseSetIndex for u64 {
    fn sparse_set_index(&self) -> usize {
        *self as usize
    }
}

impl SparseSetIndex for usize {
    fn sparse_set_index(&self) -> usize {
        *self
    }
}

#[derive(Default)]
pub struct SparseSets {
    sets: SparseSet<ComponentId, ComponentSparseSet>,
}

impl SparseSets {
    pub fn get_or_insert(&mut self, component_info: &ComponentInfo) -> &mut ComponentSparseSet {
        if !self.sets.contains(component_info.id()) {
            self.sets.insert(
                component_info.id(),
                ComponentSparseSet::new(component_info, 64),
            );
        }

        self.sets.get_mut(component_info.id()).unwrap()
    }

    pub fn get(&self, component_id: ComponentId) -> Option<&ComponentSparseSet> {
        self.sets.get(component_id)
    }

    pub unsafe fn get_unchecked(&self, component_id: ComponentId) -> &ComponentSparseSet {
        self.sets.get_unchecked(component_id)
    }

    pub fn get_mut(&mut self, component_id: ComponentId) -> Option<&mut ComponentSparseSet> {
        self.sets.get_mut(component_id)
    }

    #[inline]
    pub unsafe fn get_mut_unchecked(
        &mut self,
        component_id: ComponentId,
    ) -> &mut ComponentSparseSet {
        self.sets.get_unchecked_mut(component_id)
    }

    pub(crate) fn clear_flags(&mut self) {
        for set in self.sets.values_mut() {
            set.clear_flags();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{Entity, SparseSet};

    #[derive(Debug, Eq, PartialEq)]
    struct Foo(usize);

    #[test]
    fn sparse_set() {
        let mut set = SparseSet::<Entity, Foo>::default();
        let e0 = Entity::new(0);
        let e1 = Entity::new(1);
        let e2 = Entity::new(2);
        let e3 = Entity::new(3);
        let e4 = Entity::new(4);

        set.insert(e1, Foo(1));
        set.insert(e2, Foo(2));
        set.insert(e3, Foo(3));

        assert_eq!(set.get(e0), None);
        assert_eq!(set.get(e1), Some(&Foo(1)));
        assert_eq!(set.get(e2), Some(&Foo(2)));
        assert_eq!(set.get(e3), Some(&Foo(3)));
        assert_eq!(set.get(e4), None);

        {
            let iter_results = set.values().collect::<Vec<_>>();
            assert_eq!(iter_results, vec![&Foo(1), &Foo(2), &Foo(3)])
        }

        assert_eq!(set.remove(e2), Some(Foo(2)));
        assert_eq!(set.remove(e2), None);

        assert_eq!(set.get(e0), None);
        assert_eq!(set.get(e1), Some(&Foo(1)));
        assert_eq!(set.get(e2), None);
        assert_eq!(set.get(e3), Some(&Foo(3)));
        assert_eq!(set.get(e4), None);

        assert_eq!(set.remove(e1), Some(Foo(1)));

        assert_eq!(set.get(e0), None);
        assert_eq!(set.get(e1), None);
        assert_eq!(set.get(e2), None);
        assert_eq!(set.get(e3), Some(&Foo(3)));
        assert_eq!(set.get(e4), None);

        set.insert(e1, Foo(10));

        assert_eq!(set.get(e1), Some(&Foo(10)));

        *set.get_mut(e1).unwrap() = Foo(11);
        assert_eq!(set.get(e1), Some(&Foo(11)));
    }
}
