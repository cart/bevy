use bevy_utils::HashMap;
use std::{any::TypeId, hash::{Hash, Hasher}};

use crate::core::{Component, ComponentId, Entities, Entity, Location, TableId, TypeInfo};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ArchetypeId(pub(crate) u32);

impl ArchetypeId {
    #[inline]
    pub fn empty_archetype() -> ArchetypeId {
        ArchetypeId(0)
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub fn is_empty_archetype(&self) -> bool {
        self.0 == 0
    }
}

#[derive(Hash)]
pub struct ArchetypeHash<'a> {
    table_components: &'a [ComponentId],
    sparse_set_components: &'a [ComponentId],
}

pub struct Edges {
    add_table_component: Vec<ArchetypeId>,
    remove_table_component: Vec<ArchetypeId>,
    add_sparse_set_component: Vec<ArchetypeId>,
    remove_sparse_set_component: Vec<ArchetypeId>,
}

pub struct Archetype {
    table_components: Vec<ComponentId>,
    sparse_set_components: Vec<ComponentId>,
    table: Option<TableId>,
    edges: Edges,
}

/// Determines freshness of information derived from `World::archetypes`
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ArchetypeGeneration(pub u32);

pub struct Archetypes {
    archetypes: Vec<Archetype>,
    archetype_ids: HashMap<u64, ArchetypeId>,
}

impl Archetypes {
    #[inline]
    pub fn empty_archetype(&self) -> &Archetype {
        &self.archetypes[0]
    }

    #[inline]
    pub(crate) fn empty_archetype_mut(&mut self) -> &mut Archetype {
        &mut self.archetypes[0]
    }

    #[inline]
    pub fn generation(&self) -> ArchetypeGeneration {
        ArchetypeGeneration(self.archetypes.len() as u32)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.archetypes.len()
    }

    #[inline]
    pub fn get(&self, id: ArchetypeId) -> Option<&Archetype> {
        self.archetypes.get(id.0 as usize)
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, id: ArchetypeId) -> &Archetype {
        self.archetypes.get_unchecked(id.0 as usize)
    }

    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        self.archetypes.get_unchecked_mut(id.0 as usize)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: ArchetypeId) -> Option<&mut Archetype> {
        self.archetypes.get_mut(id.0 as usize)
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Archetype> {
        self.archetypes.iter()
    }

    #[inline]
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Archetype> {
        self.archetypes.iter_mut()
    }
}