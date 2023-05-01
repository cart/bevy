use crate::{
    io::{AssetProvider, AssetProviders, AssetReader, AssetReaderError, AssetWriter, Writer},
    loader::{AssetLoader, DeserializeMetaError, ErasedAssetLoader},
    meta::{
        AssetMeta, AssetMetaDyn, AssetMetaMinimal, AssetMetaProcessedInfoMinimal, ProcessedInfo,
        ProcessedLoader, ProcessorSettings, META_FORMAT_VERSION,
    },
    saver::AssetSaver,
    AssetPath, AssetServer, ErasedLoadedAsset,
};
use async_broadcast::{Receiver, Sender};
use bevy_app::{App, Plugin};
use bevy_ecs::system::Resource;
use bevy_log::{error, trace};
use bevy_tasks::IoTaskPool;
use bevy_utils::{BoxedFuture, HashMap, HashSet};
use futures_lite::{AsyncReadExt, AsyncWriteExt, StreamExt};
use parking_lot::RwLock;
use std::{
    any::TypeId,
    hash::{Hash, Hasher},
    marker::PhantomData,
    path::Path,
    sync::Arc,
};
use thiserror::Error;

#[derive(Default)]
pub struct AssetProcessorPlugin {
    pub source: AssetProvider,
    pub destination: AssetProvider,
}

