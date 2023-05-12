mod log;
mod process_plan;

pub use log::*;
pub use process_plan::*;

use crate::{
    io::{
        processor_gated::ProcessorGatedReader, AssetProvider, AssetProviders, AssetReader,
        AssetReaderError, AssetSourceEvent, AssetWatcher, AssetWriter, AssetWriterError,
    },
    loader::{AssetLoader, ErasedAssetLoader},
    meta::{AssetMetaMinimal, AssetMetaProcessedInfoMinimal, LoadDependencyInfo, ProcessedInfo},
    saver::AssetSaver,
    AssetLoadError, AssetLoaderError, AssetPath, AssetServer, LoadDirectError,
    MissingAssetLoaderForExtensionError,
};
use bevy_ecs::prelude::*;
use bevy_log::{debug, error, trace, warn};
use bevy_tasks::{IoTaskPool, Scope};
use bevy_utils::{BoxedFuture, HashMap, HashSet};
use futures_io::ErrorKind;
use futures_lite::{AsyncReadExt, AsyncWriteExt, FutureExt, StreamExt};
use parking_lot::{lock_api::RawRwLock, RwLock};
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use thiserror::Error;

#[derive(Resource, Clone)]
pub struct AssetProcessor {
    server: AssetServer,
    pub(crate) data: Arc<AssetProcessorData>,
}

pub struct AssetProcessorData {
    pub(crate) asset_infos: async_lock::RwLock<ProcessorAssetInfos>,
    log: async_lock::RwLock<Option<ProcessorTransactionLog>>,
    process_plans: RwLock<
        HashMap<(&'static str, &'static str, &'static str), Arc<dyn ErasedAssetProcessPlan>>,
    >,
    default_process_plans:
        RwLock<HashMap<&'static str, (&'static str, &'static str, &'static str)>>,
    state: async_lock::RwLock<ProcessorState>,
    source_reader: Box<dyn AssetReader>,
    source_writer: Box<dyn AssetWriter>,
    destination_reader: Box<dyn AssetReader>,
    destination_writer: Box<dyn AssetWriter>,
    initialized_sender: async_broadcast::Sender<()>,
    initialized_receiver: async_broadcast::Receiver<()>,
    finished_sender: async_broadcast::Sender<()>,
    finished_receiver: async_broadcast::Receiver<()>,
    source_event_receiver: crossbeam_channel::Receiver<AssetSourceEvent>,
    _source_watcher: Option<Box<dyn AssetWatcher>>,
}

