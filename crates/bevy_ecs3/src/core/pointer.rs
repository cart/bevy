use std::ops::{Deref, DerefMut};
use crate::core::{Component, ComponentFlags};

/// Unique borrow of an entity's component
pub struct Mut<'a, T: Component> {
    pub(crate) value: &'a mut T,
    pub(crate) flags: &'a mut ComponentFlags,
}

impl<'a, T: Component> Deref for Mut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'a, T: Component> DerefMut for Mut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        self.flags.insert(ComponentFlags::MUTATED);
        self.value
    }
}

impl<'a, T: Component + core::fmt::Debug> core::fmt::Debug for Mut<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.value.fmt(f)
    }
}
