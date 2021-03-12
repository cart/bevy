use std::any::TypeId;

use crate::{
    component::{Component, ComponentId},
    storage::SparseSetIndex,
};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct RelationshipKindId(usize);

impl RelationshipKindId {
    #[inline]
    pub const fn new(index: usize) -> RelationshipKindId {
        RelationshipKindId(index)
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0
    }
}

impl SparseSetIndex for RelationshipKindId {
    #[inline]
    fn sparse_set_index(&self) -> usize {
        self.index()
    }

    fn get_sparse_set_index(value: usize) -> Self {
        Self(value)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum RelationshipTarget {
    Entity(crate::entity::Entity),
    ComponentId(ComponentId),
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct Relationship {
    kind: RelationshipKindId,
    target: RelationshipTarget,
}

impl Relationship {
    pub fn new(kind: RelationshipKindId, target: RelationshipTarget) -> Self {
        Self { kind, target }
    }
}

#[derive(Debug)]
pub struct RelationshipKind {
    id: RelationshipKindId,
    type_id: Option<TypeId>,
    relationships: Vec<(Relationship, ComponentId)>,
}

#[derive(Debug, Default)]
pub struct Relationships {
    kinds: Vec<RelationshipKind>,
    components: Vec<ComponentId>,
}

impl Relationships {
    pub(crate) fn register_kind<T: Component>(&mut self) -> Result<RelationshipKindId, ()> {
        todo!()
    }

    pub(crate) fn register(
        &mut self,
        kind: RelationshipKindId,
        target: RelationshipTarget,
    ) -> Result<RelationshipKindId, ()> {
        todo!("look up relationship kind and add target to ")
    }
}