impl Plugin for AssetProcessorPlugin {
    fn build(&self, app: &mut App) {
        let (source_reader, source_writer, destination_reader, destination_writer) = {
            let mut providers = app.world.resource_mut::<AssetProviders>();
            let source_reader = providers.get_source_reader(&self.source);
            let source_writer = providers.get_source_writer(&self.source);
            let destination_reader = providers.get_destination_reader(&self.destination);
            let destination_writer = providers.get_destination_writer(&self.destination);
            (
                source_reader,
                source_writer,
                destination_reader,
                destination_writer,
            )
        };
        // The asset processor uses its own asset server with its own id space
        let asset_server = AssetServer::new(source_reader);
        let processor = AssetProcessor::new(
            asset_server,
            source_writer,
            destination_reader,
            destination_writer,
        );
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

struct ProcessorAssetInfo {
    processed_info: Option<ProcessedInfo>,
    dependants: HashSet<AssetPath<'static>>,
}

pub trait ErasedAssetProcessPlan: Send + Sync {
    fn process<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a ErasedLoadedAsset,
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
    asset_infos: async_lock::RwLock<HashMap<AssetPath<'static>, ProcessorAssetInfo>>,
    source_writer: Box<dyn AssetWriter>,
    destination_reader: Box<dyn AssetReader>,
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
        destination_reader: Box<dyn AssetReader>,
        destination_writer: Box<dyn AssetWriter>,
    ) -> Self {
        let (finished_sender, finished_receiver) = async_broadcast::broadcast(1);
        Self {
            data: Arc::new(AssetProcessorData {
                assets,
                source_writer,
                destination_reader,
                destination_writer,
                finished_sender,
                finished_receiver,
                state: async_lock::RwLock::new(ProcessorState::Scanning),
                process_plans: Default::default(),
                asset_infos: Default::default(),
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

    /// Populates the current view of each processed asset's [`ProcessedInfo`] from the processed "destination".
    /// This info will later be used to determine whether or not to re-process an asset
    /// Under normal circumstances, this should always succeed. But if it fails the path of the failed
    pub async fn populate_processed_info(&self) -> Result<(), PopulateProcessedInfoError> {
        let base_path = Path::new("");
        let reader = &*self.data.destination_reader;
        let mut path_stream = reader
            .read_directory(base_path)
            .await
            .map_err(PopulateProcessedInfoError::FailedToReadDirectory)?;
        let mut asset_infos = self.data.asset_infos.write().await;
        // PERF: parallelize this and see what kind of wins we get?
        while let Some(path) = path_stream.next().await {
            let mut meta_reader = reader
                .read_meta(&path)
                .await
                // TODO: this is probably recoverable in some cases
                .map_err(PopulateProcessedInfoError::FailedToReadPath)?;
            let mut meta_bytes = Vec::new();
            meta_reader.read_to_end(&mut meta_bytes).await.unwrap();
            let minimal: AssetMetaProcessedInfoMinimal = ron::de::from_bytes(&meta_bytes).unwrap();
            trace!(
                "Populated processed info for asset {path:?} {:?}",
                minimal.processed_info
            );

            let path = AssetPath::new(path, None);

            if let Some(processed_info) = &minimal.processed_info {
                for load_dependency_info in &processed_info.load_dependencies {
                    let load_dependency_path = AssetPath::from(&load_dependency_info.path);
                    let dependency_info = asset_infos
                        .entry(load_dependency_path.to_owned())
                        .or_insert_with(|| ProcessorAssetInfo {
                            processed_info: None,
                            dependants: Default::default(),
                        });
                    dependency_info.dependants.insert(path.to_owned());
                }
            }

            let asset_info = ProcessorAssetInfo {
                processed_info: minimal.processed_info,
                dependants: Default::default(),
            };
            asset_infos.insert(path, asset_info);
        }

        Ok(())
    }
    pub async fn process_asset(&self, path: &Path) {
        trace!("Processing asset {:?}", path);
        let assets = &self.data.assets;
        let (source_meta, meta_bytes, process_plan) = match assets.reader().read_meta(&path).await {
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
                (meta, meta_bytes, process_plan)
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
                (meta, meta_bytes, default_process_plan)
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
        let asset_path = AssetPath::new(path.to_owned(), None);
        if let Some(process_plan) = process_plan {
            trace!("Loading asset directly in order to process it {:?}", path);
            let mut reader = assets.reader().read(&path).await.unwrap();
            let mut asset_bytes = Vec::new();
            reader.read_to_end(&mut asset_bytes).await.unwrap();
            // TODO: this isn't quite ready yet
            // let asset_hash = Self::get_asset_hash(&asset_bytes);
            // PERF: in theory these hashes could be streamed if we want to avoid allocating the whole asset.
            // The downside is that reading assets would need to happen twice (once for the hash and once for the asset loader)
            // Hard to say which is worse
            // let hash = Self::get_full_hash(&meta_bytes, asset_hash);
            // {
            //     let infos = self.data.asset_infos.read().await;
            //     if let Some(asset_info) = infos.get(&asset_path) {
            //         // TODO:  check timestamp first for early-out
            //         if let Some(processed_info) = &asset_info.processed_info {
            //             if processed_info.hash == hash {
            //                 trace!(
            //                     "Skipping processing of asset {:?} because it has not changed",
            //                     asset_path
            //                 );
            //                 return;
            //             }
            //         }
            //     }
            // }

            let mut writer = self.destination_writer().write(&path).await.unwrap();
            let mut meta_writer = self.destination_writer().write_meta(&path).await.unwrap();

            match assets
                .load_with_meta_and_reader(
                    asset_path,
                    source_meta,
                    &mut asset_bytes.as_slice(),
                    false,
                )
                .await
            {
                Ok(loaded_asset) => {
                    // TODO: error handling
                    process_plan
                        .process(&mut writer, &loaded_asset)
                        .await
                        .unwrap();
                    writer.flush().await.unwrap();
                    let meta = loaded_asset.meta.unwrap().into_processed().unwrap();
                    let meta_bytes = meta.serialize();
                    meta_writer.write_all(&meta_bytes).await.unwrap();
                    meta_writer.flush().await.unwrap();
                }
                Err(err) => error!("Failed to process asset due to load error: {}", err),
            }
        } else {
            // TODO: make sure that if this asset was previously "processed", that this state transition is correct
            // Specifically, how will this affect other assets currently being processed?
            {
                let mut info = self.data.asset_infos.write().await;
                info.remove(&asset_path);
            }
            // PERF: this could be streamed instead of using the full intermediate buffer
            let mut reader = assets.reader().read(&path).await.unwrap();
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await.unwrap();

            let mut writer = self.destination_writer().write(&path).await.unwrap();
            let mut meta_writer = self.destination_writer().write_meta(&path).await.unwrap();
            writer.write_all(&bytes).await.unwrap();
            writer.flush().await.unwrap();

            let meta_bytes = source_meta.serialize();
            meta_writer.write_all(&meta_bytes).await.unwrap();
            meta_writer.flush().await.unwrap();
        }

        trace!("Finished processing asset {:?}", path);
    }

    /// NOTE: changing the hashing logic here is a _breaking change_ that requires a [`META_FORMAT_VERSION`] bump.
    fn get_full_hash(meta_bytes: &[u8], asset_hash: u64) -> u64 {
        let mut hasher = Self::get_hasher();
        meta_bytes.hash(&mut hasher);
        asset_hash.hash(&mut hasher);
        hasher.finish()
    }

    /// NOTE: changing the hashing logic here is a _breaking change_ that requires a [`META_FORMAT_VERSION`] bump.
    pub(crate) fn get_asset_hash(asset_bytes: &[u8]) -> u64 {
        let mut hasher = Self::get_hasher();
        asset_bytes.hash(&mut hasher);
        hasher.finish()
    }

    /// NOTE: changing the hashing logic here is a _breaking change_ that requires a [`META_FORMAT_VERSION`] bump.
    fn get_hasher() -> bevy_utils::AHasher {
        bevy_utils::AHasher::new_with_keys(
            315266772046776459041028670939089038334,
            325180381366804243855319169815293592503,
        )
    }

    pub fn process_assets(&self) {
        let assets = &self.data.assets;
        IoTaskPool::get().scope(|scope| {
            scope.spawn(async move {
                self.populate_processed_info().await.unwrap();
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

#[derive(Error, Debug)]
pub enum PopulateProcessedInfoError {
    #[error(transparent)]
    FailedToReadDirectory(AssetReaderError),
    #[error(transparent)]
    FailedToReadPath(AssetReaderError),
}
