mod info;

pub use info::*;

use crate::{
    folder::LoadedFolder,
    io::{AssetReader, AssetReaderError, AssetSourceEvent, AssetWatcher, Reader},
    loader::{AssetLoader, AssetLoaderError, ErasedAssetLoader, LoadContext, LoadedAsset},
    meta::{AssetMetaDyn, AssetMetaMinimal},
    path::AssetPath,
    Asset, AssetHandleProvider, Assets, ErasedLoadedAsset, Handle, UntypedAssetId, UntypedHandle,
};
use bevy_ecs::prelude::*;
use bevy_log::{error, info};
use bevy_tasks::IoTaskPool;
use bevy_utils::{HashMap, HashSet};
use crossbeam_channel::{Receiver, Sender};
use futures_lite::{AsyncReadExt, FutureExt, StreamExt};
use parking_lot::RwLock;
use std::{any::TypeId, path::Path, sync::Arc};
use thiserror::Error;

#[derive(Resource, Clone)]
pub struct AssetServer {
    pub(crate) data: Arc<AssetServerData>,
}

pub struct AssetServerData {
    pub(crate) infos: RwLock<AssetInfos>,
    pub(crate) loaders: Arc<RwLock<AssetLoaders>>,
    asset_event_sender: Sender<InternalAssetEvent>,
    asset_event_receiver: Receiver<InternalAssetEvent>,
    source_event_receiver: Receiver<AssetSourceEvent>,
    reader: Box<dyn AssetReader>,
    _watcher: Option<Box<dyn AssetWatcher>>,
}

impl AssetServer {
    pub fn new(reader: Box<dyn AssetReader>, watch_for_changes: bool) -> Self {
        Self::new_with_loaders(reader, Default::default(), watch_for_changes)
    }

    pub(crate) fn new_with_loaders(
        reader: Box<dyn AssetReader>,
        loaders: Arc<RwLock<AssetLoaders>>,
        watch_for_changes: bool,
    ) -> Self {
        let (asset_event_sender, asset_event_receiver) = crossbeam_channel::unbounded();
        let (source_event_sender, source_event_receiver) = crossbeam_channel::unbounded();
        let watcher = if watch_for_changes {
            let watcher = reader.watch_for_changes(source_event_sender);
            if watcher.is_none() {
                error!("Cannot watch for changes because the current `AssetReader` does not support it");
            }
            watcher
        } else {
            None
        };
        Self {
            data: Arc::new(AssetServerData {
                reader,
                _watcher: watcher,
                asset_event_sender,
                asset_event_receiver,
                source_event_receiver,
                loaders,
                infos: RwLock::new(AssetInfos::default()),
            }),
        }
    }

    pub fn reader(&self) -> &dyn AssetReader {
        &*self.data.reader
    }

    pub fn register_loader<L: AssetLoader>(&self, loader: L) {
        let mut loaders = self.data.loaders.write();
        let loader_index = loaders.values.len();
        loaders
            .type_name_to_index
            .insert(std::any::type_name::<L>(), loader_index);
        for extension in loader.extensions() {
            loaders
                .extension_to_index
                .insert(extension.to_string(), loader_index);
        }
        loaders.values.push(Arc::new(loader));
    }

    pub fn register_asset<A: Asset>(&self, assets: &Assets<A>) {
        self.register_handle_provider(assets.get_handle_provider());
    }

    pub(crate) fn register_handle_provider(&self, handle_provider: AssetHandleProvider) {
        let mut infos = self.data.infos.write();
        infos
            .handle_providers
            .insert(handle_provider.type_id, handle_provider);
    }

