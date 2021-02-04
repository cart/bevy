use crate::core::SparseSetIndex;
use fixedbitset::FixedBitSet;
use std::marker::PhantomData;
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Access<T: SparseSetIndex> {
    reads_all: bool,
    reads_and_writes: FixedBitSet,
    writes: FixedBitSet,
    marker: PhantomData<T>,
}

impl<T: SparseSetIndex> Default for Access<T> {
    fn default() -> Self {
        Self {
            reads_all: false,
            reads_and_writes: Default::default(),
            writes: Default::default(),
            marker: PhantomData,
        }
    }
}

impl<T: SparseSetIndex> Access<T> {
    pub fn grow(&mut self, bits: usize) {
        self.reads_and_writes.grow(bits);
        self.writes.grow(bits);
    }

    pub fn add_read(&mut self, index: T) {
        self.reads_and_writes.insert(index.sparse_set_index());
    }

    pub fn add_write(&mut self, index: T) {
        self.reads_and_writes.insert(index.sparse_set_index());
        self.writes.insert(index.sparse_set_index());
    }

    pub fn read_all(&mut self) {
        self.reads_all = true;
    }

    pub fn reads_all(&self) -> bool {
        self.reads_all
    }

    pub fn clear(&mut self) {
        self.reads_all = false;
        self.reads_and_writes.clear();
        self.writes.clear();
    }

    pub fn extend(&mut self, other: &Access<T>) {
        self.reads_all = self.reads_all || other.reads_all;
        self.reads_and_writes.union_with(&other.reads_and_writes);
        self.writes.union_with(&other.writes);
    }

    pub fn is_compatible(&self, other: &Access<T>) -> bool {
        if self.reads_all {
            0 == other.writes.count_ones(..)
        } else if other.reads_all {
            0 == self.writes.count_ones(..)
        } else {
            self.writes.is_disjoint(&other.reads_and_writes)
                && self.reads_and_writes.is_disjoint(&other.writes)
        }
    }
}
