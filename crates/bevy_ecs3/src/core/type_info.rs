use std::{alloc::Layout, any::TypeId};


/// Metadata required to store a component
#[derive(Debug, Copy, Clone)]
pub struct TypeInfo {
    id: TypeId,
    layout: Layout,
    drop: unsafe fn(*mut u8),
    type_name: &'static str,
}

impl TypeInfo {
    /// Metadata for `T`
    pub fn of<T: 'static>() -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }

        Self {
            id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop: drop_ptr::<T>,
            type_name: core::any::type_name::<T>(),
        }
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn id(&self) -> TypeId {
        self.id
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn drop(&self) -> unsafe fn(*mut u8) {
        self.drop
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }
}