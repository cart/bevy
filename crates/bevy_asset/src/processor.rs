use crate::{
    io::{AssetReaderError, AssetWriter, Writer},
    loader::{AssetLoader, DeserializeMetaError, ErasedAssetLoader},
    meta::{
        AssetMeta, AssetMetaDyn, AssetMetaMinimal, ProcessorLoaderMeta, ProcessorMeta,
        META_FORMAT_VERSION,
    },
    saver::AssetSaver,
    AssetProvider, AssetProviders, AssetServer, ErasedLoadedAsset,
};
use async_broadcast::{Receiver, Sender};
use bevy_app::{App, Plugin};
use bevy_ecs::system::Resource;
use bevy_log::{error, trace};
use bevy_tasks::IoTaskPool;
use bevy_utils::{BoxedFuture, HashMap};
use futures_lite::{AsyncReadExt, AsyncWriteExt, StreamExt};
use parking_lot::RwLock;
use std::{any::TypeId, marker::PhantomData, path::Path, sync::Arc};

#[derive(Default)]
pub struct AssetProcessorPlugin {
    pub source: AssetProvider,
    pub destination: AssetProvider,
}

impl Plugin for AssetProcessorPlugin {
    fn build(&self, app: &mut App) {
        let (source_reader, source_writer, destination_writer) = {
            let mut providers = app.world.resource_mut::<AssetProviders>();
            let source_reader = providers.get_source_reader(&self.source);
            let source_writer = providers.get_source_writer(&self.source);
            let destination_writer = providers.get_destination_writer(&self.destination);
            (source_reader, source_writer, destination_writer)
        };
        // The asset processor uses its own asset server with its own id space
        let asset_server = AssetServer::new(source_reader);
        let processor = AssetProcessor::new(asset_server, source_writer, destination_writer);
        app.insert_resource(processor.clone());
        std::thread::spawn(move || {
            processor.process_assets();
        });
    }
}

pub struct AssetProcessPlan<
    Source: AssetLoader,
    Saver: AssetSaver<Asset = Source::Asset>,
    Destination: AssetLoader,
> {
    marker: PhantomData<(Source, Destination)>,
    saver: Saver,
}

// #[derive(Error, Debug)]
// pub enum AssetProcessError {
//     #[error(transparent)]
//     FailedSave(#[from] AssetSaveError),
// }