    pub fn get_erased_asset_loader(
        &self,
        extension: &str,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForExtensionError> {
        let loaders = self.data.loaders.read();
        let index = loaders.extension_to_index.get(extension).copied();
        index
            .map(|index| loaders.values[index].clone())
            .ok_or_else(|| MissingAssetLoaderForExtensionError {
                extensions: vec![extension.to_string()],
            })
    }

    pub fn get_erased_asset_loader_with_type_name(
        &self,
        type_name: &str,
    ) -> Option<Arc<dyn ErasedAssetLoader>> {
        let loaders = self.data.loaders.read();
        let index = loaders.type_name_to_index.get(type_name).copied();
        index.map(|index| loaders.values[index].clone())
    }

    pub fn get_path_asset_loader<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForExtensionError> {
        let s = path
            .as_ref()
            .file_name()
            .ok_or(MissingAssetLoaderForExtensionError {
                extensions: Vec::new(),
            })?
            .to_str()
            .map(|s| s.to_lowercase())
            .ok_or(MissingAssetLoaderForExtensionError {
                extensions: Vec::new(),
            })?;

        let mut exts = Vec::new();
        let mut ext = s.as_str();
        while let Some(idx) = ext.find('.') {
            ext = &ext[idx + 1..];
            exts.push(ext);
            if let Ok(loader) = self.get_erased_asset_loader(ext) {
                return Ok(loader);
            }
        }
        Err(MissingAssetLoaderForExtensionError {
            extensions: exts.into_iter().map(String::from).collect(),
        })
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load<'a, A: Asset>(&self, path: impl Into<AssetPath<'a>>) -> Handle<A> {
        let path: AssetPath = path.into();
        let (handle, should_load) = {
            let mut infos = self.data.infos.write();
            infos.get_or_create_path_handle(
                path.to_owned(),
                TypeId::of::<A>(),
                HandleLoadingMode::Request,
            )
        };

        if should_load {
            let owned_handle = handle.clone();
            let owned_path = path.to_owned();
            let server = self.clone();
            IoTaskPool::get()
                .spawn(async move {
                    if let Err(err) = server
                        .load_internal(Some(owned_handle), owned_path, false)
                        .await
                    {
                        error!("{}", err);
                    }
                })
                .detach();
        }

        handle.typed_debug_checked()
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub(crate) async fn load_untyped_async<'a>(
        &self,
        path: impl Into<AssetPath<'a>>,
    ) -> Result<UntypedHandle, AssetLoadError> {
        self.load_internal(None, path.into(), false).await
    }

    async fn load_internal<'a>(
        &self,
        input_handle: Option<UntypedHandle>,
        mut path: AssetPath<'a>,
        force: bool,
    ) -> Result<UntypedHandle, AssetLoadError> {
        let (meta, loader) = self.get_meta_and_loader(&path).await.map_err(|e| {
            // if there was an input handle, a "load" operation has already started, so we must produce a "failure" event, if
            // we cannot find the meta and loader
            if let Some(handle) = &input_handle {
                self.send_asset_event(InternalAssetEvent::Failed { id: handle.id() });
            }
            e
        })?;

        let has_label = path.label().is_some();

        let (handle, should_load) = match input_handle {
            Some(handle) => {
                // TODO: add requested type validation for sub assets
                if !has_label && handle.type_id() != loader.asset_type_id() {
                    return Err(AssetLoadError::RequestedHandleTypeMismatch {
                        path: path.to_owned(),
                        requested: handle.type_id(),
                        actual_asset_name: loader.asset_type_name(),
                        loader_name: loader.type_name(),
                    });
                }
                // if a handle was passed in, the "should load" check was already done
                (handle, true)
            }
            None => {
                let mut infos = self.data.infos.write();
                infos.get_or_create_path_handle(
                    path.to_owned(),
                    loader.asset_type_id(),
                    HandleLoadingMode::Request,
                )
            }
        };

        if !should_load && !force {
            return Ok(handle);
        }

        let base_asset_id = if has_label {
            path.remove_label();
            // If the path has a label, the current id does not match the asset root type.
            // We need to get the actual asset id
            let mut infos = self.data.infos.write();
            let (actual_handle, _) = infos.get_or_create_path_handle(
                path.to_owned(),
                loader.asset_type_id(),
                // ignore current load state ... we kicked off this sub asset load because it needed to be loaded but
                // does not currently exist
                HandleLoadingMode::Force,
            );
            actual_handle.id()
        } else {
            handle.id()
        };

        match self
            .load_with_meta_and_loader(path, meta, &*loader, true)
            .await
        {
            Ok(mut loaded_asset) => {
                for (_, labeled_asset) in loaded_asset.labeled_assets.drain() {
                    self.send_asset_event(InternalAssetEvent::Loaded {
                        id: labeled_asset.handle.id(),
                        loaded_asset: labeled_asset.asset,
                    })
                }
                self.send_asset_event(InternalAssetEvent::Loaded {
                    id: base_asset_id,
                    loaded_asset,
                });
                Ok(handle)
            }
            Err(err) => {
                // TODO: fail all loading subassets
                self.send_asset_event(InternalAssetEvent::Failed { id: base_asset_id });
                Err(err)
            }
        }
    }

