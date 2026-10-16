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
    fn field(&self, index: usize) -> Option<(*const u8, fn(*const u8) -> *const dyn Reflect)>;
    // fn variant(&self) -> usize;
}

pub fn cast_enum_access<E: EnumAccess>(ptr: *const u8) -> *const dyn EnumAccess {
    ptr as *const E as *const dyn EnumAccess
}

pub struct EnumRef<'a> {
    // SAFETY NOTE: this should only ever be constructed with EnumRef::new
    ptr: Ptr<'a>,
    // SAFETY NOTE: this should only ever be constructed with EnumRef::new
    enum_type: &'static Enum,
}

impl<'a> EnumRef<'a> {
    /// # Safety
    ///
    /// `ptr` must be a valid, initialized value that exactly matches the type described in `enum_type`
    pub unsafe fn new(ptr: Ptr<'a>, enum_type: &'static Enum) -> Self {
        Self { ptr, enum_type }
    }

    pub fn field(&self, index: usize) -> Option<&dyn Reflect> {
        // SAFETY: self.ptr matches the `enum_type`
        let enum_access = unsafe { self.enum_type.enum_access(self.ptr) };
        let (field_ptr, cast_reflect) = enum_access.field(index)?;
        // EnumAccess implementer guarantees that field_ptr is a valid pointer to the field at that index, and that
        // cast_reflect is a valid "cast reflect" function for fields of that type
        Some(unsafe { &*cast_reflect(field_ptr) })
    }
}

pub struct EnumMut<'a> {
    // SAFETY NOTE: this should only ever be constructed with EnumRef::new
    ptr: PtrMut<'a>,
    // SAFETY NOTE: this should only ever be constructed with EnumRef::new
    enum_type: &'static Enum,
}

impl<'a> EnumMut<'a> {
    /// # Safety
    ///
    /// `ptr` must be a valid, initialized value that exactly matches the type described in `enum_type`
    pub unsafe fn new(ptr: PtrMut<'a>, enum_type: &'static Enum) -> Self {
        Self { ptr, enum_type }
    }

    pub fn field(&self, index: usize) -> Option<&dyn Reflect> {
        // SAFETY: self.ptr matches the `enum_type`
        let enum_access = unsafe { self.enum_type.enum_access(self.ptr.as_ref()) };
        let (field_ptr, cast_reflect) = enum_access.field(index)?;
        // EnumAccess implementer guarantees that field_ptr is a valid pointer to the field at that index, and that
        // cast_reflect is a valid "cast reflect" function for fields of that type
        Some(unsafe { &*cast_reflect(field_ptr) })
    }

    pub fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        // SAFETY: self.ptr matches the `enum_type`
        let enum_access = unsafe { self.enum_type.enum_access(self.ptr.as_ref()) };
        let (field_ptr, cast_reflect) = enum_access.field(index)?;
        // SAFETY: EnumAccess implementer guarantees that field_ptr is a valid pointer to the field at that index, and that
        // cast_reflect is a valid "cast reflect" function for fields of that type.
        // We have unique access to the struct ptr, so accessing the field mutably is ok.
        Some(unsafe { &mut *(cast_reflect(field_ptr) as *mut dyn Reflect) })
    }
}
