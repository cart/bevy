use crate::Asset;
use bevy_ecs::entity::Entity;
use bevy_reflect::Reflect;
use uuid::Uuid;

use core::{
    any::TypeId,
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
};
use derive_more::derive::From;
use thiserror::Error;

/// A unique runtime-only identifier for an [`Asset`]. This is cheap to [`Copy`]/[`Clone`] and is not directly tied to the
/// lifetime of the Asset. This means it _can_ point to an [`Asset`] that no longer exists.
///
/// For an identifier tied to the lifetime of an asset, see [`Handle`](`crate::Handle`).
///
/// For an "untyped" / "generic-less" id, see [`UntypedAssetId`].
#[derive(Reflect, From)]
#[reflect(Clone, Debug, PartialEq, Hash)]
pub struct AssetId<A: Asset> {
    /// The entity
    pub entity: Entity,
    /// A marker to store the type information of the asset.
    #[reflect(ignore, clone)]
    pub(crate) marker: PhantomData<fn() -> A>,
}

impl<A: Asset> AssetId<A> {
    /// The UUID for the default [`AssetId`]. It is valid to assign a value to this in [`Assets`](crate::Assets)
    /// and by convention (where appropriate) assets should support this pattern.
    #[deprecated(since = "0.20.0", note = "Use AssetReference::Default")]
    pub const DEFAULT_UUID: Uuid = Uuid::from_u128(200809721996911295814598172825939264631);

    /// This asset id _should_ never be valid. Assigning a value to this in [`Assets`](crate::Assets) will
    /// produce undefined behavior, so don't do it!
    #[deprecated(
        since = "0.20.0",
        note = "Use `Option<AssetId>` if possible. `AssetId::default` may also work, but note that the default can map to a valid asset."
    )]
    pub const INVALID_UUID: Uuid = Uuid::from_u128(108428345662029828789348721013522787528);

    #[inline]
    pub fn entity(&self) -> Entity {
        self.entity
    }
}

impl<A: Asset> Clone for AssetId<A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A: Asset> Copy for AssetId<A> {}

impl<A: Asset> Display for AssetId<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl<A: Asset> Debug for AssetId<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "AssetId<{}>{{ entity: {} }}",
            core::any::type_name::<A>(),
            self.entity
        )
    }
}

impl<A: Asset> Hash for AssetId<A> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.entity().hash(state);
    }
}

impl<A: Asset> PartialEq for AssetId<A> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.entity.eq(&other.entity)
    }
}

impl<A: Asset> Eq for AssetId<A> {}

impl<A: Asset> PartialOrd for AssetId<A> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<A: Asset> Ord for AssetId<A> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.entity.cmp(&other.entity)
    }
}

impl<A: Asset> Into<Entity> for &AssetId<A> {
    fn into(self) -> Entity {
        self.entity()
    }
}

impl<A: Asset> From<Entity> for AssetId<A> {
    #[inline]
    fn from(entity: Entity) -> Self {
        Self {
            entity,
            marker: PhantomData,
        }
    }
}

impl<A: Asset> Into<Entity> for AssetId<A> {
    #[inline]
    fn into(self) -> Entity {
        self.entity()
    }
}

/// Errors preventing the conversion of to/from an [`UntypedAssetId`] and an [`AssetId`].
#[derive(Error, Debug, PartialEq, Clone)]
#[non_exhaustive]
pub enum UntypedAssetIdConversionError {
    /// Caused when trying to convert an [`UntypedAssetId`] into an [`AssetId`] of the wrong type.
    #[error("This UntypedAssetId is for {found:?} and cannot be converted into an AssetId<{expected:?}>")]
    TypeIdMismatch {
        /// The [`TypeId`] of the asset that we are trying to convert to.
        expected: TypeId,
        /// The [`TypeId`] of the asset that we are trying to convert from.
        found: TypeId,
    },
}