    pub fn reload<'a>(&self, path: impl Into<AssetPath<'a>>) {
        let server = self.clone();
        let path = path.into();
        let owned_path = path.to_owned();
        IoTaskPool::get()
            .spawn(async move {
                if server.data.infos.read().is_path_alive(&owned_path) {
                    info!("Reloading {owned_path} because it has changed");
                    if let Err(err) = server.load_internal(None, owned_path, true).await {
                        error!("{}", err);
                    }
                }
            })
            .detach();
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn add<A: Asset>(&self, asset: A) -> Handle<A> {
        self.load_asset(LoadedAsset::new(asset, None))
    }

    pub(crate) fn load_asset<A: Asset>(&self, asset: impl Into<LoadedAsset<A>>) -> Handle<A> {
        let loaded_asset: LoadedAsset<A> = asset.into();
        let erased_loaded_asset: ErasedLoadedAsset = loaded_asset.into();
        self.load_asset_untyped(erased_loaded_asset)
            .typed_debug_checked()
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_asset_untyped(&self, asset: impl Into<ErasedLoadedAsset>) -> UntypedHandle {
        let loaded_asset = asset.into();
        let handle = if let Some(path) = loaded_asset.path() {
            self.get_or_create_path_handle(path.clone(), loaded_asset.asset_type_id())
        } else {
            self.data
                .infos
                .write()
                .create_loading_handle(loaded_asset.asset_type_id())
        };
        self.send_asset_event(InternalAssetEvent::Loaded {
            id: handle.id(),
            loaded_asset,
        });
        handle
    }

    /// Loads all assets from the specified folder recursively. The [`LoadedFolder`] asset (when it loads) will
    /// contain handles to all assets in the folder. You can wait for all assets to load by checking the LoadedFolder's
    /// [`AssetRecursiveDependencyLoadState`].
    #[must_use = "not using the returned strong handle may result in the unexpected release of the assets"]
    pub fn load_folder(&self, path: impl AsRef<Path>) -> Handle<LoadedFolder> {
        let handle = {
            let mut infos = self.data.infos.write();
            infos.create_loading_handle(TypeId::of::<LoadedFolder>())
        };

        let id = handle.id();

        fn load_folder<'a>(
            path: &'a Path,
            server: &'a AssetServer,
            handles: &'a mut Vec<UntypedHandle>,
        ) -> bevy_utils::BoxedFuture<'a, Result<(), AssetLoadError>> {
            async move {
                let is_dir = server.reader().is_directory(path).await?;
                if is_dir {
                    let mut path_stream = server.reader().read_directory(path.as_ref()).await?;
                    while let Some(child_path) = path_stream.next().await {
                        if server.reader().is_directory(&child_path).await? {
                            load_folder(&child_path, server, handles).await?;
                        } else {
                            let path = child_path.to_str().expect("Path should be a valid string.");
                            match server.load_untyped_async(path).await {
                                Ok(handle) => handles.push(handle),
                                // skip assets that cannot be loaded
                                Err(
                                    AssetLoadError::MissingAssetLoaderForTypeName(_)
                                    | AssetLoadError::MissingAssetLoaderForExtension(_),
                                ) => {}
                                Err(err) => return Err(err),
                            }
                        }
                    }
                }
                Ok(())
            }
            .boxed()
        }

        let server = self.clone();
        let owned_path = path.as_ref().to_owned();
        IoTaskPool::get()
            .spawn(async move {
                let mut handles = Vec::new();
                match load_folder(&owned_path, &server, &mut handles).await {
                    Ok(_) => server.send_asset_event(InternalAssetEvent::Loaded {
                        id,
                        loaded_asset: LoadedAsset::new(LoadedFolder { handles }, None).into(),
                    }),
                    Err(_) => server.send_asset_event(InternalAssetEvent::Failed { id }),
                }
            })
            .detach();

        handle.typed_debug_checked()
    }

    fn send_asset_event(&self, event: InternalAssetEvent) {
        self.data.asset_event_sender.send(event).unwrap();
    }

    pub fn get_load_states(
        &self,
        id: impl Into<UntypedAssetId>,
    ) -> Option<(LoadState, DependencyLoadState, RecursiveDependencyLoadState)> {
        self.data
            .infos
            .read()
            .get(id.into())
            .map(|i| (i.load_state, i.dep_load_state, i.rec_dep_load_state))
    }

    pub fn get_load_state(&self, id: impl Into<UntypedAssetId>) -> Option<LoadState> {
        self.data.infos.read().get(id.into()).map(|i| i.load_state)
    }

    pub fn get_recursive_dependency_load_state(
        &self,
        id: impl Into<UntypedAssetId>,
    ) -> Option<RecursiveDependencyLoadState> {
        self.data
            .infos
            .read()
            .get(id.into())
            .map(|i| i.rec_dep_load_state)
    }

    pub fn load_state(&self, id: impl Into<UntypedAssetId>) -> LoadState {
        self.get_load_state(id).unwrap_or(LoadState::NotLoaded)
    }

    pub fn recursive_dependency_load_state(
        &self,
        id: impl Into<UntypedAssetId>,
    ) -> RecursiveDependencyLoadState {
        self.get_recursive_dependency_load_state(id)
            .unwrap_or(RecursiveDependencyLoadState::NotLoaded)
    }

    /// Returns an active handle for the given path, if the asset at the given path has already started loading,
    /// or is still "alive".
    pub fn get_handle<'a, A: Asset>(&self, path: impl Into<AssetPath<'a>>) -> Option<Handle<A>> {
        self.get_handle_untyped(path)
            .map(|h| h.typed_debug_checked())
    }

    pub fn get_handle_untyped<'a>(&self, path: impl Into<AssetPath<'a>>) -> Option<UntypedHandle> {
        let infos = self.data.infos.read();
        let path = path.into();
        infos.get_path_handle(path)
    }

    pub fn get_path(&self, id: impl Into<UntypedAssetId>) -> Option<AssetPath<'static>> {
        let infos = self.data.infos.read();
        let info = infos.get(id.into())?;
        Some(info.path.as_ref()?.to_owned())
    }

    /// Retrieve a handle for the given path. This will create a handle (and AssetInfo) if it does not exist
    pub(crate) fn get_or_create_path_handle(
        &self,
        path: AssetPath<'static>,
        type_id: TypeId,
    ) -> UntypedHandle {
        let mut infos = self.data.infos.write();
        infos
            .get_or_create_path_handle(path, type_id, HandleLoadingMode::NotLoading)
            .0
    }

    pub async fn load_direct_with_meta<'a>(
        &self,
        path: impl Into<AssetPath<'a>>,
        meta: Box<dyn AssetMetaDyn>,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let path: AssetPath = path.into();
        // TODO: handle this error
        let loader = self
            .get_erased_asset_loader_with_type_name(meta.source_loader())
            .unwrap();
        self.load_with_meta_and_loader(path, meta, &*loader, false)
            .await
    }

    pub async fn load_direct<'a>(
        &self,
        path: impl Into<AssetPath<'a>>,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let path: AssetPath = path.into();
        let (meta, loader) = self.get_meta_and_loader(&path).await?;
        self.load_with_meta_and_loader(path, meta, &*loader, false)
            .await
    }

    async fn get_meta_and_loader(
        &self,
        asset_path: &AssetPath<'_>,
    ) -> Result<(Box<dyn AssetMetaDyn>, Arc<dyn ErasedAssetLoader>), AssetLoadError> {
        match self.data.reader.read_meta_bytes(asset_path.path()).await {
            Ok(meta_bytes) => {
                // TODO: this isn't fully minimal yet. we only need the loader
                let minimal: AssetMetaMinimal = ron::de::from_bytes(&meta_bytes).unwrap();
                // TODO: handle this error
                let loader = self
                    .get_erased_asset_loader_with_type_name(&minimal.loader)
                    .unwrap();
                let meta = loader.deserialize_meta(&meta_bytes).map_err(|e| {
                    AssetLoadError::AssetLoaderError {
                        path: asset_path.to_owned(),
                        loader: loader.type_name(),
                        error: AssetLoaderError::DeserializeMeta(e),
                    }
                })?;
                Ok((meta, loader))
            }
            Err(AssetReaderError::NotFound(_)) => {
                let loader = self.get_path_asset_loader(asset_path.path())?;
                let meta = loader.default_meta();
                Ok((meta, loader))
            }
            Err(err) => return Err(err.into()),
        }
    }

    async fn load_with_meta_and_loader(
        &self,
        asset_path: AssetPath<'_>,
        meta: Box<dyn AssetMetaDyn>,
        loader: &dyn ErasedAssetLoader,
        load_dependencies: bool,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let mut reader = self.data.reader.read(asset_path.path()).await?;
        self.load_with_meta_loader_and_reader(
            asset_path,
            meta,
            loader,
            &mut reader,
            load_dependencies,
        )
        .await
    }

    pub(crate) async fn load_with_meta_and_reader(
        &self,
        asset_path: AssetPath<'_>,
        meta: Box<dyn AssetMetaDyn>,
        reader: &mut Reader<'_>,
        load_dependencies: bool,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let loader_name = meta.source_loader();
        let loader = self
            .get_erased_asset_loader_with_type_name(loader_name)
            .ok_or_else(|| AssetLoadError::MissingAssetLoaderForTypeName(loader_name.clone()))?;
        self.load_with_meta_loader_and_reader(asset_path, meta, &*loader, reader, load_dependencies)
            .await
    }

    async fn load_with_meta_loader_and_reader(
        &self,
        asset_path: AssetPath<'_>,
        meta: Box<dyn AssetMetaDyn>,
        loader: &dyn ErasedAssetLoader,
        reader: &mut Reader<'_>,
        load_dependencies: bool,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let load_context = LoadContext::new(self, asset_path.to_owned(), load_dependencies);
        loader.load(reader, meta, load_context).await.map_err(|e| {
            AssetLoadError::AssetLoaderError {
                loader: loader.type_name(),
                path: asset_path.to_owned(),
                error: e,
            }
        })
    }
}

