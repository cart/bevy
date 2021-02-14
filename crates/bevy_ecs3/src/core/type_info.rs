use std::{alloc::Layout, any::TypeId};

/// Metadata required to store a component
#[derive(Debug, Copy, Clone)]
pub struct TypeInfo {
    id: TypeId,
    layout: Layout,
    drop: unsafe fn(*mut u8),
    type_name: &'static str,
    is_send: bool,
}

pub(crate) unsafe fn drop_ptr<T>(x: *mut u8) {
    x.cast::<T>().drop_in_place()
}

impl TypeInfo {
    /// Metadata for `T`
    pub fn of<T: Send + Sync + 'static>() -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }

        Self {
            id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            is_send: true,
            drop: drop_ptr::<T>,
            type_name: core::any::type_name::<T>(),
        }
    }

    pub fn of_non_send<T: 'static>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            is_send: false,
            drop: drop_ptr::<T>,
            type_name: core::any::type_name::<T>(),
        }
    }

    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.id
    }

    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    #[inline]
    pub fn drop(&self) -> unsafe fn(*mut u8) {
        self.drop
    }

    #[inline]
    pub fn is_send(&self) -> bool {
        self.is_send
    }

    #[inline]
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }
}
