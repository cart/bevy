use std::mem::MaybeUninit;

use downcast_rs::{impl_downcast, Downcast};

use crate::{Component, Entity};

pub struct SparseSet<T: Component> {
    values: Vec<T>,
    entities: Vec<Entity>,
    sparse: Vec<MaybeUninit<usize>>,
}

impl<T: Component> Default for SparseSet<T> {
    fn default() -> Self {
        Self {
            values: Default::default(),
            entities: Default::default(),
            sparse: Default::default(),
        }
    }
}

impl<T: Component> SparseSet<T> {}

trait SparseStorage: Downcast {}
impl_downcast!(SparseStorage);


#[derive(Default)]
pub struct SparseSets {}
