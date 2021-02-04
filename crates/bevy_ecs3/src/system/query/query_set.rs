use crate::{
    core::{Fetch, QueryFilter, World, WorldQuery},
    deleteme::{ArchetypeComponent, QueryAccess, TypeAccess},
    system::Query,
};
use bevy_ecs3_macros::impl_query_set;

pub struct QuerySet<T: QueryTuple> {
    value: T,
}

impl_query_set!();

pub trait QueryTuple {
    /// # Safety
    /// this might cast world and component access to the relevant Self lifetimes. verify that this is safe in each impl
    unsafe fn new(world: &World, component_access: &TypeAccess<ArchetypeComponent>) -> Self;
    fn get_accesses() -> Vec<QueryAccess>;
}

impl<T: QueryTuple> QuerySet<T> {
    /// # Safety
    /// This will create a set of Query types that could violate memory safety rules. Make sure that this is only called in
    /// ways that ensure the Queries have unique mutable access.
    pub(crate) unsafe fn new(
        world: &World,
        component_access: &TypeAccess<ArchetypeComponent>,
    ) -> Self {
        QuerySet {
            value: T::new(world, component_access),
        }
    }
}
