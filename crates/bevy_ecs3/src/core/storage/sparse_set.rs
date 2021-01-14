use crate::core::{BlobVec, ComponentFlags, ComponentId, ComponentInfo, Entity, TypeInfo};
use std::{alloc::Layout, cell::UnsafeCell, marker::PhantomData};

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
    pub unsafe fn remove_unchecked(&mut self, index: I) -> Option<V> {
        let index = index.sparse_set_index();
        self.values.get_unchecked_mut(index).take()
    }

    #[inline]
    pub fn remove(&mut self, index: I) -> Option<V> {
        let index = index.sparse_set_index();
        if index >= self.values.len() {
            None
        } else {
            // SAFE: checked that index is valid above
            unsafe { self.values.get_unchecked_mut(index).take() }
        }
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
pub struct BlobSparseSet<I> {
    dense: BlobVec,
    sparse: SparseArray<I, usize>,
}

impl<I: SparseSetIndex> BlobSparseSet<I> {
    pub fn new(item_layout: Layout, drop: unsafe fn(*mut u8), capacity: usize) -> Self {
        Self {
            dense: BlobVec::new(item_layout, drop, capacity),
            sparse: Default::default(),
        }
    }

    pub fn new_typed<T>(capacity: usize) -> Self {
        Self {
            dense: BlobVec::new_typed::<T>(capacity),
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
        let dense = &mut self.dense;
        let dense_index = *self.sparse.get_or_insert_with(index, move || {
            dense.push_uninit();
            dense.len() - 1
        });
        // SAFE: dense_index exists thanks to the call above
        self.dense.set_unchecked(dense_index, value);
    }

    #[inline]
    pub fn contains(&self, index: I) -> bool {
        self.sparse.contains(index)
    }

    #[inline]
    pub fn get(&self, index: I) -> Option<*mut u8> {
        self.sparse.get(index).map(|dense_index| {
            // SAFE: if the sparse index points to something in the dense vec, it exists
            unsafe { self.dense.get_unchecked(*dense_index) }
        })
    }

    /// SAFETY: `index` must have a value stored in the set
    #[inline]
    pub unsafe fn get_unchecked(&self, index: I) -> *mut u8 {
        let dense_index = self.sparse.get_unchecked(index);
        self.dense.get_unchecked(*dense_index)
    }

    /// SAFETY: it is the caller's responsibility to drop the returned ptr (if Some is returned).
    pub unsafe fn remove_and_forget(&mut self, index: I) -> Option<*mut u8> {
        self.sparse
            .remove(index)
            .map(|dense_index| self.dense.swap_remove_and_forget_unchecked(dense_index))
    }

    /// SAFETY: `index` must have a value stored in the set
    pub fn remove(&mut self, index: I) {
        self.sparse.remove(index).map(|dense_index|
            // SAFE: if the sparse index points to something in the dense vec, it exists
            unsafe {self.dense.swap_remove_unchecked(dense_index)});
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
                .remove_and_forget(index)
                .map(|value| std::ptr::read(value.cast::<V>()))
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
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.index()
    }
}

pub struct ComponentSparseSet {
    sparse_set: BlobSparseSet<Entity>,
}

impl ComponentSparseSet {
    pub fn new(component_info: &ComponentInfo) -> ComponentSparseSet {
        ComponentSparseSet {
            sparse_set: BlobSparseSet::new(component_info.layout(), component_info.drop(), 64),
        }
    }

    /// SAFETY: The caller must ensure that component_ptr is a pointer to the type described by TypeInfo
    /// The caller must also ensure that the component referenced by component_ptr is not dropped.
    /// This [SparseSetStorage] takes ownership of component_ptr and will drop it at the appropriate time.
    pub unsafe fn put_component(&mut self, entity: Entity, component_ptr: *mut u8) {
        self.sparse_set.insert(entity, component_ptr);
    }

    pub fn get_component(&self, entity: Entity) -> Option<*mut u8> {
        self.sparse_set.get(entity)
    }

    /// SAFETY: this sparse set must contain the given `entity`
    pub unsafe fn get_component_unchecked(&self, entity: Entity) -> *mut u8 {
        self.sparse_set.get_unchecked(entity)
    }

    /// SAFETY: it is the caller's responsibility to drop the returned ptr (if Some is returned).
    pub unsafe fn remove_component_and_forget(&mut self, entity: Entity) -> Option<*mut u8> {
        self.sparse_set.remove_and_forget(entity)
    }

    pub fn remove_component(&mut self, entity: Entity) {
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
    pub fn get_or_insert(&mut self, component_info: &ComponentInfo) -> &mut ComponentSparseSet {
        if !self.sets.contains(component_info.id()) {
            self.sets.insert(
                component_info.id(),
                UnsafeCell::new(ComponentSparseSet::new(component_info)),
            );
        }

        // SAFE: unique access to self
        unsafe { &mut *self.sets.get_mut(component_info.id()).unwrap().get() }
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