impl AssetProcessor {
    pub fn new(
        providers: &mut AssetProviders,
        source: &AssetProvider,
        destination: &AssetProvider,
    ) -> Self {
        let data = Arc::new(AssetProcessorData::new(
            providers.get_source_reader(source),
            providers.get_source_writer(source),
            providers.get_destination_reader(destination),
            providers.get_destination_writer(destination),
        ));
        let destination_reader = providers.get_destination_reader(destination);
        // The asset processor uses its own asset server with its own id space
        let server = AssetServer::new(
            Box::new(ProcessorGatedReader::new(destination_reader, data.clone())),
            true,
        );
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
        } else if last_state != ProcessorState::Processing && state == ProcessorState::Processing {
            self.data.initialized_sender.broadcast(()).await.unwrap();
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

    pub fn destination_reader(&self) -> &dyn AssetReader {
        &*self.data.destination_reader
    }

    pub fn destination_writer(&self) -> &dyn AssetWriter {
        &*self.data.destination_writer
    }

    pub fn start(processor: Res<Self>) {
        let processor = processor.clone();
        std::thread::spawn(move || {
            processor.process_assets();
            futures_lite::future::block_on(processor.listen_for_source_change_events());
        });
    }

    // TODO: document this process in full and describe why the "eventual consistency" works
    pub fn process_assets(&self) {
        let start_time = Instant::now();
        debug!("Processing started");
        IoTaskPool::get().scope(|scope| {
            scope.spawn(async move {
                self.initialize().await.unwrap();
                let path = PathBuf::from("");
                self.process_assets_internal(scope, path).await.unwrap();
            });
        });
        // This must happen _after_ the scope resolves or it will happen "too early"
        // Don't move this into the async scope above! process_assets is a blocking/sync function this is fine
        futures_lite::future::block_on(self.finish_processing_assets());
        let end_time = Instant::now();
        debug!("Processing finished in {:?}", end_time - start_time);
    }

    // PERF: parallelize change event processing
    pub async fn listen_for_source_change_events(&self) {
        debug!("Listening for changes to source assets");
        loop {
            for event in self.data.source_event_receiver.try_iter() {
                match event {
                    AssetSourceEvent::Added(path)
                    | AssetSourceEvent::AddedMeta(path)
                    | AssetSourceEvent::Modified(path)
                    | AssetSourceEvent::ModifiedMeta(path) => {
                        debug!("Asset {:?} was modified. Attempting to re-process", path);
                        self.process_asset(&path).await;
                    }
                    AssetSourceEvent::Removed(path) => {
                        debug!("Removing processed {:?} because source was removed", path);
                        error!("remove is not implemented");
                        // // TODO: clean up in memory
                        // if let Err(err) = self.destination_writer().remove(&path).await {
                        //     warn!("Failed to remove non-existent asset {path:?}: {err}");
                        // }
                    }
                    AssetSourceEvent::RemovedMeta(path) => {
                        // If meta was removed, we might need to regenerate it.
                        // Likewise, the user might be manually re-adding the asset.
                        // Therefore, we shouldn't automatically delete meta ... that is a
                        // user-initiated action.
                        debug!(
                            "Meta for asset {:?} was removed. Attempting to re-process",
                            path
                        );
                        self.process_asset(&path).await;
                    }
                    AssetSourceEvent::AddedFolder(path) => {
                        debug!("Folder {:?} was added. Attempting to re-process", path);
                        // error!("add folder not implemented");
                        IoTaskPool::get().scope(|scope| {
                            scope.spawn(async move {
                                self.process_assets_internal(scope, path).await.unwrap();
                            });
                        });
                    }
                    AssetSourceEvent::RemovedFolder(path) => {
                        debug!("Removing folder {:?} because source was removed", path);
                        error!("remove folder is not implemented");
                        // TODO: clean up memory
                        // if let Err(err) = self.destination_writer().remove_directory(&path).await {
                        //     warn!("Failed to remove folder {path:?}: {err}");
                        // }
                    }
                }
            }

            // TODO: make sure the "global processor state" is set appropriately here. or alternatively, remove the global processor state
            self.finish_processing_assets().await;
        }
    }

    async fn finish_processing_assets(&self) {
        self.try_reprocessing_queued().await;
        // clean up metadata in asset server
        self.server.data.infos.write().consume_handle_drop_events();
        self.set_state(ProcessorState::Finished).await;
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
                // Files without extensions are skipped
                if path.extension().is_some() {
                    let processor = self.clone();
                    scope.spawn(async move {
                        processor.process_asset(&path).await;
                    });
                }
            }
            Ok(())
        }
        .boxed()
    }

    async fn try_reprocessing_queued(&self) {
        let mut check_reprocess = true;
        while check_reprocess {
            let mut check_reprocess_queue =
                std::mem::take(&mut self.data.asset_infos.write().await.check_reprocess_queue);
            IoTaskPool::get().scope(|scope| {
                for path in check_reprocess_queue.drain(..) {
                    let processor = self.clone();
                    scope.spawn(async move {
                        processor.process_asset(&path).await;
                    });
                }
            });
            let infos = self.data.asset_infos.read().await;
            check_reprocess = !infos.check_reprocess_queue.is_empty();
        }
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

    /// Populates the initial view of each asset by scanning the source and destination folders.
    /// This info will later be used to determine whether or not to re-process an asset
    async fn initialize(&self) -> Result<(), InitializeError> {
        self.validate_transaction_log_and_recover().await;
        let mut asset_infos = self.data.asset_infos.write().await;
        fn get_asset_paths<'a>(
            reader: &'a dyn AssetReader,
            path: PathBuf,
            paths: &'a mut Vec<PathBuf>,
        ) -> BoxedFuture<'a, Result<(), AssetReaderError>> {
            async move {
                if reader.is_directory(&path).await? {
                    let mut path_stream = reader.read_directory(&path).await?;
                    while let Some(child_path) = path_stream.next().await {
                        get_asset_paths(reader, child_path, paths).await?
                    }
                } else {
                    paths.push(path);
                }
                Ok(())
            }
            .boxed()
        }

        let mut source_paths = Vec::new();
        let source_reader = self.source_reader();
        get_asset_paths(source_reader, PathBuf::from(""), &mut source_paths)
            .await
            .map_err(InitializeError::FailedToReadSourcePaths)?;

        let mut destination_paths = Vec::new();
        let destination_reader = self.destination_reader();
        get_asset_paths(
            destination_reader,
            PathBuf::from(""),
            &mut destination_paths,
        )
        .await
        .map_err(InitializeError::FailedToReadSourcePaths)?;

        for path in source_paths.iter() {
            asset_infos.get_or_insert(AssetPath::new(path.to_owned(), None));
        }

        for path in destination_paths.iter() {
            let asset_path = AssetPath::new(path.to_owned(), None);
            let mut dependencies = Vec::new();
            if let Some(info) = asset_infos.get_mut(&asset_path) {
                match self.destination_reader().read_meta_bytes(path).await {
                    Ok(meta_bytes) => {
                        match ron::de::from_bytes::<AssetMetaProcessedInfoMinimal>(&meta_bytes) {
                            Ok(minimal) => {
                                debug!(
                                    "Populated processed info for asset {path:?} {:?}",
                                    minimal.processed_info
                                );

                                if let Some(processed_info) = &minimal.processed_info {
                                    for load_dependency_info in &processed_info.load_dependencies {
                                        dependencies.push(load_dependency_info.path.to_owned());
                                    }
                                }
                                info.processed_info = minimal.processed_info;
                            }
                            Err(err) => {
                                debug!("Removing processed data for {path:?} because meta could not be parsed: {err}");
                                self.remove_processed_asset(path).await;
                            }
                        }
                    }
                    Err(err) => {
                        debug!("Removing processed data for {path:?} because meta failed to load: {err}");
                        self.remove_processed_asset(path).await;
                    }
                }
            } else {
                debug!("Removing processed data for non-existent asset {path:?}");
                self.remove_processed_asset(path).await;
            }

            for dependency in dependencies {
                asset_infos.add_dependant(&dependency, asset_path.to_owned());
            }
        }

        self.set_state(ProcessorState::Processing).await;

        Ok(())
    }

    async fn remove_processed_asset(&self, path: &Path) {
        if let Err(err) = self.destination_writer().remove(path).await {
            warn!("Failed to remove non-existent asset {path:?}: {err}");
        }

        if let Err(err) = self.destination_writer().remove_meta(path).await {
            warn!("Failed to remove non-existent meta {path:?}: {err}");
        }
    }

    async fn process_asset(&self, path: &Path) {
        let result = self.process_asset_internal(path).await;
        let mut infos = self.data.asset_infos.write().await;
        let asset_path = AssetPath::new(path.to_owned(), None);
        infos.finish_processing(asset_path, result).await;
    }

    async fn process_asset_internal(
        &self,
        path: &Path,
    ) -> Result<ProcessResult, ProcessAssetError> {
        debug!("Processing asset {:?}", path);
        let server = &self.server;
        let (mut source_meta, meta_bytes, process_plan) =
            match self.source_reader().read_meta_bytes(&path).await {
                Ok(meta_bytes) => {
                    // TODO: handle error
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
            full_hash: new_hash,
            load_dependencies: Vec::new(),
        };

        let asset_path = AssetPath::new(path.to_owned(), None);
        {
            let infos = self.data.asset_infos.read().await;
            if let Some(current_processed_info) = infos
                .get(&asset_path)
                .and_then(|i| i.processed_info.as_ref())
            {
                if current_processed_info.hash == new_hash {
                    let mut dependency_changed = false;
                    for current_dep_info in &current_processed_info.load_dependencies {
                        let live_hash = infos
                            .get(&current_dep_info.path)
                            .and_then(|i| i.processed_info.as_ref())
                            .map(|i| i.full_hash);
                        if live_hash != Some(current_dep_info.full_hash) {
                            dependency_changed = true;
                            break;
                        }
                    }
                    if !dependency_changed {
                        return Ok(ProcessResult::SkippedNotChanged);
                    }
                }
            }
        }
        let transaction_lock = {
            let mut infos = self.data.asset_infos.write().await;
            let info = infos.get_or_insert(asset_path.clone());
            info.file_transaction_lock.clone()
        };
        transaction_lock.lock_exclusive();
        // TODO: when adding error handling, make sure the lock is unlocked
        // TODO: error handling
        let mut writer = self.destination_writer().write(&path).await.unwrap();
        let mut meta_writer = self.destination_writer().write_meta(&path).await.unwrap();

        if let Some(process_plan) = process_plan {
            debug!("Loading asset directly in order to process it {:?}", path);
            let loaded_asset = server
                .load_with_meta_and_reader(
                    asset_path.clone(),
                    source_meta,
                    &mut asset_bytes.as_slice(),
                    false,
                )
                .await?;
            for (path, full_hash) in loaded_asset.loader_dependencies.iter() {
                new_processed_info
                    .load_dependencies
                    .push(LoadDependencyInfo {
                        full_hash: *full_hash,

                        path: path.to_owned(),
                    })
            }
            let full_hash = Self::get_full_hash(
                new_hash,
                new_processed_info
                    .load_dependencies
                    .iter()
                    .map(|i| i.full_hash),
            );
            new_processed_info.full_hash = full_hash;
            // TODO: if the process plan fails this will produce an "unfinished" log entry, forcing a rebuild on next run.
            // Directly writing to the asset file in the process plan necessitates this behavior.
            {
                let mut logger = self.data.log.write().await;
                logger.as_mut().unwrap().begin_path(&path).await.unwrap();
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
            {
                let mut logger = self.data.log.write().await;
                logger.as_mut().unwrap().begin_path(&path).await.unwrap();
            }
            writer.write_all(&asset_bytes).await.unwrap();
            writer.flush().await.unwrap();
            *source_meta.processed_info_mut() = Some(new_processed_info.clone());
            let meta_bytes = source_meta.serialize();
            meta_writer.write_all(&meta_bytes).await.unwrap();
            meta_writer.flush().await.unwrap();
        }

        {
            let mut logger = self.data.log.write().await;
            logger.as_mut().unwrap().end_path(&path).await.unwrap();
        }

        // SAFETY: exclusive lock was acquired above
        // See ProcessedAssetInfo::file_transaction_lock docs for rationale
        unsafe { transaction_lock.unlock_exclusive() }

        Ok(ProcessResult::Processed(new_processed_info))
    }

    async fn validate_transaction_log_and_recover(&self) {
        if let Err(err) = ProcessorTransactionLog::validate().await {
            let state_is_valid = match err {
                ValidateLogError::ReadLogError(err) => {
                    error!("Failed to read processor log file. Processed assets cannot be validated so they must be re-generated {err}");
                    false
                }
                ValidateLogError::EntryErrors(entry_errors) => {
                    let mut state_is_valid = true;
                    for entry_error in entry_errors {
                        match entry_error {
                            LogEntryError::DuplicateTransaction(_)
                            | LogEntryError::EndedMissingTransaction(_) => {
                                error!("{}", entry_error);
                                state_is_valid = false;
                                break;
                            }
                            LogEntryError::UnfinishedTransaction(path) => {
                                debug!("Asset {path:?} did not finish processing. Clearning state for that asset");
                                if let Err(err) = self.destination_writer().remove(&path).await {
                                    match err {
                                        AssetWriterError::Io(err) => {
                                            // any error but NotFound means we could be in a bad state
                                            if err.kind() != ErrorKind::NotFound {
                                                error!("Failed to remove asset {path:?}: {err}");
                                                state_is_valid = false;
                                            }
                                        }
                                    }
                                }
                                if let Err(err) = self.destination_writer().remove_meta(&path).await
                                {
                                    match err {
                                        AssetWriterError::Io(err) => {
                                            // any error but NotFound means we could be in a bad state
                                            if err.kind() != ErrorKind::NotFound {
                                                error!(
                                                    "Failed to remove asset meta {path:?}: {err}"
                                                );
                                                state_is_valid = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    state_is_valid
                }
            };

            if !state_is_valid {
                error!("Processed asset transaction log state was invalid and unrecoverable for some reason (see previous logs). Removing processed assets and starting fresh.");
                if let Err(err) = self
                    .destination_writer()
                    .remove_assets_in_directory(&Path::new(""))
                    .await
                {
                    panic!("Processed assets were in a bad state. To correct this, the asset processor attempted to remove all processed assets and start from scratch. This failed. There is no way to continue. Try restarting, or deleting imported asset state manually. {err}");
                }
            }
        }
        let mut log = self.data.log.write().await;
        *log = match ProcessorTransactionLog::new().await {
            Ok(log) => Some(log),
            Err(err) => panic!("Failed to initialize asset processor log. This cannot be recovered. Try restarting. If that doesn't work, try deleting processed asset state. {}", err),
        };
    }

    /// NOTE: changing the hashing logic here is a _breaking change_ that requires a [`META_FORMAT_VERSION`] bump.
    fn get_hash(meta_bytes: &[u8], asset_bytes: &[u8]) -> u64 {
        let mut hasher = Self::get_hasher();
        meta_bytes.hash(&mut hasher);
        asset_bytes.hash(&mut hasher);
        hasher.finish()
    }

    fn get_full_hash(hash: u64, dependency_hashes: impl Iterator<Item = u64>) -> u64 {
        let mut hasher = Self::get_hasher();
        hash.hash(&mut hasher);
        for hash in dependency_hashes {
            hash.hash(&mut hasher);
        }
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

impl AssetProcessorData {
    pub fn new(
        source_reader: Box<dyn AssetReader>,
        source_writer: Box<dyn AssetWriter>,
        destination_reader: Box<dyn AssetReader>,
        destination_writer: Box<dyn AssetWriter>,
    ) -> Self {
        let (finished_sender, finished_receiver) = async_broadcast::broadcast(1);
        let (initialized_sender, initialized_receiver) = async_broadcast::broadcast(1);
        let (source_event_sender, source_event_receiver) = crossbeam_channel::unbounded();
        // TODO: watching for changes could probably be entirely optional / we could just warn here
        let source_watcher = source_reader.watch_for_changes(source_event_sender);
        if source_watcher.is_none() {
            error!(
                "Cannot watch for changes because the current `AssetReader` does not support it"
            );
        }
        AssetProcessorData {
            source_reader,
            source_writer,
            destination_reader,
            destination_writer,
            finished_sender,
            finished_receiver,
            initialized_sender,
            initialized_receiver,
            source_event_receiver,
            _source_watcher: source_watcher,
            state: async_lock::RwLock::new(ProcessorState::Initializing),
            log: Default::default(),
            process_plans: Default::default(),
            asset_infos: Default::default(),
            default_process_plans: Default::default(),
        }
    }

    pub async fn wait_until_processed(&self, path: &Path) -> ProcessStatus {
        self.wait_until_initialized().await;
        let mut receiver = {
            let infos = self.asset_infos.write().await;
            let info = infos.get(&AssetPath::new(path.to_owned(), None));
            match info {
                Some(info) => match info.status {
                    Some(result) => return result,
                    // This receiver must be created prior to losing the read lock to ensure this is transactional
                    None => info.status_receiver.clone(),
                },
                None => return ProcessStatus::NonExistent,
            }
        };
        receiver.recv().await.unwrap()
    }

    pub async fn wait_until_initialized(&self) {
        let receiver = {
            let state = self.state.read().await;
            match *state {
                ProcessorState::Initializing => {
                    // This receiver must be created prior to losing the read lock to ensure this is transactional
                    Some(self.initialized_receiver.clone())
                }
                _ => None,
            }
        };

        if let Some(mut receiver) = receiver {
            receiver.recv().await.unwrap()
        }
    }

    pub async fn wait_until_finished(&self) {
        let receiver = {
            let state = self.state.read().await;
            match *state {
                ProcessorState::Initializing | ProcessorState::Processing => {
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

#[derive(Debug, Clone)]
pub enum ProcessResult {
    Processed(ProcessedInfo),
    SkippedNotChanged,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ProcessStatus {
    Processed,
    Failed,
    NonExistent,
}

pub(crate) struct ProcessorAssetInfo {
    processed_info: Option<ProcessedInfo>,
    dependants: HashSet<AssetPath<'static>>,
    status: Option<ProcessStatus>,
    /// A lock that controls read/write access to processed asset files. The lock is shared for both the asset bytes and the meta bytes.
    /// _This lock must be locked whenever a read or write to processed assets occurs_
    /// There are scenarios where processed assets (and their metadata) are being read and written in multiple places at once:
    /// * when the processor is running in parallel with an app
    /// * when processing assets in parallel, the processor might read an asset's load_dependencies when processing new versions of those dependencies
    ///     * this second scenario almost certainly isn't possible with the current implementation, but its worth protecting against
    /// This lock defends against those scenarios by ensuring readers don't read while processed files are being written. And it ensures
    /// Because this lock is shared across meta and asset bytes, readers can esure they don't read "old" versions of metadata with "new" asset data.  
    /// This is a "raw" rwlock because:
    /// * it doesn't need to own data
    /// * [`ProcessorGatedReader`] needs to hold locks without lifetimes tied to [`ProcessorAssetInfo`]. High level lock guards have lifetimes
    pub(crate) file_transaction_lock: Arc<parking_lot::RawRwLock>,
    status_sender: async_broadcast::Sender<ProcessStatus>,
    status_receiver: async_broadcast::Receiver<ProcessStatus>,
}

impl Default for ProcessorAssetInfo {
    fn default() -> Self {
        let (status_sender, status_receiver) = async_broadcast::broadcast(1);
        Self {
            processed_info: Default::default(),
            dependants: Default::default(),
            file_transaction_lock: Arc::new(RawRwLock::INIT),
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

/// The "current" in memory view of the asset space. This is "eventually consistent". It does not directly
/// represent the state of assets in storage, but rather a valid historical view that will gradually become more
/// consistent as events are processed.
#[derive(Default)]
pub struct ProcessorAssetInfos {
    /// The "current" in memory view of the asset space. During processing, if path does not exist in this, it should
    /// be considered non-existent.
    /// NOTE: YOU MUST USE `get_or_insert` TO ADD ITEMS TO THIS COLLECTION
    infos: HashMap<AssetPath<'static>, ProcessorAssetInfo>,
    /// Dependants for assets that don't exist. This exists to track "dangling" asset references due to deleted / missing files.
    /// If the dependant asset is added, it can "resolve" these dependancies and re-compute those assets.
    /// Therefore this _must_ always be consistent with the `infos` data. If a new asset is added to `infos`, it should
    /// check this maps for dependencies and add them. If an asset is removed, it should update the dependants here.
    non_existent_dependants: HashMap<AssetPath<'static>, HashSet<AssetPath<'static>>>,
    check_reprocess_queue: VecDeque<PathBuf>,
}

impl ProcessorAssetInfos {
    fn get_or_insert(&mut self, asset_path: AssetPath<'static>) -> &mut ProcessorAssetInfo {
        self.infos.entry(asset_path.clone()).or_insert_with(|| {
            let mut info = ProcessorAssetInfo::default();
            // track existing dependenants by resolving existing "hanging" dependants.
            if let Some(dependants) = self.non_existent_dependants.remove(&asset_path) {
                info.dependants = dependants;
            }
            info
        })
    }

    pub(crate) fn get(&self, asset_path: &AssetPath<'static>) -> Option<&ProcessorAssetInfo> {
        self.infos.get(asset_path)
    }

    fn get_mut(&mut self, asset_path: &AssetPath<'static>) -> Option<&mut ProcessorAssetInfo> {
        self.infos.get_mut(asset_path)
    }

    fn add_dependant(&mut self, asset_path: &AssetPath<'static>, dependant: AssetPath<'static>) {
        if let Some(info) = self.get_mut(asset_path) {
            info.dependants.insert(dependant);
        } else {
            let dependants = self
                .non_existent_dependants
                .entry(asset_path.to_owned())
                .or_default();
            dependants.insert(dependant);
        }
    }

    async fn finish_processing(
        &mut self,
        asset_path: AssetPath<'static>,
        result: Result<ProcessResult, ProcessAssetError>,
    ) {
        match result {
            Ok(ProcessResult::Processed(processed_info)) => {
                debug!("Finished processing asset {:?}", asset_path,);
                // clean up old dependants
                let old_processed_info = self
                    .infos
                    .get_mut(&asset_path)
                    .and_then(|i| i.processed_info.take());
                if let Some(old_processed_info) = old_processed_info {
                    self.clear_dependencies(&asset_path, old_processed_info);
                }

                // populate new dependants
                for load_dependency_info in &processed_info.load_dependencies {
                    self.add_dependant(&load_dependency_info.path, asset_path.to_owned());
                }
                let info = self.get_or_insert(asset_path);
                info.processed_info = Some(processed_info);
                info.update_status(ProcessStatus::Processed).await;
                let dependants = info.dependants.iter().cloned().collect::<Vec<_>>();
                for path in dependants {
                    self.check_reprocess_queue.push_back(path.path().to_owned());
                }
            }
            Ok(ProcessResult::SkippedNotChanged) => {
                debug!(
                    "Skipping processing of asset {:?} because it has not changed",
                    asset_path
                );
                let info = self.get_mut(&asset_path).expect("info should exist");
                // NOTE: skipping an asset on a given pass doesn't mean it won't change in the future as a result
                // of a dependency being re-processed. This means apps might receive an "old" (but valid) asset first.
                // This is in the interest of fast startup times that don't block for all assets being checked + reprocessed
                // Therefore this relies on hot-reloading in the app to pickup the "latest" version of the asset
                // If "block until latest state is reflected" is required, we can easily add a less granular
                // "block until first pass finished" mode
                info.update_status(ProcessStatus::Processed).await;
            }
            Err(ProcessAssetError::MissingAssetLoaderForExtension(_)) => {
                trace!("No loader found for {:?}", asset_path);
            }
            Err(err) => {
                error!("Failed to process asset {:?}: {:?}", asset_path, err);
                // if this failed because a dependency could not be loaded, make sure it is reprocessed if that dependency is reprocessed
                if let ProcessAssetError::AssetLoadError(AssetLoadError::AssetLoaderError {
                    error: AssetLoaderError::Load(loader_error),
                    ..
                }) = err
                {
                    if let Some(error) = loader_error.downcast_ref::<LoadDirectError>() {
                        let info = self.get_mut(&asset_path).expect("info should exist");
                        info.processed_info = Some(ProcessedInfo {
                            hash: u64::MAX,
                            full_hash: u64::MAX,
                            load_dependencies: vec![],
                        });
                        self.add_dependant(&error.dependency, asset_path.to_owned());
                    }
                }

                let info = self.get_mut(&asset_path).expect("info should exist");
                info.update_status(ProcessStatus::Failed).await;
            }
        }
    }

    // Remove the info for the given path. This should only happen if an asset's source is removed / non-existent
    async fn remove(&mut self, asset_path: &AssetPath<'static>) {
        let info = self.infos.remove(asset_path);
        if let Some(info) = info {
            if let Some(processed_info) = info.processed_info {
                self.clear_dependencies(&asset_path, processed_info);
            }
            // Tell all listeners this asset does not exist
            info.status_sender
                .broadcast(ProcessStatus::NonExistent)
                .await
                .unwrap();
        }
    }

    fn clear_dependencies(&mut self, asset_path: &AssetPath<'static>, removed_info: ProcessedInfo) {
        for old_load_dep in removed_info.load_dependencies {
            if let Some(info) = self.infos.get_mut(&old_load_dep.path) {
                info.dependants.remove(asset_path);
            } else {
                if let Some(dependants) = self.non_existent_dependants.get_mut(&old_load_dep.path) {
                    dependants.remove(&asset_path);
                }
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ProcessorState {
    Initializing,
    Processing,
    Finished,
}

#[derive(Error, Debug)]
pub enum InitializeError {
    #[error(transparent)]
    FailedToReadSourcePaths(AssetReaderError),
    #[error(transparent)]
    FailedToReadDestinationPaths(AssetReaderError),
    #[error("Failed to validate asset log: {0}")]
    ValidateLogError(ValidateLogError),
}
