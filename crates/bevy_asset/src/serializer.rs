use bevy_type_registry::TypeUuid;

use crate::Asset;

/// A loader for a given asset of type `T`
pub trait AssetSerializer<T: Asset>: TypeUuid + Send + Sync + 'static {
    fn serialize(&self, value: &T, bytes: Vec<u8>) -> Result<(), anyhow::Error>;
}