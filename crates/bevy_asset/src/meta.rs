use downcast_rs::{impl_downcast, Downcast};
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::{
    loader::AssetLoader,
    saver::{AssetSaver, NullSaver},
};

#[derive(Serialize, Deserialize)]
pub struct AssetMeta<Source: AssetLoader, Saver: AssetSaver, Destination: AssetLoader> {
    pub meta_format_version: String,
    pub loader: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processor: Option<ProcessorMeta<Saver::Settings, Destination::Settings>>,
    pub loader_settings: Source::Settings,
}

pub const META_FORMAT_VERSION: &str = "1.0";

#[derive(Serialize, Deserialize)]
pub struct AssetMetaMinimal {
    pub loader: String,
    pub processor: Option<ProcessorMetaMinimal>,
}

impl AssetMetaMinimal {
    pub fn destination_loader(&self) -> Option<&String> {
        self.processor.as_ref().map(|p| match &p.loader {
            ProcessorLoaderMetaMinimal::UseSourceLoader => &self.loader,
            ProcessorLoaderMetaMinimal::Loader { loader } => loader,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct ProcessorMeta<SaverSettings: Settings, DestinationSettings: Settings> {
    pub saver: String,
    pub saver_settings: SaverSettings,
    pub loader: ProcessorLoaderMeta<DestinationSettings>,
}

#[derive(Serialize, Deserialize, Default)]
pub enum ProcessorLoaderMeta<S: Settings> {
    #[default]
    UseSourceLoader,
    Loader {
        loader: String,
        settings: S,
    },
}

#[derive(Serialize, Deserialize)]
pub struct ProcessorMetaMinimal {
    pub saver: String,
    pub loader: ProcessorLoaderMetaMinimal,
}

#[derive(Serialize, Deserialize, Default)]
pub enum ProcessorLoaderMetaMinimal {
    #[default]
    UseSourceLoader,
    Loader {
        loader: String,
    },
}

pub trait AssetMetaDyn: Downcast + Send + Sync {
    fn source_loader(&self) -> &String;
    fn source_loader_settings(&self) -> &dyn Settings;
    fn saver(&self) -> Option<&String>;
    fn saver_settings(&self) -> Option<&dyn Settings>;
    fn destination_loader(&self) -> Option<&String>;
    fn destination_loader_settings(&self) -> Option<&dyn Settings>;
    /// Converts this metadata into its "processed" form, which shifts the "Destination"
    /// loader into the source loader and removes the processing configuration.
    /// Returns Some if the conversion was successful. This will return None if the processor
    /// was configured with ProcessorLoaderMeta::UseSourceLoader, but the Source and Destination
    /// loader types don't match.
    fn get_processed_meta(self: Box<Self>) -> Option<Box<dyn AssetMetaDyn>>;
    fn serialize(&self) -> Vec<u8>;
}

impl<Source: AssetLoader, Saver: AssetSaver, Destination: AssetLoader> AssetMetaDyn
    for AssetMeta<Source, Saver, Destination>
where
    Saver::Settings: 'static,
{
    fn serialize(&self) -> Vec<u8> {
        ron::ser::to_string_pretty(&self, PrettyConfig::default())
            .expect("type is convertible to ron")
            .into_bytes()
    }

    fn source_loader(&self) -> &String {
        &self.loader
    }

    fn source_loader_settings(&self) -> &dyn Settings {
        &self.loader_settings
    }

    fn saver(&self) -> Option<&String> {
        self.processor.as_ref().map(|p| &p.saver)
    }

    fn saver_settings(&self) -> Option<&dyn Settings> {
        match &self.processor {
            Some(p) => Some(&p.saver_settings),
            None => None,
        }
    }

    fn destination_loader(&self) -> Option<&String> {
        self.processor.as_ref().map(|p| match &p.loader {
            ProcessorLoaderMeta::UseSourceLoader => &self.loader,
            ProcessorLoaderMeta::Loader { loader, .. } => loader,
        })
    }

    fn destination_loader_settings(&self) -> Option<&dyn Settings> {
        match &self.processor {
            Some(p) => Some(match &p.loader {
                ProcessorLoaderMeta::UseSourceLoader => &self.loader_settings,
                ProcessorLoaderMeta::Loader { settings, .. } => settings,
            }),
            None => None,
        }
    }

    fn get_processed_meta(self: Box<Self>) -> Option<Box<dyn AssetMetaDyn>> {
        if let Some(processor) = self.processor {
            let (loader, loader_settings) = match processor.loader {
                ProcessorLoaderMeta::UseSourceLoader => {
                    let settings: Box<dyn Settings> = Box::new(self.loader_settings);
                    (
                        self.loader,
                        *settings.downcast::<Destination::Settings>().ok()?,
                    )
                }
                ProcessorLoaderMeta::Loader { loader, settings } => (loader, settings),
            };
            Some(Box::new(AssetMeta::<Destination, NullSaver, Destination> {
                processor: None,
                meta_format_version: self.meta_format_version,
                loader,
                loader_settings,
            }))
        } else {
            Some(self)
        }
    }
}

impl_downcast!(AssetMetaDyn);

pub trait Settings: Downcast + Send + Sync {}

impl<T: 'static> Settings for T where T: Send + Sync {}

impl_downcast!(Settings);
