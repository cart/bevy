use crate::{ty::Type, Reflect};
use alloc::boxed::Box;
use bevy_ptr::{Ptr, PtrMut};
use core::{fmt::Debug, ptr::NonNull};

#[derive(Debug)]
pub struct Struct {
    pub fields: &'static [Field],
    pub kind: FieldKind,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldKind {
    Named,
    Unnamed,
    Unit,
}

#[derive(Debug)]
pub struct Field {
    pub name: &'static str,
    pub offset: usize,
    pub ty: TypeRef,
}

impl Struct {
    /// # Safety
    /// `struct_type` must be the type of this field's "parent struct"
    /// `func` must initialize every pointer to a valid value that matches the type described in each [`Field`].
    pub unsafe fn initialize(
        &self,
        struct_type: &Type,
        mut func: impl FnMut(&Field, PtrMut),
    ) -> Option<Box<dyn Reflect>> {
        let ptr = struct_type.alloc()?;
        for field in self.fields {
            // SAFETY: ptr corresponds to an allocation of the type of the parent struct of this field, as it was just initialized
            let field_ptr = unsafe { field.offset_ptr(ptr) };
            // SAFETY: field_ptr is a unique ptr to an allocated section of memory for this field
            let field_ptr_mut =
                unsafe { PtrMut::new(NonNull::new_unchecked(field_ptr as *mut u8)) };
            (func)(field, field_ptr_mut)
        }

        // SAFETY: `ptr` is now fully initialized
        Some(unsafe { Box::from_raw(struct_type.as_reflect_ptr(ptr) as *mut dyn Reflect) })
    }
}

impl Field {
    /// # Safety
    ///
    /// `struct_ptr` must be a valid instance of the type of this field's parent struct
    pub unsafe fn offset_ptr<'a>(&self, ptr: *const u8) -> *const u8 {
        // SAFETY: `struct_type` matches the `parent_ptr` type, and the [`Reflect`] implementer guarantees
        // type layout correctness.
        unsafe { ptr.byte_add(self.offset) }
    }

    /// # Safety
    ///
    /// `struct_ptr` must be a valid instance of the type of this field's parent struct
    pub unsafe fn access<'a>(&self, struct_ptr: Ptr<'a>) -> &'a dyn Reflect {
        // SAFETY: self.ptr matches the type of the parent struct of this field
        let field_ptr = unsafe { self.offset_ptr(struct_ptr.as_ptr()) };
        // SAFETY: field_ptr is valid and matches the type of field.ty
        unsafe {
            self.ty
                .get()
                .as_reflect(Ptr::new(NonNull::new_unchecked(field_ptr as *mut u8)))
        }
    }

    /// # Safety
    ///
    /// `struct_ptr` must be a valid instance of the type of this field's parent struct
    pub unsafe fn access_mut<'a>(&self, struct_ptr: &'a mut PtrMut) -> &'a mut dyn Reflect {
        // SAFETY: self.ptr matches the type of the parent struct of this field
        let field_ptr = unsafe { self.offset_ptr(struct_ptr.as_ptr()) };
        // SAFETY: field_ptr is a unique, valid reference to the field.
        let field_ptr_mut = unsafe { PtrMut::new(NonNull::new_unchecked(field_ptr as *mut u8)) };
        // SAFETY: field_ptr is valid and matches the type of field.ty
        // Lifetime of the returned mutable Reflect borrow is tied to struct_ptr to avoid potential aliasing
        unsafe { self.ty.get().as_reflect_mut(field_ptr_mut) }
    }
}

/// A lazy reference to a [`Type`] via a function pointer. This makes evaluation lazy, which moves
/// const-eval to runtime, boosting compile times.
#[derive(Clone, Copy)]
pub struct TypeRef(pub fn() -> &'static Type);

impl TypeRef {
    /// Get the referenced type by calling the function.
    #[inline]
    pub fn get(&self) -> &'static Type {
        (self.0)()
    }
}

impl Debug for TypeRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("TypeRef")
            .field(&(self.0)().type_path.short())
            .finish()
    }
}

pub struct StructRef<'a> {
    // SAFETY NOTE: this should only ever be constructed with StructRef::new
    ptr: Ptr<'a>,
    // SAFETY NOTE: this should only ever be constructed with StructRef::new
    struct_type: &'static Struct,
}

impl<'a> StructRef<'a> {
    /// # Safety
    ///
    /// `ptr` must be a valid, initialized value that exactly matches the type described in `struct_type`
    pub unsafe fn new(ptr: Ptr<'a>, struct_type: &'static Struct) -> Self {
        Self { ptr, struct_type }
    }

    pub fn field(&self, index: usize) -> Option<&dyn Reflect> {
        let field = self.struct_type.fields.get(index)?;
        // SAFETY: self.ptr matches the type of the parent struct of this field
        Some(unsafe { field.access(self.ptr) })
    }
}

pub struct StructMut<'a> {
    // SAFETY NOTE: this should only ever be constructed with StructMut::new
    ptr: PtrMut<'a>,
    // SAFETY NOTE: this should only ever be constructed with StructRef::new
    struct_type: &'static Struct,
}

impl<'a> StructMut<'a> {
    /// # Safety
    ///
    /// `ptr` must be a valid, initialized value that exactly matches the type described in `struct_type`
    pub unsafe fn new(ptr: PtrMut<'a>, struct_type: &'static Struct) -> Self {
        Self { ptr, struct_type }
    }

    pub fn field(&self, index: usize) -> Option<&dyn Reflect> {
        let field = self.struct_type.fields.get(index)?;
        // SAFETY: self.ptr matches the type of the parent struct of this field
        Some(unsafe { field.access(self.ptr.as_ref()) })
    }

    pub fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        let field = self.struct_type.fields.get(index)?;
        // SAFETY: self.ptr matches the type of the parent struct of this field
        Some(unsafe { field.access_mut(&mut self.ptr) })
    }
}
