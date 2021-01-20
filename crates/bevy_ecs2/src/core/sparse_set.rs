use super::blob_vec::BlobVec;
use crate::{ComponentFlags, ComponentId, Entity, TypeInfo};
use std::{alloc::Layout, cell::UnsafeCell, marker::PhantomData};

#[derive(Debug)]
pub struct BlobSparseSet<I> {
    dense: BlobVec,
    indices: Vec<I>,
    sparse: Vec<Option<usize>>,
}

impl<I: SparseSetIndex> BlobSparseSet<I> {
    pub fn new(item_layout: Layout, drop: unsafe fn(*mut u8), capacity: usize) -> Self {
        Self {
            dense: BlobVec::new(item_layout, drop, capacity),
            indices: Vec::with_capacity(capacity),
            sparse: Default::default(),
        }
    }

    pub fn new_typed<T>(capacity: usize) -> Self {
        Self {
            dense: BlobVec::new_typed::<T>(capacity),
            indices: Vec::with_capacity(capacity),
            sparse: Default::default(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// SAFETY: The `value` pointer must point to a valid address that matches the internal BlobVec's Layout.
    /// Caller is responsible for ensuring the value is not dropped. This collection will drop the value when needed.
    pub unsafe fn insert(&mut self, index: I, value: *mut u8) {
        let sparse_index = index.sparse_set_index();
        match self.sparse.get_mut(sparse_index) {
            // in bounds, and a value already exists
            Some(Some(dense_index)) => {
                self.dense.set_unchecked(*dense_index, value);
            }
            // in bounds, but no value
            Some(dense_index) => {
                *dense_index = Some(self.dense.len());
                self.indices.push(index);
                self.dense.push(value);
            }
            // out of bounds. resize to fit
            None => {
                self.sparse.resize(sparse_index + 1, None);
                self.sparse[sparse_index] = Some(self.dense.len());
                self.indices.push(index);
                self.dense.push(value);
            }
        }
    }

    pub fn contains(&self, index: I) -> bool {
        let sparse_index = index.sparse_set_index();
        if let Some(Some(_)) = self.sparse.get(sparse_index) {
            true
        } else {
            false
        }
    }

    pub fn get(&self, index: I) -> Option<*mut u8> {
        let sparse_index = index.sparse_set_index();
        match self.sparse.get(sparse_index) {
            // in bounds, and a value exists
            Some(Some(dense_index)) => Some(
                // SAFE: if the sparse index points to something, it exists
                unsafe { self.dense.get_unchecked(*dense_index) },
            ),
            // the value does not exist
            _ => None,
        }
    }

    /// SAFETY: `index` must have a value stored in the set
    pub unsafe fn get_unchecked(&self, index: I) -> *mut u8 {
        let sparse_index = index.sparse_set_index();
        let dense_index = self.sparse.get_unchecked(sparse_index).unwrap();
        self.dense.get_unchecked(dense_index)
    }

    /// SAFETY: it is the caller's responsibility to drop the returned ptr (if Some is returned).
    pub unsafe fn remove(&mut self, index: I) -> Option<*mut u8> {
        let sparse_index = index.sparse_set_index();
        if sparse_index >= self.sparse.len() {
            return None;
        }
        // SAFE: access to indices that exist. access is disjoint
        let sparse_ptr = self.sparse.as_mut_ptr();
        let sparse_value = &mut *sparse_ptr.add(sparse_index);
        if let Some(dense_index) = *sparse_value {
            let removed = self.dense.swap_remove_and_forget_unchecked(dense_index);
            self.indices.swap_remove(dense_index);
            *sparse_value = None;
            let moved_dense_value = sparse_ptr.add(self.indices[dense_index].sparse_set_index());
            *moved_dense_value = Some(dense_index);
            Some(removed)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct SparseSet<I: SparseSetIndex, V: 'static> {
    internal: BlobSparseSet<I>,
    marker: PhantomData<V>,
}

impl<I: SparseSetIndex, V> Default for SparseSet<I, V> {
    fn default() -> Self {
        Self::new(64)
    }
}

impl<I: SparseSetIndex, V> SparseSet<I, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            internal: BlobSparseSet::new_typed::<V>(capacity),
            marker: Default::default(),
        }
    }

    pub fn insert(&mut self, index: I, mut value: V) {
        // SAFE: self.internal's layout matches `value`. `value` is properly forgotten
        unsafe {
            self.internal
                .insert(index, (&mut value as *mut V).cast::<u8>());
        }
        std::mem::forget(value);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.internal.len()
    }

    pub fn contains(&self, index: I) -> bool {
        self.internal.contains(index)
    }

    pub fn get(&self, index: I) -> Option<&V> {
        // SAFE: value is of type V
        self.internal
            .get(index)
            .map(|value| unsafe { &*value.cast::<V>() })
    }

    pub fn get_mut(&mut self, index: I) -> Option<&mut V> {
        // SAFE: value is of type V
        self.internal
            .get(index)
            .map(|value| unsafe { &mut *value.cast::<V>() })
    }

    /// SAFETY: `index` must have a value stored in the set
    #[inline]
    pub unsafe fn get_unchecked(&self, index: I) -> &mut V {
        &mut *self.internal.get_unchecked(index).cast::<V>()
    }

    /// SAFETY: `index` must have a value stored in the set and access must be unique
    #[inline]
    pub unsafe fn get_unchecked_mut(&self, index: I) -> &mut V {
        &mut *self.internal.get_unchecked(index).cast::<V>()
    }

    pub fn remove(&mut self, index: I) -> Option<V> {
        // SAFE: value is V and is immediately read onto the stack
        unsafe {
            self.internal
                .remove(index)
                .map(|value| std::ptr::read(value.cast::<V>()))
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&I, &V)> {
        // SAFE: value is V
        unsafe {
            self.internal
                .indices
                .iter()
                .zip(self.internal.dense.iter_type::<V>())
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        unsafe { self.internal.dense.iter_type::<V>() }
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        unsafe { self.internal.dense.iter_type_mut::<V>() }
    }

    // pub fn iter_mut(&mut self) -> impl Iterator<Item = (&I, &mut V)> {
    //     self.indices.iter().zip(self.dense.iter_mut())
    // }
}

pub trait SparseSetIndex {
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

impl SparseSetIndex for Entity {
    fn sparse_set_index(&self) -> usize {
        self.id() as usize
    }
}

impl SparseSetIndex for ComponentId {
    fn sparse_set_index(&self) -> usize {
        self.0
    }
}

pub struct ComponentSparseSet {
    sparse_set: BlobSparseSet<Entity>,
    type_info: TypeInfo,
}

impl ComponentSparseSet {
    pub fn new(type_info: &TypeInfo) -> ComponentSparseSet {
        ComponentSparseSet {
            sparse_set: BlobSparseSet::new(type_info.layout(), type_info.drop(), 64),
            type_info: type_info.clone(),
        }
    }

    /// SAFETY: The caller must ensure that component_ptr is a pointer to the type described by TypeInfo
    /// The caller must also ensure that the component referenced by component_ptr is not dropped.
    /// This [SparseSetStorage] takes ownership of component_ptr and will drop it at the appropriate time.
    pub unsafe fn put_component(
        &mut self,
        entity: Entity,
        component_ptr: *mut u8,
        component_flags: ComponentFlags,
    ) {
        self.sparse_set.insert(entity, component_ptr);
    }

    pub fn get_component(&self, entity: Entity) -> Option<*mut u8> {
        self.sparse_set.get(entity)
    }

    /// SAFETY: it is the caller's responsibility to drop the returned ptr (if Some is returned).
    pub unsafe fn remove_component(&mut self, entity: Entity) -> Option<*mut u8> {
        self.sparse_set.remove(entity)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.sparse_set.len()
    }
}

#[derive(Default)]
pub struct SparseSets {
    sets: SparseSet<ComponentId, UnsafeCell<ComponentSparseSet>>,
}

impl SparseSets {
    pub fn get_or_insert(
        &mut self,
        component_id: ComponentId,
        type_info: &TypeInfo,
    ) -> &mut ComponentSparseSet {
        if !self.sets.contains(component_id) {
            self.sets.insert(
                component_id,
                UnsafeCell::new(ComponentSparseSet::new(type_info)),
            );
        }

        // SAFE: unique access to self
        unsafe { &mut *self.sets.get_mut(component_id).unwrap().get() }
    }

    pub fn get(&self, component_id: ComponentId) -> Option<&ComponentSparseSet> {
        // SAFE: read access to self
        unsafe { self.sets.get(component_id).map(|set| &*set.get()) }
    }

    pub unsafe fn get_unchecked(
        &self,
        component_id: ComponentId,
    ) -> Option<*mut ComponentSparseSet> {
        self.sets.get(component_id).map(|set| set.get())
    }

    pub fn get_mut(&mut self, component_id: ComponentId) -> Option<&mut ComponentSparseSet> {
        // SAFE: unique access to self
        unsafe { self.sets.get_mut(component_id).map(|set| &mut *set.get()) }
    }

    #[inline]
    pub unsafe fn get_mut_unchecked(
        &mut self,
        component_id: ComponentId,
    ) -> &mut ComponentSparseSet {
        // SAFE: unique access to self
        &mut *self.sets.get_unchecked_mut(component_id).get()
    }

    // PERF: this could be optimized if we stored SparseSet info in archetypes ( ping @Sander if @cart forgets :) )
    pub fn remove_entity(&mut self, entity: Entity) {
        for sparse_set in self.sets.values_mut() {
            unsafe {
                // SAFE: unique access to self
                let sparse_set = &mut *sparse_set.get();

                // SAFE: removed component is dropped
                if let Some(component) = sparse_set.remove_component(entity) {
                    (sparse_set.type_info.drop())(component);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Entity, SparseSet};

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
            let iter_results = set.iter().collect::<Vec<_>>();
            assert_eq!(
                iter_results,
                vec![(&e1, &Foo(1)), (&e2, &Foo(2)), (&e3, &Foo(3))]
            )
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
