use crate::{self as bevy_reflect2, TypePath};
use crate::{GetTypeRegistration, TypeRegistration};
use alloc::string::String;
use bevy_reflect2_macros::type_path;
use core::{alloc::Layout, any::TypeId};

use crate::{
    kind::{Kind, Primitive},
    ty::{cast_reflect, set_default, Type, TypeVTable, Typed},
};

macro_rules! impl_primitive_type {
    ($path:ty, $primitive_type:path, $type_path:expr) => {
        unsafe impl Typed for $path {
            const TYPE: &'static Type = &Type {
                type_id: TypeId::of::<$path>(),
                kind: Kind::Primitive($primitive_type),
                layout: Some(Layout::new::<$path>()),
                vtable: TypeVTable {
                    cast_reflect: cast_reflect::<$path>,
                    set_default: Some(set_default::<$path>),
                },
                type_path: $type_path,
            };
        }

        impl GetTypeRegistration for $path {
            fn get_type_registration() -> TypeRegistration {
                TypeRegistration::of::<Self>()
            }
        }
    };
    ($path:ty, $primitive_type:path) => {
        impl_primitive_type! {$path, $primitive_type, type_path!($path)}
    };
}

impl_primitive_type!(bool, Primitive::Bool);
impl_primitive_type!(u8, Primitive::Int);
impl_primitive_type!(u16, Primitive::Int);
impl_primitive_type!(u32, Primitive::Int);
impl_primitive_type!(u64, Primitive::Int);
impl_primitive_type!(u128, Primitive::Int);
impl_primitive_type!(usize, Primitive::Int);
impl_primitive_type!(i8, Primitive::Int);
impl_primitive_type!(i16, Primitive::Int);
impl_primitive_type!(i32, Primitive::Int);
impl_primitive_type!(i64, Primitive::Int);
impl_primitive_type!(i128, Primitive::Int);
impl_primitive_type!(isize, Primitive::Int);
impl_primitive_type!(f32, Primitive::Float);
impl_primitive_type!(f64, Primitive::Float);
impl_primitive_type!(char, Primitive::Char);
impl_primitive_type!(String, Primitive::String);
impl_primitive_type!(
    (),
    Primitive::Unit,
    TypePath::new("()", "()", (0, 2), None, None)
);
