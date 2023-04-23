// Is this actually useful? Can't use it for things like "processed asset redirects" because the output type will be different
// Could be useful if you really do want a different asset type

use bevy_ecs::prelude::Component;
use bevy_utils::BoxedFuture;
use serde::{Deserialize, Serialize};

use crate::{io::Writer, loader::LoadedAsset, meta::Settings};

pub trait AssetSaver: Send + Sync + 'static {
    type Asset: Component;
    type Settings: Settings + Default + Serialize + for<'a> Deserialize<'a>;

    fn save<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a Self::Asset,
        settings: &'a Self::Settings,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>>;

    fn extension(&self) -> &'static str;
}

pub trait ErasedAssetSaver: Send + Sync + 'static {
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a LoadedAsset,
        settings: &'a dyn Settings,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>>;
    fn extension(&self) -> &'static str;
    fn type_name(&self) -> &'static str;
}

impl<T: AssetSaver> ErasedAssetSaver for T {
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a LoadedAsset,
        settings: &'a dyn Settings,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
        Box::pin(async move {
            let settings = settings
                .downcast_ref::<T::Settings>()
                .expect("AssetLoader settings should match the loader type");
            let asset = asset.get::<T::Asset>().unwrap();
            self.save(writer, asset, settings).await?;
            Ok(())
        })
    }
    fn extension(&self) -> &'static str {
        T::extension(&self)
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

pub struct NullSaver;

#[derive(Component)]
pub struct NullAsset;

impl AssetSaver for NullSaver {
    type Asset = NullAsset;

    type Settings = ();

    fn save<'a>(
        &'a self,
        _writer: &'a mut Writer,
        _asset: &'a Self::Asset,
        _settings: &'a Self::Settings,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
        panic!("NullSaver should never be called");
    }

    fn extension(&self) -> &'static str {
        panic!("NullSaver should never be called");
    }
}
