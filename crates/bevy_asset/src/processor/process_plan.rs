use crate::{
    io::Writer,
    meta::{AssetMeta, AssetMetaDyn, ProcessedLoader, ProcessorSettings, META_FORMAT_VERSION},
    saver::AssetSaver,
    AssetLoader, DeserializeMetaError, ErasedLoadedAsset,
};
use bevy_utils::BoxedFuture;
use std::{any::TypeId, marker::PhantomData};

pub struct AssetProcessPlan<
    Source: AssetLoader,
    Saver: AssetSaver<Asset = Source::Asset>,
    Destination: AssetLoader,
> {
    pub(crate) marker: PhantomData<(Source, Destination)>,
    pub(crate) saver: Saver,
}

impl<Source: AssetLoader, Saver: AssetSaver<Asset = Source::Asset>, Destination: AssetLoader>
    ErasedAssetProcessPlan for AssetProcessPlan<Source, Saver, Destination>
{
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a ErasedLoadedAsset,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
        let meta = asset.meta.as_ref().unwrap();
        let asset = asset.get::<Saver::Asset>().unwrap();
        self.saver.save(
            writer,
            asset,
            meta.saver_settings()
                .and_then(|s| s.downcast_ref::<Saver::Settings>())
                .expect("Processor should only run if saver settings exist"),
        )
    }

    fn deserialize_meta(&self, meta: &[u8]) -> Result<Box<dyn AssetMetaDyn>, DeserializeMetaError> {
        let meta: AssetMeta<Source, Saver, Destination> = ron::de::from_bytes(meta)?;
        Ok(Box::new(meta))
    }

    fn source_loader(&self) -> &'static str {
        std::any::type_name::<Source>()
    }

    fn default_meta(&self) -> Box<dyn AssetMetaDyn> {
        Box::new(AssetMeta::<Source, Saver, Destination> {
            meta_format_version: META_FORMAT_VERSION.to_string(),
            processed_info: None,
            loader_settings: Source::Settings::default(),
            loader: std::any::type_name::<Source>().to_string(),
            processor: Some(ProcessorSettings {
                saver: std::any::type_name::<Saver>().to_string(),
                saver_settings: Saver::Settings::default(),
                loader: if TypeId::of::<Source>() == TypeId::of::<Destination>() {
                    ProcessedLoader::UseSourceLoader
                } else {
                    ProcessedLoader::Loader {
                        loader: std::any::type_name::<Destination>().to_string(),
                        settings: Destination::Settings::default(),
                    }
                },
            }),
        })
    }
}

pub trait ErasedAssetProcessPlan: Send + Sync {
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a ErasedLoadedAsset,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>>;
    fn deserialize_meta(&self, meta: &[u8]) -> Result<Box<dyn AssetMetaDyn>, DeserializeMetaError>;
    fn default_meta(&self) -> Box<dyn AssetMetaDyn>;
    fn source_loader(&self) -> &'static str;
}
