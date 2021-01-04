use crate::{Component, ComponentId, Entity};
use std::any::Any;

#[derive(Debug)]
pub struct SparseSet<I, V> {
    dense: Vec<V>,
    indices: Vec<I>,
    sparse: Vec<Option<usize>>,
}

impl<I, V> Default for SparseSet<I, V> {
    fn default() -> Self {
        Self {
            dense: Default::default(),
            sparse: Default::default(),
            indices: Default::default(),
        }
    }
}

impl<I: SparseSetIndex, V> SparseSet<I, V> {
    pub fn insert(&mut self, index: I, value: V) {
        let sparse_index = index.sparse_set_index();
        match self.sparse.get_mut(sparse_index) {
            // in bounds, and a value already exists
            Some(Some(dense_index)) => {
                self.dense[*dense_index] = value;
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

    pub fn get(&self, index: I) -> Option<&V> {
        let sparse_index = index.sparse_set_index();
        match self.sparse.get(sparse_index) {
            // in bounds, and a value exists
            Some(Some(dense_index)) => Some(&self.dense[*dense_index]),
            // the value does not exist
            _ => None,
        }
    }

    pub fn get_mut(&mut self, index: I) -> Option<&mut V> {
        let sparse_index = index.sparse_set_index();
        match self.sparse.get(sparse_index) {
            // in bounds, and a value exists
            Some(Some(dense_index)) => Some(&mut self.dense[*dense_index]),
            // the value does not exist
            _ => None,
        }
    }

    pub fn remove(&mut self, index: I) -> Option<V> {
        let sparse_index = index.sparse_set_index();
        if sparse_index >= self.sparse.len() {
            return None;
        }
        // SAFE: access to indices that exist. access is disjoint
        unsafe {
            let sparse_ptr = self.sparse.as_mut_ptr();
            let dense_value = &mut *sparse_ptr.add(sparse_index);
            if let Some(dense_index) = *dense_value {
                let removed = self.dense.swap_remove(dense_index);
                self.indices.swap_remove(dense_index);
                *dense_value = None;
                let moved_dense_value =
                    sparse_ptr.add(self.indices[dense_index].sparse_set_index());
                *moved_dense_value = Some(dense_index);
                Some(removed)
            } else {
                None
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&I, &V)> {
        self.indices.iter().zip(self.dense.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&I, &mut V)> {
        self.indices.iter().zip(self.dense.iter_mut())
    }
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

pub struct SparseSetStorage {
    sparse_set: Box<dyn Any>,
}

impl SparseSetStorage {
    pub fn new<T: Component>() -> SparseSetStorage {
        SparseSetStorage {
            sparse_set: Box::new(SparseSet::<Entity, T>::default()),
        }
    }
}

#[derive(Default)]
pub struct SparseSets {
    sets: SparseSet<ComponentId, SparseSetStorage>,
}

impl SparseSets {
    // pub fn get_or_insert(&mut self, type_info: TypeInfo) -> &mut SparseSetStorage {

    // }
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
