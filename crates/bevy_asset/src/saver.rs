use crate as bevy_asset;
use crate::{io::Writer, meta::Settings, Asset, ErasedLoadedAsset};
use bevy_utils::BoxedFuture;
use serde::{Deserialize, Serialize};

pub trait AssetSaver: Send + Sync + 'static {
    type Asset: Asset;
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
        asset: &'a ErasedLoadedAsset,
        settings: &'a dyn Settings,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>>;
    fn extension(&self) -> &'static str;
    fn type_name(&self) -> &'static str;
}

impl<S: AssetSaver> ErasedAssetSaver for S {
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a ErasedLoadedAsset,
        settings: &'a dyn Settings,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
        Box::pin(async move {
            let settings = settings
                .downcast_ref::<S::Settings>()
                .expect("AssetLoader settings should match the loader type");
            let asset = asset.get::<S::Asset>().unwrap();
            self.save(writer, asset, settings).await?;
            Ok(())
        })
    }
    fn extension(&self) -> &'static str {
        S::extension(&self)
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<S>()
    }
}

pub struct NullSaver;

#[derive(Asset)]
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