pub fn handle_internal_asset_events(world: &mut World) {
    world.resource_scope(|world, assets: Mut<AssetServer>| {
        let mut infos = assets.data.infos.write();
        for event in assets.data.asset_event_receiver.try_iter() {
            match event {
                InternalAssetEvent::Loaded { id, loaded_asset } => {
                    infos.process_asset_load(id, loaded_asset, world);
                }
                InternalAssetEvent::Failed { id } => infos.process_asset_fail(id),
            }
        }

        let mut paths_to_reload = HashSet::new();
        for event in assets.data.source_event_receiver.try_iter() {
            match event {
                // TODO: if the asset was processed and the processed file was changed, the first modified event
                // should be skipped?
                AssetSourceEvent::Modified(path) | AssetSourceEvent::ModifiedMeta(path) => {
                    paths_to_reload.insert(path);
                }
                _ => {}
            }
        }

        for path in paths_to_reload {
            assets.reload(path);
        }
    })
}

#[derive(Default)]
pub(crate) struct AssetLoaders {
    values: Vec<Arc<dyn ErasedAssetLoader>>,
    extension_to_index: HashMap<String, usize>,
    type_name_to_index: HashMap<&'static str, usize>,
}

pub enum InternalAssetEvent {
    Loaded {
        id: UntypedAssetId,
        loaded_asset: ErasedLoadedAsset,
    },
    Failed {
        id: UntypedAssetId,
    },
}

