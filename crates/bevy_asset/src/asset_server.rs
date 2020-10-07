use crate::{
    path::{AssetPath, AssetPathId, SourcePathId},
    Asset, AssetDynamic, AssetIo, AssetIoError, AssetLifecycle, AssetLifecycleChannel,
    AssetLifecycleEvent, AssetLoader, AssetSources, Assets, FileAssetIo, Handle, HandleId,
    HandleUntyped, LabelId, LoadContext, LoadState, RefChange, RefChangeChannel, SourceInfo,
    SourceMeta,
};
use anyhow::Result;
use bevy_ecs::Res;
use bevy_tasks::TaskPool;
use bevy_utils::HashMap;
use crossbeam_channel::TryRecvError;
use parking_lot::RwLock;
use std::{
    path::{Path, PathBuf},
    str::Utf8Error,
    sync::Arc,
};
use thiserror::Error;
use uuid::Uuid;

/// Errors that occur while loading assets with an AssetServer
#[derive(Error, Debug)]
pub enum AssetServerError {
    #[error("Asset folder path is not a directory.")]
    AssetFolderNotADirectory(String),
    #[error("No AssetLoader found for the given extension.")]
    MissingAssetLoader,
    #[error("The given type does not match the type of the loaded asset.")]
    IncorrectHandleType,
    #[error("PathLoader encountered an error")]
    PathLoaderError(#[from] AssetIoError),
    #[error("Encountered an error loading meta asset information.")]
    MetaLoad(#[from] MetaLoadError),
}

#[derive(Error, Debug)]
pub enum MetaLoadError {
    #[error("Meta file is not a valid utf8 string.")]
    Utf8(#[from] Utf8Error),
    #[error("Meta file is not valid ron.")]
    Ron(#[from] ron::Error),
    #[error("Encountered an error loading the path.")]
    AssetIoError(#[from] AssetIoError),
}

#[derive(Default)]
pub(crate) struct AssetRefCounter {
    pub(crate) channel: Arc<RefChangeChannel>,
    pub(crate) ref_counts: Arc<RwLock<HashMap<HandleId, usize>>>,
}

pub struct AssetServerInternal<TAssetIo: AssetIo = FileAssetIo> {
    pub(crate) asset_io: TAssetIo,
    pub(crate) asset_ref_counter: AssetRefCounter,
    pub(crate) asset_sources: Arc<RwLock<AssetSources>>,
    pub(crate) asset_lifecycles: Arc<RwLock<HashMap<Uuid, Box<dyn AssetLifecycle>>>>,
    loaders: RwLock<Vec<Arc<Box<dyn AssetLoader>>>>,
    extension_to_loader_index: RwLock<HashMap<String, usize>>,
    handle_to_path: Arc<RwLock<HashMap<HandleId, AssetPath<'static>>>>,
    task_pool: TaskPool,
}

/// Loads assets from the filesystem on background threads
pub struct AssetServer<TAssetIo: AssetIo = FileAssetIo> {
    pub(crate) server: Arc<AssetServerInternal<TAssetIo>>,
}

impl<TAssetIo: AssetIo> Clone for AssetServer<TAssetIo> {
    fn clone(&self) -> Self {
        Self {
            server: self.server.clone(),
        }
    }
}

impl<TAssetIo: AssetIo> AssetServer<TAssetIo> {
    pub fn new(asset_io: TAssetIo, task_pool: TaskPool) -> Self {
        AssetServer {
            server: Arc::new(AssetServerInternal {
                loaders: Default::default(),
                extension_to_loader_index: Default::default(),
                asset_sources: Default::default(),
                asset_ref_counter: Default::default(),
                handle_to_path: Default::default(),
                asset_lifecycles: Default::default(),
                task_pool,
                asset_io,
            }),
        }
    }

    pub(crate) fn register_asset_type<T: Asset + AssetDynamic>(&self) -> Assets<T> {
        self.server.asset_lifecycles.write().insert(
            T::TYPE_UUID,
            Box::new(AssetLifecycleChannel::<T>::default()),
        );
        Assets::new(self.server.asset_ref_counter.channel.sender.clone())
    }

    pub fn add_loader<T>(&self, loader: T)
    where
        T: AssetLoader,
    {
        let mut loaders = self.server.loaders.write();
        let loader_index = loaders.len();
        for extension in loader.extensions().iter() {
            self.server
                .extension_to_loader_index
                .write()
                .insert(extension.to_string(), loader_index);
        }
        loaders.push(Arc::new(Box::new(loader)));
    }

    pub fn watch_for_changes(&self) -> Result<(), AssetServerError> {
        self.server.asset_io.watch_for_changes()?;
        Ok(())
    }

    pub fn load_folder_meta<P: AsRef<Path>>(&self, path: P) -> Result<(), AssetServerError> {
        for child_path in self.server.asset_io.read_directory(path.as_ref())? {
            if child_path.is_dir() {
                self.load_folder_meta(&child_path)?;
            } else {
                if self
                    .get_asset_loader(child_path.extension().unwrap().to_str().unwrap())
                    .is_err()
                {
                    continue;
                }
                match self.load_asset_meta(&child_path) {
                    Ok(_) => {}
                    Err(MetaLoadError::AssetIoError(AssetIoError::NotFound)) => {}
                    Err(err) => return Err(err.into()),
                }
            }
        }

        Ok(())
    }

    pub fn get_handle<T: Asset, I: Into<HandleId>>(&self, id: I) -> Option<Handle<T>> {
        let sender = self.server.asset_ref_counter.channel.sender.clone();
        Some(Handle::strong(id.into(), sender))
    }

    pub fn get_handle_untyped<I: Into<HandleId>>(&self, id: I) -> Option<HandleUntyped> {
        let sender = self.server.asset_ref_counter.channel.sender.clone();
        Some(HandleUntyped::strong(id.into(), sender))
    }

    pub fn get_meta_path<P: AsRef<Path>>(path: P) -> PathBuf {
        let mut meta_path = path.as_ref().to_owned();
        if let Some(extension) = meta_path.extension().map(|e| e.to_owned()) {
            meta_path.set_extension(&format!(
                "{}.meta",
                extension
                    .to_str()
                    .expect("extension should be valid unicode")
            ));
        } else {
            meta_path.set_extension("meta");
        }

        meta_path
    }

    fn load_asset_meta<P: AsRef<Path>>(&self, path: P) -> Result<SourceMeta, MetaLoadError> {
        let path = path.as_ref();
        let meta_path = Self::get_meta_path(path);
        match self.server.asset_io.load_path(&meta_path) {
            Ok(meta_bytes) => {
                let meta_str = std::str::from_utf8(&meta_bytes)?;
                Ok(ron::from_str::<SourceMeta>(&meta_str)?)
            }
            Err(err) => Err(MetaLoadError::from(err)),
        }
    }

    pub fn save_meta(&self) -> Result<(), AssetServerError> {
        for asset_info in self.server.asset_sources.read().iter() {
            let meta_ron =
                ron::ser::to_string_pretty(&asset_info.meta, ron::ser::PrettyConfig::new())
                    .unwrap();
            let meta_path = Self::get_meta_path(&asset_info.path);
            self.server
                .asset_io
                .save_path(&meta_path, meta_ron.as_ref())?;
        }

        Ok(())
    }

    fn get_asset_loader(
        &self,
        extension: &str,
    ) -> Result<Arc<Box<dyn AssetLoader>>, AssetServerError> {
        self.server
            .extension_to_loader_index
            .read()
            .get(extension)
            .map(|index| self.server.loaders.read()[*index].clone())
            .ok_or(AssetServerError::MissingAssetLoader)
    }

    pub fn get_load_state_untyped<I: Into<SourcePathId>>(&self, id: I) -> Option<LoadState> {
        self.server.asset_sources.read().get_load_state(id.into())
    }

    pub fn get_handle_path<H: Into<HandleId>>(&self, handle: H) -> Option<AssetPath<'_>> {
        self.server
            .handle_to_path
            .read()
            .get(&handle.into())
            .cloned()
    }

    pub fn get_load_state<H: Into<HandleId>>(&self, handle: H) -> Option<LoadState> {
        match handle.into() {
            HandleId::AssetPathId(id) => self
                .server
                .asset_sources
                .read()
                .get_load_state(id.source_path_id()),
            HandleId::Id(_, _) => None,
        }
    }

    pub fn get_group_load_state(
        &self,
        handles: impl IntoIterator<Item = HandleId>,
    ) -> Option<LoadState> {
        let mut load_state = LoadState::Loaded;
        for handle_id in handles {
            match handle_id {
                HandleId::AssetPathId(id) => match self.get_load_state_untyped(id) {
                    Some(LoadState::Loaded) => continue,
                    Some(LoadState::Loading) => {
                        load_state = LoadState::Loading;
                    }
                    Some(LoadState::Failed) => return Some(LoadState::Failed),
                    None => return None,
                },
                HandleId::Id(_, _) => return None,
            }
        }

        Some(load_state)
    }

    pub fn load<'a, T: Asset, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Result<Handle<T>, AssetServerError> {
        let handle_untyped = self.load_untyped(path)?;
        Ok(handle_untyped.typed())
    }

    fn load_with_loader<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
        asset_loader: Arc<Box<dyn AssetLoader>>,
    ) -> Result<(), AssetServerError> {
        let asset_path: AssetPath = path.into();
        let extension = asset_path.path().extension().unwrap().to_str().unwrap();
        let asset_path_id: AssetPathId = asset_path.get_id();
        let version = {
            let mut asset_sources = self.server.asset_sources.write();
            if asset_sources.get(asset_path_id.source_path_id()).is_none() {
                let source_meta = match self.load_asset_meta(asset_path.path()) {
                    Ok(source_meta) => source_meta,
                    Err(MetaLoadError::AssetIoError(AssetIoError::NotFound)) => {
                        SourceMeta::new(extension.to_string())
                    }
                    Err(err) => return Err(err.into()),
                };

                asset_sources.add(SourceInfo {
                    load_state: LoadState::Loading,
                    meta: source_meta,
                    asset_types: HashMap::default(),
                    path: asset_path.path().to_owned(),
                    committed_assets: 0,
                    version: 0,
                });
            }
            let source_info = asset_sources
                .get_mut(asset_path_id.source_path_id())
                .expect("AssetSource Path -> Id mapping should exist");
            source_info.committed_assets = 0;
            source_info.version += 1;
            source_info.version
        };

        let bytes = self.server.asset_io.load_path(asset_path.path())?;
        // TODO: set previous asset ids
        let mut load_context = LoadContext::new(
            asset_path.path(),
            &self.server.asset_ref_counter.channel,
            version,
        );
        asset_loader.load(bytes, &mut load_context).unwrap();
        let mut asset_sources = self.server.asset_sources.write();
        let source_info = asset_sources
            .get_mut(asset_path_id.source_path_id())
            .expect("AssetSource should exist at this point");

        // TODO: fully replace source info to avoid statefulness bugs
        // if version hasn't changed since we started load, update metadata and send assets
        if version == source_info.version {
            load_context.set_meta(&mut source_info.meta);
            // if all assets have been committed (aka there were 0), set state to "Loaded"
            if source_info.is_loaded() {
                source_info.load_state = LoadState::Loaded;
            }

            source_info.asset_types.clear();
            for (label, loaded_asset) in load_context.labeled_assets.iter() {
                let label_id = LabelId::from(label.as_ref().map(|label| label.as_str()));
                let type_uuid = loaded_asset.value.as_ref().unwrap().type_uuid();
                source_info.asset_types.insert(label_id, type_uuid);
            }
            self.create_assets_in_load_context(&mut load_context);
        }
        Ok(())
    }

    pub fn load_untyped<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Result<HandleUntyped, AssetServerError> {
        let handle_id = self.load_untracked(path)?;
        Ok(self
            .get_handle_untyped(handle_id)
            .expect("RefChangeChannel should exist at this point"))
    }

    pub fn load_untracked<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Result<HandleId, AssetServerError> {
        let asset_path: AssetPath<'a> = path.into();
        let server = self.clone();
        let owned_path = asset_path.to_owned();
        let extension = asset_path.path().extension().unwrap().to_str().unwrap();
        let asset_loader = self.get_asset_loader(extension)?;
        self.server
            .task_pool
            .spawn(async move {
                server.load_with_loader(owned_path, asset_loader).unwrap();
            })
            .detach();
        Ok(asset_path.into())
    }

    pub fn load_folder<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Vec<HandleUntyped>, AssetServerError> {
        let path = path.as_ref();
        if !path.is_dir() {
            return Err(AssetServerError::AssetFolderNotADirectory(
                path.to_str().unwrap().to_string(),
            ));
        }

        let mut handles = Vec::new();
        for child_path in self.server.asset_io.read_directory(path.as_ref())? {
            if child_path.is_dir() {
                handles.extend(self.load_folder(&child_path)?);
            } else {
                let handle = match self
                    .load_untyped(child_path.to_str().expect("Path should be a valid string"))
                {
                    Ok(handle) => handle,
                    Err(AssetServerError::MissingAssetLoader) => continue,
                    Err(err) => return Err(err),
                };
                handles.push(handle);
            }
        }

        Ok(handles)
    }

    pub fn free_unused_assets(&self) {
        let receiver = &self.server.asset_ref_counter.channel.receiver;
        let mut ref_counts = self.server.asset_ref_counter.ref_counts.write();
        let asset_sources = self.server.asset_sources.read();
        let mut potential_frees = Vec::new();
        loop {
            let ref_change = match receiver.try_recv() {
                Ok(ref_change) => ref_change,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => panic!("RefChange channel disconnected"),
            };
            match ref_change {
                RefChange::Increment(handle_id) => *ref_counts.entry(handle_id).or_insert(0) += 1,
                RefChange::Decrement(handle_id) => {
                    let entry = ref_counts.entry(handle_id).or_insert(0);
                    *entry -= 1;
                    if *entry == 0 {
                        potential_frees.push(handle_id);
                    }
                }
            }
        }

        if !potential_frees.is_empty() {
            let asset_lifecycles = self.server.asset_lifecycles.read();
            for potential_free in potential_frees {
                if let Some(i) = ref_counts.get(&potential_free).cloned() {
                    if i == 0 {
                        let type_uuid = match potential_free {
                            HandleId::Id(type_uuid, _) => Some(type_uuid),
                            HandleId::AssetPathId(id) => asset_sources
                                .get(id.source_path_id())
                                .and_then(|source_info| source_info.get_asset_type(id.label_id())),
                        };

                        if let Some(type_uuid) = type_uuid {
                            if let Some(asset_lifecycle) = asset_lifecycles.get(&type_uuid) {
                                asset_lifecycle.free_asset(potential_free);
                            }
                        }
                    }
                }
            }
        }
    }

    fn create_assets_in_load_context(&self, load_context: &mut LoadContext) {
        let asset_lifecycles = self.server.asset_lifecycles.read();
        for (label, asset) in load_context.labeled_assets.iter_mut() {
            let asset_value = asset
                .value
                .take()
                .expect("Asset should exist at this point");
            if let Some(asset_lifecycle) = asset_lifecycles.get(&asset_value.type_uuid()) {
                let asset_path =
                    AssetPath::new_ref(&load_context.path, label.as_ref().map(|l| l.as_str()));
                asset_lifecycle.create_asset(asset_path.into(), asset_value, load_context.version);
            } else {
                panic!("Failed to find AssetSender for label {:?}. Are you sure that is a registered asset type?", label);
            }
        }
    }

    pub(crate) fn update_asset_storage<T: Asset + AssetDynamic>(&self, assets: &mut Assets<T>) {
        let asset_lifecycles = self.server.asset_lifecycles.read();
        let asset_lifecycle = asset_lifecycles.get(&T::TYPE_UUID).unwrap();
        let mut asset_sources = self.server.asset_sources.write();
        let channel = asset_lifecycle
            .downcast_ref::<AssetLifecycleChannel<T>>()
            .unwrap();

        loop {
            match channel.receiver.try_recv() {
                Ok(AssetLifecycleEvent::Create(asset_result)) => {
                    // update SourceInfo if this asset was loaded from an AssetPath
                    if let HandleId::AssetPathId(id) = asset_result.id {
                        if let Some(source_info) = asset_sources.get_mut(id.source_path_id()) {
                            if source_info.version == asset_result.version {
                                source_info.committed_assets += 1;
                                if source_info.is_loaded() {
                                    source_info.load_state = LoadState::Loaded;
                                }
                            }
                        }
                    }
                    assets.set(asset_result.id, asset_result.asset);
                }
                Ok(AssetLifecycleEvent::Free(handle_id)) => {
                    assets.remove(handle_id);
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                Err(TryRecvError::Disconnected) => panic!("AssetChannel disconnected"),
            }
        }
    }
}

pub fn free_unused_assets_system(asset_server: Res<AssetServer>) {
    asset_server.free_unused_assets();
}
