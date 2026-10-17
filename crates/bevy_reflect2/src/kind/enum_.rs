use core::any::Any;

use bevy_ptr::{Ptr, PtrMut};

use crate::{FieldKind, Reflect, TypeRef};

// # Todo
// - Implement offset-driven fast-path for repr C types
//
// # Rust Feature Wishlist
//  - [offset_of_enum](https://github.com/rust-lang/rust/issues/120141): Would enable efficient
//    offset-driven field access and reusing the Field / StructRef types
#[derive(Debug)]
pub struct Enum {
    pub variants: &'static [Variant],
    pub cast_enum_access: fn(*const u8) -> *const dyn EnumAccess,
}

impl Enum {
    /// Caller must ensure that `enum_ptr` is a valid, initialized value that matches this [`Enum`].
    pub unsafe fn enum_access<'a>(&self, enum_ptr: Ptr<'a>) -> &'a dyn EnumAccess {
        // Caller guarantees that ptr matches the type defined in this `Enum`, and the `Enum` author
        // guarantees that the cast_enum_access implementation is valid
        unsafe { &*(self.cast_enum_access)(enum_ptr.as_ptr()) }
    }

    /// Caller must ensure that `enum_ptr` is a valid, initialized value that matches this [`Enum`].
    pub unsafe fn enum_access_mut<'a>(&self, enum_ptr: &mut PtrMut<'a>) -> &'a mut dyn EnumAccess {
        // Caller guarantees that ptr matches the type defined in this `Enum`, and the `Enum` author
        // guarantees that the cast_enum_access implementation is valid
        unsafe { &mut *((self.cast_enum_access)(enum_ptr.as_ptr()) as *mut dyn EnumAccess) }
    }
}

#[derive(Debug)]
pub struct Variant {
    pub name: &'static str,
    pub kind: FieldKind,
    pub fields: &'static [EnumField],
}

#[derive(Debug)]
pub struct EnumField {
    pub name: &'static str,
    pub ty: TypeRef,
}

/// # Safety
///
/// [`EnumAccess::field`] must return a tuple containing (1) a valid pointer to the field at the given index,
/// and (2) a valid "cast reflect" function for fields of that type
pub unsafe trait EnumAccess: Any {
    fn field(&self, index: usize) -> Option<&dyn Reflect>;
    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect>;
    fn variant(&self) -> usize;
}

pub fn cast_enum_access<E: EnumAccess>(ptr: *const u8) -> *const dyn EnumAccess {
    ptr as *const E as *const dyn EnumAccess
}

pub struct EnumRef<'a> {
    pub(crate) access: &'a dyn EnumAccess,
    pub(crate) enum_type: &'static Enum,
}

impl<'a> EnumRef<'a> {
    #[inline]
    pub fn enum_type(&self) -> &'static Enum {
        self.enum_type
    }

    #[inline]
    pub fn field(&self, index: usize) -> Option<&dyn Reflect> {
        self.access.field(index)
    }

    #[inline]
    pub fn variant(&self) -> usize {
        self.access.variant()
    }
}

pub struct EnumMut<'a> {
    pub(crate) access: &'a mut dyn EnumAccess,
    pub(crate) enum_type: &'static Enum,
}

impl<'a> EnumMut<'a> {
    #[inline]
    pub fn enum_type(&self) -> &'static Enum {
        self.enum_type
    }

    #[inline]
    pub fn field(&self, index: usize) -> Option<&dyn Reflect> {
        self.access.field(index)
    }

    #[inline]
    pub fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        self.access.field_mut(index)
    }

    #[inline]
    pub fn variant(&self) -> usize {
        self.access.variant()
    }
}