/// The load state of an asset.
#[derive(Component, Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LoadState {
    /// The asset has not started loading yet
    NotLoaded,
    /// The asset is in the process of loading.
    Loading,
    /// The asset has been loaded and has been added to the [`World`]
    Loaded,
    /// The asset failed to load.
    Failed,
}

/// The load state of an asset's dependencies.
#[derive(Component, Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DependencyLoadState {
    /// The asset has not started loading yet
    NotLoaded,
    /// Dependencies are still loading
    Loading,
    /// Dependencies have all loaded
    Loaded,
    /// One or more dependencies have failed to load
    Failed,
}

/// The recursive load state of an asset's dependencies.
#[derive(Component, Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RecursiveDependencyLoadState {
    /// The asset has not started loading yet
    NotLoaded,
    /// Dependencies in this asset's dependency tree are still loading
    Loading,
    /// Dependencies in this asset's dependency tree have all loaded
    Loaded,
    /// One or more dependencies have failed to load in this asset's dependency tree
    Failed,
}

#[derive(Error, Debug)]

pub enum AssetLoadError {
    // TODO: producer of this error should look up friendly type names
    #[error("Requested handle of type {requested:?} for asset '{path}' does not match actual asset type '{actual_asset_name}', which used loader '{loader_name}'")]
    RequestedHandleTypeMismatch {
        path: AssetPath<'static>,
        requested: TypeId,
        actual_asset_name: &'static str,
        loader_name: &'static str,
    },
    #[error(transparent)]
    MissingAssetLoaderForExtension(#[from] MissingAssetLoaderForExtensionError),
    #[error("No `AssetLoader` found for {0}")]
    MissingAssetLoaderForTypeName(String),
    #[error(transparent)]
    AssetReaderError(#[from] AssetReaderError),
    #[error("Encountered an error while reading asset metadata bytes")]
    AssetMetaReadError,
    #[error("Asset '{path}' encountered an error in {loader}: {error}")]
    AssetLoaderError {
        path: AssetPath<'static>,
        loader: &'static str,
        error: AssetLoaderError,
    },
}

#[derive(Error, Debug)]
#[error("no `AssetLoader` found{}", format_missing_asset_ext(.extensions))]

pub struct MissingAssetLoaderForExtensionError {
    extensions: Vec<String>,
}

fn format_missing_asset_ext(exts: &[String]) -> String {
    if !exts.is_empty() {
        format!(
            " for the following extension{}: {}",
            if exts.len() > 1 { "s" } else { "" },
            exts.join(", ")
        )
    } else {
        String::new()
    }
}
