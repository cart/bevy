use crate::{
    io::{
        processor_gated::ProcessorGatedReader, AssetProvider, AssetProviders, AssetReader,
        AssetReaderError, AssetWriter, AssetWriterError, Writer,
    },
    loader::{AssetLoader, DeserializeMetaError, ErasedAssetLoader},
    meta::{
        AssetMeta, AssetMetaDyn, AssetMetaMinimal, AssetMetaProcessedInfoMinimal,
        LoadDependencyInfo, ProcessedInfo, ProcessedLoader, ProcessorSettings, META_FORMAT_VERSION,
    },
    saver::AssetSaver,
    AssetLoadError, AssetPath, AssetServer, ErasedLoadedAsset, MissingAssetLoaderForExtensionError,
};
use async_broadcast::{Receiver, Sender};
use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::*;
use bevy_log::{error, trace};
use bevy_tasks::{IoTaskPool, Scope};
use bevy_utils::{BoxedFuture, HashMap, HashSet};
use futures_lite::{AsyncReadExt, AsyncWriteExt, FutureExt, StreamExt};
use parking_lot::RwLock;
use std::{
    any::TypeId,
    hash::{Hash, Hasher},
    marker::PhantomData,
    path::{Path, PathBuf},
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
        let processor = {
            let mut providers = app.world.resource_mut::<AssetProviders>();
            let source_reader = providers.get_source_reader(&self.source);
            let source_writer = providers.get_source_writer(&self.source);
            let destination_reader = providers.get_destination_reader(&self.destination);
            let destination_writer = providers.get_destination_writer(&self.destination);
            // The asset processor uses its own asset server with its own id space
            let data = Arc::new(AssetProcessorData::new(
                source_reader,
                source_writer,
                destination_reader,
                destination_writer,
            ));
            let destination_reader = providers.get_destination_reader(&self.destination);
            let asset_server = AssetServer::new(Box::new(ProcessorGatedReader::new(
                destination_reader,
                data.clone(),
            )));
            AssetProcessor::new(asset_server, data)
        };
        app.insert_resource(processor)
            .add_systems(Startup, start_processor);
    }
}

pub fn start_processor(processor: Res<AssetProcessor>) {
    let processor = processor.clone();
    std::thread::spawn(move || {
        processor.process_assets();
    });
}

pub struct AssetProcessPlan<
    Source: AssetLoader,
    Saver: AssetSaver<Asset = Source::Asset>,
    Destination: AssetLoader,
> {
    marker: PhantomData<(Source, Destination)>,
    saver: Saver,
}

