use crate::{entity::Entity, world::EntityWorldMut};
use log::warn;
use std::{borrow::Cow, string::String};
use thiserror::Error;

/// Either a path to an entity, or an entity.
pub enum EntityOrPath<'a> {
    /// An entity
    Entity(Entity),
    /// A path to an entity
    Path(EntityPath<'a>),
}

impl<'a> Default for EntityOrPath<'a> {
    fn default() -> Self {
        Self::Path(Default::default())
    }
}

impl<'a> From<Entity> for EntityOrPath<'a> {
    #[inline]
    fn from(entity: Entity) -> Self {
        Self::Entity(entity)
    }
}

impl<'a> From<&'a str> for EntityOrPath<'a> {
    #[inline]
    fn from(entity_path: &'a str) -> Self {
        Self::Path(EntityPath::from(entity_path))
    }
}

impl<'a> From<&'a String> for EntityOrPath<'a> {
    #[inline]
    fn from(entity_path: &'a String) -> Self {
        Self::Path(EntityPath::from(entity_path))
    }
}

impl From<String> for EntityOrPath<'static> {
    #[inline]
    fn from(asset_path: String) -> Self {
        Self::Path(EntityPath::from(asset_path))
    }
}

/// A path to an entity.
pub struct EntityPath<'a>(Cow<'a, str>);

impl<'a> Default for EntityPath<'a> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<'a> From<&'a str> for EntityPath<'a> {
    #[inline]
    fn from(entity_path: &'a str) -> Self {
        EntityPath(Cow::Borrowed(entity_path))
    }
}

impl<'a> From<&'a String> for EntityPath<'a> {
    #[inline]
    fn from(entity_path: &'a String) -> Self {
        EntityPath(Cow::Borrowed(entity_path.as_str()))
    }
}

impl From<String> for EntityPath<'static> {
    #[inline]
    fn from(asset_path: String) -> Self {
        EntityPath(Cow::Owned(asset_path.into()))
    }
}

/// An [`Error`] that occurs when failing to resolve an [`EntityPath`].
#[derive(Error, Debug)]
pub enum ResolveEntityPathError {}

impl<'w> EntityWorldMut<'w> {
    /// Attempt to resolve the given `path` to an [`Entity`].
    pub fn resolve_path<'a>(
        &self,
        path: &EntityPath<'a>,
    ) -> Result<Entity, ResolveEntityPathError> {
        if !path.0.is_empty() {
            warn!("Resolving non-empty entity paths doesn't work yet!");
        }
        Ok(self.id())
    }
}
