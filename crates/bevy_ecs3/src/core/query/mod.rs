mod access;
mod fetch;
mod filter;
mod iter;
mod state;

pub use access::*;
pub use fetch::*;
pub use filter::*;
pub use iter::*;
pub use state::*;

#[cfg(test)]
mod tests {
    use crate::core::{ComponentDescriptor, QueryState, StorageType, World};

    #[derive(Debug, Eq, PartialEq)]
    struct A(usize);
    #[derive(Debug, Eq, PartialEq)]
    struct B(usize);

    #[test]
    fn query() {
        let mut world = World::new();
        world.spawn().insert_bundle((A(1), B(1)));
        world.spawn().insert_bundle((A(2),));
        let values = world.query::<&A>().collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (_a, mut b) in world.query_mut::<(&A, &mut B)>() {
            b.0 = 3;
        }
        let values = world.query::<&B>().collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }

    #[test]
    fn stateful_query() {
        let mut world = World::new();
        let mut query_state = QueryState::default();
        world.spawn().insert_bundle((A(1), B(1)));
        world.spawn().insert_bundle((A(2),));
        unsafe {
            let values = world
                .query::<&A>()
                .with_state(&mut query_state)
                .collect::<Vec<&A>>();
            assert_eq!(values, vec![&A(1), &A(2)]);
        }

        unsafe {
            let mut query_state = QueryState::default();
            for (_a, mut b) in world.query::<(&A, &mut B)>().with_state(&mut query_state) {
                b.0 = 3;
            }
        }

        unsafe {
            let mut query_state = QueryState::default();
            let values = world
                .query::<&B>()
                .with_state(&mut query_state)
                .collect::<Vec<&B>>();
            assert_eq!(values, vec![&B(3)]);
        }
    }

    #[test]
    fn multi_storage_query() {
        let mut world = World::new();
        world
            .register_component(ComponentDescriptor::of::<A>(StorageType::SparseSet))
            .unwrap();

        world.spawn().insert_bundle((A(1), B(2)));
        world.spawn().insert_bundle((A(2),));

        let values = world.query::<&A>().collect::<Vec<&A>>();
        assert_eq!(values, vec![&A(1), &A(2)]);

        for (_a, mut b) in world.query::<(&A, &mut B)>() {
            b.0 = 3;
        }

        let values = world.query::<&B>().collect::<Vec<&B>>();
        assert_eq!(values, vec![&B(3)]);
    }
}