#[derive(Error, Debug)]
pub enum ProcessAssetError {
    #[error(transparent)]
    MissingAssetLoaderForExtension(#[from] MissingAssetLoaderForExtensionError),
    #[error(transparent)]
    AssetWriterError(#[from] AssetWriterError),
    #[error("Failed to read asset metadata {0:?}")]
    ReadAssetMetaError(AssetReaderError),
    #[error(transparent)]
    AssetLoadError(#[from] AssetLoadError),
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

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ProcessResult {
    Processed,
    SkippedNotChanged,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ProcessStatus {
    Processed,
    Failed,
    NonExistent,
}

struct ProcessorAssetInfo {
    processed_info: Option<ProcessedInfo>,
    dependants: HashSet<AssetPath<'static>>,
    status: Option<ProcessStatus>,
    status_sender: Sender<ProcessStatus>,
    status_receiver: Receiver<ProcessStatus>,
}

impl Default for ProcessorAssetInfo {
    fn default() -> Self {
        let (status_sender, status_receiver) = async_broadcast::broadcast(1);
        Self {
            processed_info: Default::default(),
            dependants: Default::default(),
            status: None,
            status_sender,
            status_receiver,
        }
    }
}

impl ProcessorAssetInfo {
    async fn update_status(&mut self, status: ProcessStatus) {
        if self.status != Some(status) {
            self.status = Some(status);
            self.status_sender.broadcast(status).await.unwrap();
        }
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
}

#[derive(Default)]
pub struct ProcessorAssetInfos {
    infos: HashMap<AssetPath<'static>, ProcessorAssetInfo>,
    maybe_non_existent: HashSet<AssetPath<'static>>,
}

impl ProcessorAssetInfos {
    fn get_or_insert(
        &mut self,
        asset_path: AssetPath<'static>,
        definitely_exists: bool,
    ) -> &mut ProcessorAssetInfo {
        match self.infos.entry(asset_path.clone()) {
            bevy_utils::hashbrown::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            bevy_utils::hashbrown::hash_map::Entry::Vacant(entry) => {
                if !definitely_exists {
                    self.maybe_non_existent.insert(asset_path);
                }
                entry.insert(ProcessorAssetInfo::default())
            }
        }
    }

    fn get(&self, asset_path: &AssetPath<'static>) -> Option<&ProcessorAssetInfo> {
        self.infos.get(asset_path)
    }

    // Remove the info for the given path. This should only happen if an asset's source is removed / non-existent
    async fn remove(&mut self, asset_path: &AssetPath<'static>) {
        let info = self.infos.remove(asset_path);
        if let Some(info) = info {
            // Tell all listeners this asset does not exist
            info.status_sender
                .broadcast(ProcessStatus::NonExistent)
                .await
                .unwrap();
        }
    }

    async fn resolve_maybe_non_existent(&mut self) {
        for path in self.maybe_non_existent.drain() {
            match self.infos.entry(path) {
                bevy_utils::hashbrown::hash_map::Entry::Occupied(entry) => {
                    if entry.get().status.is_none() {
                        let info = entry.remove();
                        info.status_sender
                            .broadcast(ProcessStatus::NonExistent)
                            .await
                            .unwrap();
                    }
                }
                _ => {}
            }
        }
    }
}

pub struct AssetProcessorData {
    process_plans: RwLock<
        HashMap<(&'static str, &'static str, &'static str), Arc<dyn ErasedAssetProcessPlan>>,
    >,
    default_process_plans:
        RwLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>>,
    state: async_lock::RwLock<ProcessorState>,
    asset_infos: async_lock::RwLock<ProcessorAssetInfos>,
    source_reader: Box<dyn AssetReader>,
    source_writer: Box<dyn AssetWriter>,
    destination_reader: Box<dyn AssetReader>,
    destination_writer: Box<dyn AssetWriter>,
    finished_sender: Sender<()>,
    finished_receiver: Receiver<()>,
}

impl AssetProcessorData {
    pub fn new(
        source_reader: Box<dyn AssetReader>,
        source_writer: Box<dyn AssetWriter>,
        destination_reader: Box<dyn AssetReader>,
        destination_writer: Box<dyn AssetWriter>,
    ) -> Self {
        let (finished_sender, finished_receiver) = async_broadcast::broadcast(1);
        AssetProcessorData {
            source_reader,
            source_writer,
            destination_reader,
            destination_writer,
            finished_sender,
            finished_receiver,
            state: async_lock::RwLock::new(ProcessorState::Scanning),
            process_plans: Default::default(),
            asset_infos: Default::default(),
            default_process_plans: Default::default(),
        }
    }

    pub async fn wait_until_processed(&self, path: &Path) -> ProcessStatus {
        let mut receiver = {
            let mut infos = self.asset_infos.write().await;
            let info = infos.get_or_insert(AssetPath::new(path.to_owned(), None), false);
            match info.status {
                Some(result) => return result,
                // This receiver must be created prior to losing the read lock to ensure this is transactional
                None => info.status_receiver.clone(),
            }
        };

        receiver.recv().await.unwrap()
    }

    pub async fn wait_until_finished(&self) {
        let receiver = {
            let state = self.state.read().await;
            match *state {
                ProcessorState::Scanning | ProcessorState::Processing => {
                    // This receiver must be created prior to losing the read lock to ensure this is transactional
                    Some(self.finished_receiver.clone())
                }
                ProcessorState::Finished => None,
            }
        };

        if let Some(mut receiver) = receiver {
            receiver.recv().await.unwrap()
        }
    }
}

#[derive(Resource, Clone)]
pub struct AssetProcessor {
    server: AssetServer,
    pub(crate) data: Arc<AssetProcessorData>,
}

impl AssetProcessor {
    pub fn new(server: AssetServer, data: Arc<AssetProcessorData>) -> Self {
        Self { server, data }
    }

    pub fn server(&self) -> &AssetServer {
        &self.server
    }

    async fn set_state(&self, state: ProcessorState) {
        let mut state_guard = self.data.state.write().await;
        let last_state = *state_guard;
        *state_guard = state;
        if last_state != ProcessorState::Finished && state == ProcessorState::Finished {
            self.data.finished_sender.broadcast(()).await.unwrap();
        }
    }

    pub async fn get_state(&self) -> ProcessorState {
        *self.data.state.read().await
    }

    pub fn source_reader(&self) -> &dyn AssetReader {
        &*self.data.source_reader
    }

    pub fn source_writer(&self) -> &dyn AssetWriter {
        &*self.data.source_writer
    }

    pub fn destination_writer(&self) -> &dyn AssetWriter {
        &*self.data.destination_writer
    }

    fn process_assets_internal<'scope, 'env>(
        &'scope self,
        scope: &'scope Scope<'scope, 'env, ()>,
        path: PathBuf,
    ) -> bevy_utils::BoxedFuture<'scope, Result<(), AssetReaderError>> {
        async move {
            if self.source_reader().is_directory(&path).await? {
                let mut path_stream = self.source_reader().read_directory(&path).await.unwrap();
                while let Some(path) = path_stream.next().await {
                    self.process_assets_internal(scope, path).await?;
                }
            } else {
                let processor = self.clone();
                scope.spawn(async move {
                    processor.process_asset(&path).await;
                });
            }
            Ok(())
        }
        .boxed()
    }

    // TODO: document this process in full and describe why the "eventual consistency" works
    pub fn process_assets(&self) {
        IoTaskPool::get().scope(|scope| {
            scope.spawn(async move {
                self.populate_processed_info().await.unwrap();
                let path = PathBuf::from("");
                self.process_assets_internal(scope, path).await.unwrap();
            });
        });
        IoTaskPool::get().scope(|scope| {
            scope.spawn(async move {
                let mut asset_infos = self.data.asset_infos.write().await;
                asset_infos.resolve_maybe_non_existent().await;
                // clean up metadata
                self.server.data.infos.write().consume_handle_drop_events();
                self.set_state(ProcessorState::Finished).await;
                trace!("finished processing");
            })
        });
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
                    // TODO: ensure that treating these dependencies as "definitely existent" is valid
                    // It should be ... as long as we only write meta when processing is successful
                    let dependency_info =
                        asset_infos.get_or_insert(load_dependency_path.to_owned(), true);
                    dependency_info.dependants.insert(path.to_owned());
                }
            }

            asset_infos.get_or_insert(path, true).processed_info = minimal.processed_info;
        }

        Ok(())
    }
    pub async fn process_asset(&self, path: &Path) {
        match self.process_asset_internal(path).await {
            Ok(result) => match result {
                ProcessResult::Processed | ProcessResult::SkippedNotChanged => {
                    let mut infos = self.data.asset_infos.write().await;
                    let info = infos.get_or_insert(AssetPath::new(path.to_owned(), None), true);
                    info.update_status(ProcessStatus::Processed).await;
                }
            },
            Err(ProcessAssetError::MissingAssetLoaderForExtension(_)) => {
                trace!("No loader found for {:?}", path);
            }
            Err(err) => {
                let mut infos = self.data.asset_infos.write().await;
                let info = infos.get_or_insert(AssetPath::new(path.to_owned(), None), true);
                info.update_status(ProcessStatus::Failed).await;
                error!("Failed to process asset {:?} {}", path, err);
            }
        }
    }
    pub async fn process_asset_internal(
        &self,
        path: &Path,
    ) -> Result<ProcessResult, ProcessAssetError> {
        trace!("Processing asset {:?}", path);
        let server = &self.server;
        let (mut source_meta, meta_bytes, process_plan) =
            match self.source_reader().read_meta(&path).await {
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
                            server
                                .get_erased_asset_loader_with_type_name(&minimal.loader)
                                .unwrap()
                                .deserialize_meta(&meta_bytes)
                                .unwrap()
                        });
                    (meta, meta_bytes, process_plan)
                }
                Err(AssetReaderError::NotFound(_path)) => {
                    let loader = server.get_path_asset_loader(&path)?;
                    let default_process_plan =
                        self.get_default_process_plan(ErasedAssetLoader::type_name(&*loader));
                    let meta = default_process_plan
                        .as_ref()
                        .map(|p| p.default_meta())
                        .unwrap_or_else(|| loader.default_meta());
                    let meta_bytes = meta.serialize();
                    // write meta to source location if it doesn't already exist
                    let mut meta_writer = self.source_writer().write_meta(&path).await?;
                    // TODO: handle error
                    meta_writer.write_all(&meta_bytes).await.unwrap();
                    meta_writer.flush().await.unwrap();
                    (meta, meta_bytes, default_process_plan)
                }
                Err(err) => return Err(ProcessAssetError::ReadAssetMetaError(err)),
            };

        // TODO:  check timestamp first for early-out
        let mut reader = self.source_reader().read(&path).await.unwrap();
        let mut asset_bytes = Vec::new();
        reader.read_to_end(&mut asset_bytes).await.unwrap();
        // PERF: in theory these hashes could be streamed if we want to avoid allocating the whole asset.
        // The downside is that reading assets would need to happen twice (once for the hash and once for the asset loader)
        // Hard to say which is worse
        let new_hash = Self::get_hash(&meta_bytes, &asset_bytes);
        let mut new_processed_info = ProcessedInfo {
            hash: new_hash,
            load_dependencies: Vec::new(),
        };

        let asset_path = AssetPath::new(path.to_owned(), None);
        {
            let infos = self.data.asset_infos.read().await;
            if let Some(asset_info) = infos.get(&asset_path) {
                if let Some(current_processed_info) = &asset_info.processed_info {
                    if current_processed_info.hash == new_hash {
                        trace!(
                            "Skipping processing of asset {:?} because it has not changed",
                            asset_path
                        );
                        return Ok(ProcessResult::SkippedNotChanged);
                    }
                }
            }
        }

        // TODO: error handling
        let mut writer = self.destination_writer().write(&path).await.unwrap();
        let mut meta_writer = self.destination_writer().write_meta(&path).await.unwrap();

        if let Some(process_plan) = process_plan {
            trace!("Loading asset directly in order to process it {:?}", path);
            let loaded_asset = server
                .load_with_meta_and_reader(
                    asset_path.clone(),
                    source_meta,
                    &mut asset_bytes.as_slice(),
                    false,
                )
                .await?;
            for (path, hash) in loaded_asset.loader_dependencies.iter() {
                new_processed_info
                    .load_dependencies
                    .push(LoadDependencyInfo {
                        hash: *hash,
                        path: path.to_string(),
                    })
            }
            process_plan
                .process(&mut writer, &loaded_asset)
                .await
                .unwrap();
            writer.flush().await.unwrap();

            let mut meta = loaded_asset.meta.unwrap().into_processed().unwrap();
            *meta.processed_info_mut() = Some(new_processed_info.clone());
            let meta_bytes = meta.serialize();
            meta_writer.write_all(&meta_bytes).await.unwrap();
            meta_writer.flush().await.unwrap();
        } else {
            // TODO: make sure that if this asset was previously "processed", that this state transition is correct
            // Specifically, how will this affect other assets currently being processed?
            {
                let mut infos = self.data.asset_infos.write().await;
                infos.remove(&asset_path).await;
            }

            writer.write_all(&asset_bytes).await.unwrap();
            writer.flush().await.unwrap();
            *source_meta.processed_info_mut() = Some(new_processed_info.clone());
            let meta_bytes = source_meta.serialize();
            meta_writer.write_all(&meta_bytes).await.unwrap();
            meta_writer.flush().await.unwrap();
        }

        {
            let mut infos = self.data.asset_infos.write().await;
            let info = infos.get_or_insert(asset_path, true);
            info.processed_info = Some(new_processed_info);
        }

        trace!("Finished processing asset {:?}", path);
        Ok(ProcessResult::Processed)
    }

    /// NOTE: changing the hashing logic here is a _breaking change_ that requires a [`META_FORMAT_VERSION`] bump.
    fn get_hash(meta_bytes: &[u8], asset_bytes: &[u8]) -> u64 {
        let mut hasher = Self::get_hasher();
        meta_bytes.hash(&mut hasher);
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