impl<Source: AssetLoader, Saver: AssetSaver<Asset = Source::Asset>, Destination: AssetLoader>
    ErasedAssetProcessPlan for AssetProcessPlan<Source, Saver, Destination>
{
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a ErasedLoadedAsset,
        meta: &'a dyn AssetMetaDyn,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {
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

    fn default_meta(&self) -> Box<dyn AssetMetaDyn> {
        Box::new(AssetMeta::<Source, Saver, Destination> {
            meta_format_version: META_FORMAT_VERSION.to_string(),
            loader_settings: Source::Settings::default(),
            loader: std::any::type_name::<Source>().to_string(),
            processor: Some(ProcessorMeta {
                saver: std::any::type_name::<Saver>().to_string(),
                saver_settings: Saver::Settings::default(),
                loader: if TypeId::of::<Source>() == TypeId::of::<Destination>() {
                    ProcessorLoaderMeta::UseSourceLoader
                } else {
                    ProcessorLoaderMeta::Loader {
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
        meta: &'a dyn AssetMetaDyn,
    ) -> BoxedFuture<'a, Result<(), anyhow::Error>>;
    fn deserialize_meta(&self, meta: &[u8]) -> Result<Box<dyn AssetMetaDyn>, DeserializeMetaError>;
    fn default_meta(&self) -> Box<dyn AssetMetaDyn>;
}

pub struct AssetProcessorData {
    assets: AssetServer,
    process_plans: RwLock<
        HashMap<(&'static str, &'static str, &'static str), Arc<dyn ErasedAssetProcessPlan>>,
    >,
    default_process_plans:
        RwLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>>,
    state: async_lock::RwLock<ProcessorState>,
    source_writer: Box<dyn AssetWriter>,
    destination_writer: Box<dyn AssetWriter>,
    finished_sender: Sender<()>,
    finished_receiver: Receiver<()>,
}

#[derive(Resource, Clone)]
pub struct AssetProcessor {
    data: Arc<AssetProcessorData>,
}

impl AssetProcessor {
    pub fn new(
        assets: AssetServer,
        source_writer: Box<dyn AssetWriter>,
        destination_writer: Box<dyn AssetWriter>,
    ) -> Self {
        let (finished_sender, finished_receiver) = async_broadcast::broadcast(1);
        Self {
            data: Arc::new(AssetProcessorData {
                assets,
                source_writer,
                destination_writer,
                finished_sender,
                finished_receiver,
                state: async_lock::RwLock::new(ProcessorState::Scanning),
                process_plans: Default::default(),
                default_process_plans: Default::default(),
            }),
        }
    }

    async fn set_state(&self, state: ProcessorState) {
        let mut state_guard = self.data.state.write().await;
        let last_state = *state_guard;
        *state_guard = state;
        if last_state != ProcessorState::Finished && state == ProcessorState::Finished {
            self.data.finished_sender.broadcast(()).await.unwrap();
        }
    }

    pub async fn wait_until_finished(&self) {
        let receiver = {
            let state = self.data.state.read().await;
            match *state {
                ProcessorState::Scanning | ProcessorState::Processing => {
                    Some(self.data.finished_receiver.clone())
                }
                ProcessorState::Finished => None,
            }
        };

        if let Some(mut receiver) = receiver {
            receiver.recv().await.unwrap()
        }
    }

    pub async fn get_state(&self) -> ProcessorState {
        *self.data.state.read().await
    }

    pub fn assets(&self) -> &AssetServer {
        &self.data.assets
    }

    pub fn source_writer(&self) -> &dyn AssetWriter {
        &*self.data.source_writer
    }

    pub fn destination_writer(&self) -> &dyn AssetWriter {
        &*self.data.destination_writer
    }

    pub fn register_process_plan<
        Source: AssetLoader,
        Saver: AssetSaver<Asset = Source::Asset>,
        Destination: AssetLoader,
    >(
        &self,
        saver: Saver,
    ) {
        let mut process_plans = self.data.process_plans.write();
        let process_plan_key = (
            std::any::type_name::<Source>(),
            std::any::type_name::<Saver>(),
            std::any::type_name::<Destination>(),
        );
        process_plans.insert(
            process_plan_key,
            Arc::new(AssetProcessPlan::<Source, Saver, Destination> {
                saver,
                marker: PhantomData,
            }),
        );
        let mut default_process_plans = self.data.default_process_plans.write();
        default_process_plans
            .entry(std::any::type_name::<Source>())
            .or_insert_with(|| process_plan_key);
    }

    // TODO: can this just be a type id?
    pub fn get_default_process_plan(
        &self,
        loader: &str,
    ) -> Option<Arc<dyn ErasedAssetProcessPlan>> {
        let default_plans = self.data.default_process_plans.read();
        let key = default_plans.get(&loader)?;
        self.data.process_plans.read().get(key).cloned()
    }

    pub fn get_process_plan(
        &self,
        source_loader: &str,
        saver: &str,
        destination_loader: &str,
    ) -> Option<Arc<dyn ErasedAssetProcessPlan>> {
        self.data
            .process_plans
            .read()
            .get(&(source_loader, saver, destination_loader))
            .cloned()
    }

    pub async fn process_asset(&self, path: &Path) {
        trace!("Processing asset {:?}", path);
        let assets = &self.data.assets;
        let (source_meta, process_plan) = match assets.reader().read_meta(&path).await {
            Ok(mut meta_reader) => {
                let mut meta_bytes = Vec::new();
                // TODO: handle error
                meta_reader.read_to_end(&mut meta_bytes).await.unwrap();
                let minimal: AssetMetaMinimal = ron::de::from_bytes(&meta_bytes).unwrap();
                let process_plan = minimal.processor.as_ref().and_then(|p| {
                    self.get_process_plan(
                        &minimal.loader,
                        &p.saver,
                        minimal.destination_loader().unwrap(),
                    )
                });
                let meta = process_plan
                    .as_ref()
                    .map(|p| p.deserialize_meta(&meta_bytes).unwrap())
                    .unwrap_or_else(|| {
                        assets
                            .get_erased_asset_loader_with_type_name(&minimal.loader)
                            .unwrap()
                            .deserialize_meta(&meta_bytes)
                            .unwrap()
                    });
                (meta, process_plan)
            }
            Err(AssetReaderError::NotFound(_path)) => {
                let Ok(loader) = assets.get_path_asset_loader(&path) else {
                        trace!("No asset loader set up for {:?}", path);
                        return;
                    };
                let default_process_plan =
                    self.get_default_process_plan(ErasedAssetLoader::type_name(&*loader));
                let meta = default_process_plan
                    .as_ref()
                    .map(|p| p.default_meta())
                    .unwrap_or_else(|| loader.default_meta());
                let meta_bytes = meta.serialize();
                // write meta to source location if it doesn't already exist
                match self.source_writer().write_meta(&path).await {
                    Ok(mut meta_writer) => {
                        // TODO: handle error
                        meta_writer.write_all(&meta_bytes).await.unwrap();
                        meta_writer.flush().await.unwrap();
                    }
                    Err(err) => {
                        error!(
                            "Encountered an error while writing meta for {:?} {:?}",
                            path, err
                        );

                        return;
                    }
                }
                (meta, default_process_plan)
            }
            Err(err) => {
                error!(
                    "Encountered an error while loading meta for {:?}: {:?}",
                    path, err
                );
                return;
            }
        };

        // TODO: error handling
        let mut writer = self.destination_writer().write(&path).await.unwrap();
        let mut meta_writer = self.destination_writer().write_meta(&path).await.unwrap();
        if let Some(process_plan) = process_plan {
            trace!("Loading asset directly in order to process it {:?}", path);
            match assets
                .load_direct_with_meta_async(path, &*source_meta)
                .await
            {
                Ok(loaded_asset) => {
                    // TODO: error handling
                    process_plan
                        .process(&mut writer, &loaded_asset, &*source_meta)
                        .await
                        .unwrap();
                    writer.flush().await.unwrap();

                    let meta = source_meta.get_processed_meta().unwrap();
                    let meta_bytes = meta.serialize();
                    meta_writer.write_all(&meta_bytes).await.unwrap();
                    meta_writer.flush().await.unwrap();
                }
                Err(err) => error!("Failed to process asset due to load error: {}", err),
            }
        } else {
            // TODO: this could be streamed instead of using the full intermediate buffer
            let mut reader = assets.reader().read(&path).await.unwrap();
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await.unwrap();
            writer.write_all(&bytes).await.unwrap();
            writer.flush().await.unwrap();

            let meta_bytes = source_meta.serialize();
            meta_writer.write_all(&meta_bytes).await.unwrap();
            meta_writer.flush().await.unwrap();
        }

        trace!("Finished processing asset {:?}", path);
    }

    pub fn process_assets(&self) {
        let assets = &self.data.assets;
        IoTaskPool::get().scope(|scope| {
            scope.spawn(async move {
                let path = Path::new("");
                let mut path_stream = assets.reader().read_directory(path).await.unwrap();
                while let Some(path) = path_stream.next().await {
                    let processor = self.clone();
                    scope.spawn(async move {
                        processor.process_asset(&path).await;
                    });
                }
                // clean up metadata
                assets.data.infos.write().consume_handle_drop_events();
            });
        });
        IoTaskPool::get().scope(|scope| {
            scope.spawn(async move {
                self.set_state(ProcessorState::Finished).await;
                trace!("finished processing");
            })
        });
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ProcessorState {
    Scanning,
    Processing,
    Finished,
}
