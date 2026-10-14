use bevy_ptr::{Ptr, PtrMut};

use crate::{kind::Kind, reflect::Reflect, TypePath};
use alloc::{alloc::alloc, boxed::Box};
use core::{
    alloc::Layout,
    any::{Any, TypeId},
    fmt::Debug,
};

pub struct Type {
    pub type_id: TypeId,
    pub kind: Kind,
    pub layout: Option<Layout>,
    pub vtable: TypeVTable,
    pub type_path: TypePath,
}

impl Type {
    pub const fn of<T: Typed>() -> &'static Type {
        T::TYPE
    }

    pub const fn type_path(&self) -> &TypePath {
        &self.type_path
    }

    pub fn alloc(&self) -> Option<*mut u8> {
        // SAFETY: layout is for a sized type
        self.layout.map(|layout| unsafe { alloc(layout) })
    }

    pub fn default(&self) -> Option<Box<dyn Reflect>> {
        let ptr = self.alloc()?;
        let set_default = self.vtable.set_default?;
        // SAFETY: `ptr` is a new allocation using the reflected layout for this type
        unsafe { (set_default)(ptr) };
        let reflect_ptr = (self.vtable.cast_reflect)(ptr);
        // SAFETY: `ptr` is a fully initialized, owned allocation
        Some(unsafe { Box::from_raw(reflect_ptr as *mut dyn Reflect) })
    }

    /// Converts the `ptr` to a *const dyn Reflect pointer, using the Reflect implementation associated
    /// with this type.
    pub fn as_reflect_ptr(&self, ptr: *const u8) -> *const dyn Reflect {
        (self.vtable.cast_reflect)(ptr)
    }

    /// # Safety
    ///
    /// Caller must ensure that `ptr` is a valid, initialized value that matches this [`Type`].
    pub unsafe fn as_reflect<'a>(&self, ptr: Ptr<'a>) -> &'a dyn Reflect {
        // Caller guarantees that ptr matches the type defined in this `Type`, and the `Type` author
        // guarantees that the cast_reflect implementation is valid
        unsafe { &*self.as_reflect_ptr(ptr.as_ptr()) }
    }

    /// # Safety
    ///
    /// Caller must ensure that `ptr` is a valid, initialized value that matches this [`Type`].
    pub unsafe fn as_reflect_mut<'a>(&self, ptr: PtrMut<'a>) -> &'a mut dyn Reflect {
        // Caller guarantees that ptr matches the type defined in this `Type`, and the `Type` author
        // guarantees that the cast_reflect implementation is valid
        unsafe { &mut *(self.as_reflect_ptr(ptr.as_ptr()) as *mut dyn Reflect) }
    }
}

pub unsafe trait Typed: Any {
    const TYPE: &'static Type;
}

impl Debug for Type {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Type")
            .field("type_id", &self.type_id)
            .field("kind", &self.kind)
            .field("layout", &self.layout)
            .field("type_path", &self.type_path)
            .finish()
    }
}

pub struct TypeVTable {
    pub cast_reflect: fn(*const u8) -> *const dyn Reflect,
    pub set_default: Option<unsafe fn(*mut u8)>,
}

pub fn cast_reflect<T: Reflect>(ptr: *const u8) -> *const dyn Reflect {
    ptr as *const T as *const dyn Reflect
}

pub unsafe fn set_default<T: Default>(ptr: *mut u8) {
    // SAFETY: caller verifies that ptr is a valid allocation of type T, and it corresponds
    // to a unique, mutable reference
    unsafe {
        *(ptr as *mut T) = T::default();
    }
}
