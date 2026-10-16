pub use bevy_reflect2_macros::Reflect;

use crate::{
    kind::Kind,
    ty::{Type, Typed},
    EnumMut, EnumRef, StructMut, StructRef,
};
use alloc::boxed::Box;
use bevy_ptr::{Ptr, PtrMut};
use core::{any::Any, ptr::NonNull};

// Design Notes:
// - Keep this small ... it will be implemented for many types. Don't tack on a bunch of convenience
//   methods here
pub unsafe trait Reflect: Any {
    fn ty(&self) -> &'static Type;
}

unsafe impl<T: Typed> Reflect for T {
    fn ty(&self) -> &'static Type {
        T::TYPE
    }
}

impl dyn Reflect {
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }

    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut::<T>()
    }

    pub fn downcast<T: Any>(self: Box<Self>) -> Result<Box<T>, Box<dyn Any>> {
        (self as Box<dyn Any>).downcast::<T>()
    }

    pub fn ptr(&self) -> Ptr<'_> {
        let ptr = self as *const dyn Reflect as *mut u8;
        // SAFETY: references are never null. `ptr` is constrained to the correct lifetime.
        unsafe { Ptr::new(NonNull::new_unchecked(ptr)) }
    }

    pub fn ptr_mut(&mut self) -> PtrMut<'_> {
        let ptr = self as *mut dyn Reflect as *mut u8;
        // SAFETY: references are never null. `ptr` is constrained to the correct lifetime.
        unsafe { PtrMut::new(NonNull::new_unchecked(ptr)) }
    }

    pub fn struct_ref(&self) -> Option<StructRef<'_>> {
        let Kind::Struct(struct_type) = &self.ty().kind else {
            return None;
        };

        // SAFETY: self.ptr() is a valid, initialized type that matches the type described in struct_type
        Some(unsafe { StructRef::new(self.ptr(), struct_type) })
    }

    pub fn struct_mut(&mut self) -> Option<StructMut<'_>> {
        let Kind::Struct(struct_type) = &self.ty().kind else {
            return None;
        };

        // SAFETY: self.ptr() is a valid, initialized type that matches the type described in struct_type
        Some(unsafe { StructMut::new(self.ptr_mut(), struct_type) })
    }

    pub fn enum_ref(&self) -> Option<EnumRef<'_>> {
        let Kind::Enum(enum_type) = &self.ty().kind else {
            return None;
        };

        // SAFETY: self.ptr() is a valid, initialized type that matches the type described in enum_type
        Some(unsafe { EnumRef::new(self.ptr(), enum_type) })
    }

    pub fn enum_mut(&mut self) -> Option<EnumMut<'_>> {
        let Kind::Enum(enum_type) = &self.ty().kind else {
            return None;
        };

        // SAFETY: self.ptr() is a valid, initialized type that matches the type described in enum_type
        Some(unsafe { EnumMut::new(self.ptr_mut(), enum_type) })
    }
}
