use crate::{Asset, AssetDynamic};
use bevy_type_registry::{TypeUuid, Uuid};

/// A serializer for a given asset of type `T`
pub trait AssetSerializer: TypeUuid + Send + Sync + 'static {
    type Asset: Asset;
    fn serialize(&self, asset: &Self::Asset) -> Result<Vec<u8>, anyhow::Error>;
    fn extension(&self) -> &str;
}

pub trait AssetSerializerDynamic: Send + Sync + 'static {
    fn serialize_dyn(&self, asset: &dyn AssetDynamic) -> Result<Vec<u8>, anyhow::Error>;
    fn extension(&self) -> &str;
    fn asset_type_uuid(&self) -> Uuid;
}
impl<T: AssetSerializer> AssetSerializerDynamic for T {
    fn serialize_dyn(&self, asset: &dyn AssetDynamic) -> Result<Vec<u8>, anyhow::Error> {
        let asset_value = asset.downcast_ref::<T::Asset>().unwrap();
        self.serialize(asset_value)
    }

    fn extension(&self) -> &str {
        AssetSerializer::extension(self)
    }

    fn asset_type_uuid(&self) -> Uuid {
        T::Asset::TYPE_UUID
    }
}
