//! Rust reflection.

#![expect(unsafe_code, reason = "This crate uses unsafe")]
#![expect(missing_docs, reason = "Work in progress")]
#![no_std]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod impls;

/// The reflect prelude.
///
/// This includes the most common types in this crate, re-exported for your convenience.
pub mod prelude {
    #[doc(hidden)]
    pub use crate::Reflect;
}

mod constcat;
mod kind;
mod reflect;
mod ty;
mod type_path;
mod type_registry;

pub use constcat::*;
pub use kind::*;
pub use reflect::*;
pub use ty::*;
pub use type_path::*;
pub use type_registry::*;

#[cfg(test)]
mod tests {
    use crate::{self as bevy_reflect2, CreateTypeData, Kind, Reflect, TypeRegistry, Typed};
    use core::any::{Any, TypeId};

    #[test]
    fn basics() {
        #[derive(Reflect)]
        struct Foo {
            value: u32,
            other: f32,
        }

        let foo = Foo {
            value: 10,
            other: 20.,
        };
        let reflect = &foo as &dyn Reflect;
        assert_eq!(reflect.ty().type_id, TypeId::of::<Foo>());

        assert_eq!(reflect.ty().type_path().full(), "bevy_reflect2::tests::Foo");
        assert_eq!(reflect.ty().type_path().short(), "Foo");
        assert_eq!(
            reflect.ty().type_path().module_name(),
            Some("bevy_reflect2::tests")
        );
        assert_eq!(reflect.ty().type_path().crate_name(), Some("bevy_reflect2"));

        let struct_ref = reflect.struct_ref().unwrap();
        let field = struct_ref.field(0).unwrap();
        assert_eq!(field.ty().type_id, TypeId::of::<u32>());
        let value = field.downcast_ref::<u32>().unwrap();
        assert_eq!(*value, 10);

        let field = struct_ref.field(1).unwrap();
        assert_eq!(field.ty().type_id, TypeId::of::<f32>());
        let value = field.downcast_ref::<f32>().unwrap();
        assert_eq!(*value, 20.);
    }

    #[test]
    fn reflect_default() {
        #[derive(Reflect)]
        #[reflect(Default)]
        struct Foo(u32);

        impl Default for Foo {
            fn default() -> Self {
                Self(42)
            }
        }

        let foo = Foo::TYPE.default().unwrap();
        assert_eq!(foo.downcast_ref::<Foo>().unwrap().0, 42);

        #[derive(Reflect)]
        #[reflect(Default(bar))]
        struct Bar(u32);

        fn bar() -> Bar {
            Bar(10)
        }

        let bar = Bar::TYPE.default().unwrap();
        assert_eq!(bar.downcast_ref::<Bar>().unwrap().0, 10);
    }

    #[test]
    fn custom_type_data() {
        #[derive(Clone)]
        struct ReflectThing(TypeId);

        impl<T: Any> CreateTypeData<T> for ReflectThing {
            fn create_type_data(_input: ()) -> Self {
                Self(TypeId::of::<T>())
            }
        }

        #[derive(Reflect)]
        #[reflect(Thing)]
        struct Foo;

        let mut type_registry = TypeRegistry::new();
        type_registry.register::<Foo>();
        let registration = type_registry.get(TypeId::of::<Foo>()).unwrap();
        let thing = registration.data::<ReflectThing>().unwrap();
        assert_eq!(thing.0, TypeId::of::<Foo>());
    }
    // #[test]
    // fn generic_basics() {
    //     #[derive(Reflect)]
    //     struct Foo<T> {
    //         value: T,
    //         other: f32,
    //     }
    //     let foo = Foo {
    //         value: 10.0f32,
    //         other: 20.,
    //     };
    //     let reflect = &foo as &dyn Reflect;
    //     assert_eq!(
    //         reflect.ty().type_path().full(),
    //         "bevy_reflect2::tests::Foo<f32>"
    //     );

    //     trait Foo {
    //         const VALUE: &str;
    //     }

    //     struct X;

    //     impl Foo for X {
    //         const VALUE: &str = "hello";
    //     }

    //     struct Wrapped<X>(X);
    //     impl<X: Foo> Wrapped<X> {
    //         const __VALUE: &str = <X as Foo>::VALUE;
    //     }

    //     impl<X: Foo> Foo for Wrapped<X> {
    //         const VALUE: &str = { crate::concat!("<", Self::__VALUE) };
    //     }
    // }

    #[test]
    fn initialize() {
        #[derive(Reflect, PartialEq, Eq, Debug)]
        struct Foo {
            a: u32,
            b: u32,
        }

        let ty = Foo::TYPE;
        let Kind::Struct(struct_kind) = &ty.kind else {
            panic!("expected struct type")
        };

        let mut values = [("a", 10), ("b", 20)].iter();

        let value = unsafe {
            struct_kind.initialize(ty, |field, ptr| {
                let (name, value) = values.next().unwrap();
                assert_eq!(*name, field.name);
                *ptr.deref_mut::<u32>() = *value;
            })
        }
        .unwrap();

        assert_eq!(value.downcast_ref::<Foo>().unwrap(), &Foo { a: 10, b: 20 });
    }
}
