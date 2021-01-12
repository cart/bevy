pub mod core;

pub mod prelude {
    pub use crate::core::*;
}

#[macro_export]
macro_rules! smaller_tuples_too {
    ($m: ident, $ty: ident) => {
        $m!{$ty}
        $m!{}
    };
    ($m: ident, $ty: ident, $($tt: ident),*) => {
        $m!{$ty, $($tt),*}
        smaller_tuples_too!{$m, $($tt),*}
    };
}

#[cfg(test)]
mod tests {
    use crate::prelude::{ComponentDescriptor, StorageType, World};

    #[test]
    fn random_access() {
        let mut world = World::new();
        world
            .components_mut()
            .add(ComponentDescriptor::of::<i32>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn(("abc", 123));
        let f = world.spawn(("def", 456, true));
        assert_eq!(*world.get::<&str>(e).unwrap(), "abc");
        assert_eq!(*world.get::<i32>(e).unwrap(), 123);
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);

        // test archetype get_mut()
        *world.get_mut::<&'static str>(e).unwrap() = "xyz";
        assert_eq!(*world.get::<&'static str>(e).unwrap(), "xyz");

        // test sparse set get_mut()
        *world.get_mut::<i32>(f).unwrap() = 42;
        assert_eq!(*world.get::<i32>(f).unwrap(), 42);
    }

    #[test]
    fn despawn_archetype() {
        let mut world = World::new();
        let e = world.spawn(("abc", 123));
        let f = world.spawn(("def", 456));
        assert_eq!(world.query::<()>().count(), 2);
        world.despawn(e).unwrap();
        assert_eq!(world.query::<()>().count(), 1);
        assert!(world.get::<&str>(e).is_err());
        assert!(world.get::<i32>(e).is_err());
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);
    }

    #[test]
    fn despawn_mixed() {
        let mut world = World::new();
        world
            .components_mut()
            .add(ComponentDescriptor::of::<i32>(StorageType::SparseSet))
            .unwrap();
        let e = world.spawn(("abc", 123));
        let f = world.spawn(("def", 456));
        assert_eq!(world.query::<()>().count(), 2);
        world.despawn(e).unwrap();
        assert_eq!(world.query::<()>().count(), 1);
        assert!(world.get::<&str>(e).is_err());
        assert!(world.get::<i32>(e).is_err());
        assert_eq!(*world.get::<&str>(f).unwrap(), "def");
        assert_eq!(*world.get::<i32>(f).unwrap(), 456);
    }
}
