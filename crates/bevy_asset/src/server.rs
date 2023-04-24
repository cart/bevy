use crate::{
    io::{AssetReader, AssetReaderError},
    loader::{AssetLoader, AssetLoaderError, ErasedAssetLoader, LoadContext, LoadedAsset},
    meta::{AssetMetaDyn, AssetMetaMinimal},
    path::AssetPath,
    Asset, AssetHandleProvider, Assets, Handle, InternalAssetHandle, UntypedAssetId, UntypedHandle,
};
use bevy_ecs::prelude::*;
use bevy_log::{error, trace, warn};
use bevy_tasks::IoTaskPool;
use bevy_utils::{Entry, HashMap, HashSet};
use crossbeam_channel::{Receiver, Sender};
use futures_lite::AsyncReadExt;
use parking_lot::RwLock;
use std::{
    any::TypeId,
    path::Path,
    sync::{Arc, Weak},
};
use thiserror::Error;

#[derive(Resource, Clone)]
pub struct AssetServer {
    pub(crate) data: Arc<AssetsData>,
}

pub struct AssetsData {
    pub(crate) infos: RwLock<AssetInfos>,
    pub(crate) loaders: Arc<RwLock<AssetLoaders>>,
    asset_event_sender: Sender<InternalAssetEvent>,
    asset_event_receiver: Receiver<InternalAssetEvent>,
    reader: Box<dyn AssetReader>,
}

pub struct AssetInfos {
    path_to_id: HashMap<AssetPath<'static>, UntypedAssetId>,
    infos: HashMap<UntypedAssetId, AssetInfo>,
    handle_providers: HashMap<TypeId, AssetHandleProvider>,
}

#[derive(Default)]
pub(crate) struct AssetLoaders {
    values: Vec<Arc<dyn ErasedAssetLoader>>,
    extension_to_index: HashMap<String, usize>,
    type_name_to_index: HashMap<&'static str, usize>,
}

struct AssetInfo {
    weak_handle: Weak<InternalAssetHandle>,
    path: AssetPath<'static>,
    load_state: AssetLoadState,
    dep_load_state: AssetDependencyLoadState,
    rec_dep_load_state: AssetRecursiveDependencyLoadState,
    loading_dependencies: usize,
    failed_dependencies: usize,
    loading_rec_dependencies: usize,
    failed_rec_dependencies: usize,
    dependants_waiting_on_load: HashSet<UntypedAssetId>,
    dependants_waiting_on_recursive_dep_load: HashSet<UntypedAssetId>,
    handle_drops_to_skip: usize,
}

impl AssetInfo {
    fn new(weak_handle: Weak<InternalAssetHandle>, path: AssetPath<'static>) -> Self {
        Self {
            weak_handle,
            path,
            load_state: AssetLoadState::NotLoaded,
            dep_load_state: AssetDependencyLoadState::Loading,
            rec_dep_load_state: AssetRecursiveDependencyLoadState::Loading,
            loading_dependencies: 0,
            failed_dependencies: 0,
            loading_rec_dependencies: 0,
            failed_rec_dependencies: 0,
            dependants_waiting_on_load: HashSet::default(),
            dependants_waiting_on_recursive_dep_load: HashSet::default(),
            handle_drops_to_skip: 0,
        }
    }
}

impl AssetInfos {
    /// Retrieves asset tracking data, or creates it if it doesn't exist.
    /// Returns true if an asset load should be kicked off
    fn get_or_create_handle(
        &mut self,
        path: AssetPath<'static>,
        type_id: TypeId,
        requesting_load: bool,
    ) -> (UntypedHandle, bool) {
        match self.path_to_id.entry(path.clone()) {
            Entry::Occupied(entry) => {
                let id = *entry.get();
                // if there is a path_to_id entry, info always exists
                let info = self.infos.get_mut(&id).unwrap();
                let mut should_load = false;
                if requesting_load && info.load_state == AssetLoadState::NotLoaded {
                    info.load_state = AssetLoadState::Loading;
                    should_load = true;
                }

                if let Some(strong_handle) = info.weak_handle.upgrade() {
                    // If we can upgrade the handle, there is at least one live handle right now,
                    // The asset load has already kicked off (and maybe completed), so we can just
                    // return a strong handle
                    (UntypedHandle::Strong(strong_handle), should_load)
                } else {
                    // Asset meta exists, but all live handles were dropped. This means the `track_assets` system
                    // hasn't been run yet to remove the current asset
                    // (note that this is guaranteed to be transactional with the `track_assets` system because
                    // because it locks the AssetInfos collection)

                    // We must create a new strong handle for the existing id and ensure that the drop of the old
                    // strong handle doesn't remove the asset from the Assets collection
                    info.handle_drops_to_skip += 1;
                    let provider = self.handle_providers.get(&type_id).unwrap_or_else(|| {
                        panic!(
                            "Cannot allocate a handle for asset of type {:?} because it does not exist",
                            type_id
                        )
                    });
                    let handle = provider.get_handle(id.internal(), true);
                    info.weak_handle = Arc::downgrade(&handle);
                    (UntypedHandle::Strong(handle), should_load)
                }
            }
            // The entry does not exist, so this is a "fresh" asset load. We must create a new handle
            Entry::Vacant(entry) => {
                let provider = self.handle_providers.get(&type_id).unwrap_or_else(|| {
                    panic!(
                        "Cannot allocate a handle for asset of type {:?} because it does not exist",
                        type_id
                    )
                });

                let handle = provider.reserve_handle_internal(true);
                entry.insert(handle.id);
                let mut info = AssetInfo::new(Arc::downgrade(&handle), path);
                if requesting_load {
                    info.load_state = AssetLoadState::Loading;
                }
                self.infos.insert(handle.id, info);
                (UntypedHandle::Strong(handle), requesting_load)
            }
        }
    }

    fn get(&self, id: UntypedAssetId) -> Option<&AssetInfo> {
        self.infos.get(&id)
    }

    fn get_mut(&mut self, id: UntypedAssetId) -> Option<&mut AssetInfo> {
        self.infos.get_mut(&id)
    }

    fn get_path_handle(&mut self, path: AssetPath) -> Option<UntypedHandle> {
        let id = *self.path_to_id.get(&path)?;
        let info = self.infos.get(&id)?;
        let strong_handle = info.weak_handle.upgrade()?;
        Some(UntypedHandle::Strong(strong_handle))
    }

    // Returns `true` if the asset should be removed from the collection
    pub(crate) fn process_handle_drop(&mut self, id: UntypedAssetId) -> bool {
        Self::process_handle_drop_internal(&mut self.infos, &mut self.path_to_id, id)
    }

    fn process_asset_load(&mut self, loaded_asset: LoadedAsset, world: &mut World) {
        loaded_asset.asset.insert(loaded_asset.id, world);
        for (_label, sub_asset) in loaded_asset.labeled_assets {
            self.process_asset_load(sub_asset, world);
        }
        let mut loading_deps = loaded_asset.dependencies.len();
        let mut failed_deps = 0;
        let mut loading_rec_deps = loaded_asset.dependencies.len();
        let mut failed_rec_deps = 0;
        for (dep_id, _) in loaded_asset.dependencies.iter() {
            if let Some(dep_info) = self.get_mut(*dep_id) {
                match dep_info.load_state {
                    AssetLoadState::NotLoaded | AssetLoadState::Loading => {
                        // If dependency is loading, wait for it.
                        dep_info.dependants_waiting_on_load.insert(loaded_asset.id);
                    }
                    AssetLoadState::Loaded => {
                        // If dependency is loaded, reduce our count by one
                        loading_deps -= 1;
                    }
                    AssetLoadState::Failed => {
                        failed_deps += 1;
                        loading_deps -= 1;
                    }
                }
                match dep_info.rec_dep_load_state {
                    AssetRecursiveDependencyLoadState::Loading => {
                        // If dependency is loading, wait for it.
                        dep_info
                            .dependants_waiting_on_recursive_dep_load
                            .insert(loaded_asset.id);
                    }
                    AssetRecursiveDependencyLoadState::Loaded => {
                        // If dependency is loaded, reduce our count by one
                        loading_rec_deps -= 1;
                    }
                    AssetRecursiveDependencyLoadState::Failed => {
                        failed_rec_deps += 1;
                        loading_rec_deps -= 1;
                    }
                }
            } else {
                // the dependency id no longer exists, which implies it was manually removed
                warn!(
                    "Manually removed dependency {:?} during load. This was probably a mistake",
                    dep_id
                );
                loading_deps -= 1;
                loading_rec_deps -= 1;
            }
        }

        let dep_load_state = match (loading_deps, failed_deps) {
            (0, 0) => AssetDependencyLoadState::Loaded,
            (_loading, 0) => AssetDependencyLoadState::Loading,
            (_loading, _failed) => AssetDependencyLoadState::Failed,
        };

        let rec_dep_load_state = match (loading_rec_deps, failed_rec_deps) {
            (0, 0) => AssetRecursiveDependencyLoadState::Loaded,
            (_loading, 0) => AssetRecursiveDependencyLoadState::Loading,
            (_loading, _failed) => AssetRecursiveDependencyLoadState::Failed,
        };

        let (dependants_waiting_on_load, dependants_waiting_on_rec_load) = {
            let info = self
                .get_mut(loaded_asset.id)
                .expect("Asset info should always exist at this point");
            info.loading_dependencies = loading_deps;
            info.failed_dependencies = failed_deps;
            info.loading_rec_dependencies = loading_rec_deps;
            info.failed_rec_dependencies = failed_rec_deps;
            info.load_state = AssetLoadState::Loaded;
            info.dep_load_state = dep_load_state;
            info.rec_dep_load_state = rec_dep_load_state;

            let dependants_waiting_on_rec_load = if matches!(
                rec_dep_load_state,
                AssetRecursiveDependencyLoadState::Loaded
                    | AssetRecursiveDependencyLoadState::Failed
            ) {
                Some(std::mem::take(
                    &mut info.dependants_waiting_on_recursive_dep_load,
                ))
            } else {
                None
            };

            (
                std::mem::take(&mut info.dependants_waiting_on_load),
                dependants_waiting_on_rec_load,
            )
        };

        for id in dependants_waiting_on_load {
            if let Some(info) = self.get_mut(id) {
                info.loading_dependencies -= 1;
                if info.loading_dependencies == 0 {
                    // send dependencies loaded event
                    info.dep_load_state = AssetDependencyLoadState::Loaded;
                }
            }
        }

        if let Some(dependants_waiting_on_rec_load) = dependants_waiting_on_rec_load {
            match rec_dep_load_state {
                AssetRecursiveDependencyLoadState::Loaded => {
                    for dep_id in dependants_waiting_on_rec_load {
                        Self::propagate_loaded_state(self, dep_id);
                    }
                }
                AssetRecursiveDependencyLoadState::Failed => {
                    for dep_id in dependants_waiting_on_rec_load {
                        Self::propagate_failed_state(self, dep_id);
                    }
                }
                AssetRecursiveDependencyLoadState::Loading => {
                    // dependants_waiting_on_rec_load should be None in this case
                    unreachable!("`Loading` state should never be propagated.")
                }
            }
        }
    }

    fn propagate_loaded_state(infos: &mut AssetInfos, id: UntypedAssetId) {
        let dependants_waiting_on_rec_load = if let Some(info) = infos.get_mut(id) {
            info.loading_rec_dependencies -= 1;
            if info.loading_rec_dependencies == 0 && info.failed_rec_dependencies == 0 {
                info.rec_dep_load_state = AssetRecursiveDependencyLoadState::Loaded;
                Some(std::mem::take(
                    &mut info.dependants_waiting_on_recursive_dep_load,
                ))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(dependants_waiting_on_rec_load) = dependants_waiting_on_rec_load {
            for dep_id in dependants_waiting_on_rec_load {
                Self::propagate_loaded_state(infos, dep_id);
            }
        }
    }

    fn propagate_failed_state(infos: &mut AssetInfos, id: UntypedAssetId) {
        let dependants_waiting_on_rec_load = if let Some(info) = infos.get_mut(id) {
            info.loading_rec_dependencies -= 1;
            info.failed_rec_dependencies += 1;
            info.rec_dep_load_state = AssetRecursiveDependencyLoadState::Failed;
            Some(std::mem::take(
                &mut info.dependants_waiting_on_recursive_dep_load,
            ))
        } else {
            None
        };

        if let Some(dependants_waiting_on_rec_load) = dependants_waiting_on_rec_load {
            for dep_id in dependants_waiting_on_rec_load {
                Self::propagate_failed_state(infos, dep_id);
            }
        }
    }

    fn process_asset_fail(&mut self, id: UntypedAssetId) {
        let (dependants_waiting_on_load, dependants_waiting_on_rec_load) = {
            let info = self
                .get_mut(id)
                .expect("Asset info should always exist at this point");
            info.load_state = AssetLoadState::Failed;
            info.dep_load_state = AssetDependencyLoadState::Failed;
            info.rec_dep_load_state = AssetRecursiveDependencyLoadState::Failed;
            (
                std::mem::take(&mut info.dependants_waiting_on_load),
                std::mem::take(&mut info.dependants_waiting_on_recursive_dep_load),
            )
        };

        for id in dependants_waiting_on_load {
            if let Some(info) = self.get_mut(id) {
                info.loading_dependencies -= 1;
                info.dep_load_state = AssetDependencyLoadState::Failed;
            }
        }

        for dep_id in dependants_waiting_on_rec_load {
            Self::propagate_failed_state(self, dep_id);
        }
    }

    fn process_handle_drop_internal(
        infos: &mut HashMap<UntypedAssetId, AssetInfo>,
        path_to_id: &mut HashMap<AssetPath<'static>, UntypedAssetId>,
        id: UntypedAssetId,
    ) -> bool {
        match infos.entry(id) {
            Entry::Occupied(mut entry) => {
                if entry.get_mut().handle_drops_to_skip > 0 {
                    entry.get_mut().handle_drops_to_skip -= 1;
                    false
                } else {
                    let info = entry.remove();
                    path_to_id.remove(&info.path);
                    true
                }
            }
            // Either the asset was already dropped, it doesn't exist, or it isn't managed by the asset server
            // None of these cases should result in a removal from the Assets collection
            Entry::Vacant(_) => false,
        }
    }

    /// Consumes all current handle drop events. This will update information in AssetInfos, but it
    /// will not affect [`Assets`] storages. For normal use cases, prefer `Assets::track_assets()`
    /// This should only be called if `Assets` storage isn't being used (such as in [`AssetProcessor`](crate::processor::AssetProcessor))
    pub(crate) fn consume_handle_drop_events(&mut self) {
        for provider in self.handle_providers.values() {
            while let Ok(drop_event) = provider.drop_receiver.try_recv() {
                let id = drop_event.id;
                if drop_event.asset_server_managed {
                    Self::process_handle_drop_internal(
                        &mut self.infos,
                        &mut self.path_to_id,
                        id.untyped(provider.type_id),
                    );
                }
            }
        }
    }
}

pub enum InternalAssetEvent {
    Loaded { loaded_asset: LoadedAsset },
    Failed { id: UntypedAssetId },
}

impl AssetServer {
    pub fn new(reader: Box<dyn AssetReader>) -> Self {
        Self::new_with_loaders(reader, Default::default())
    }

    pub(crate) fn new_with_loaders(
        reader: Box<dyn AssetReader>,
        loaders: Arc<RwLock<AssetLoaders>>,
    ) -> Self {
        let (asset_event_sender, asset_event_receiver) = crossbeam_channel::unbounded();
        Self {
            data: Arc::new(AssetsData {
                reader,
                asset_event_sender,
                asset_event_receiver,
                loaders,
                infos: RwLock::new(AssetInfos {
                    infos: Default::default(),
                    path_to_id: Default::default(),
                    handle_providers: Default::default(),
                }),
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
    pub fn load<'a, A: Asset, P: Into<AssetPath<'a>>>(&self, path: P) -> Handle<A> {
        self.load_untyped_internal(path, TypeId::of::<A>())
            .typed_unchecked()
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    fn load_untyped_internal<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
        type_id: TypeId,
    ) -> UntypedHandle {
        let path: AssetPath = path.into();
        trace!("Loading asset {}", path);
        let (handle, should_load) = {
            let mut infos = self.data.infos.write();
            infos.get_or_create_handle(path.to_owned(), type_id, true)
        };

        if !should_load {
            return handle;
        }

        let assets = self.clone();
        let id = handle.id();
        let owned_path = path.to_owned();
        IoTaskPool::get()
            .spawn(async move {
                match assets.load_async_internal(Some(id), owned_path, true).await {
                    Ok(loaded_asset) => {
                        assets
                            .data
                            .asset_event_sender
                            .send(InternalAssetEvent::Loaded { loaded_asset })
                            .unwrap();
                    }
                    Err(err) => {
                        assets
                            .data
                            .asset_event_sender
                            .send(InternalAssetEvent::Failed { id })
                            .unwrap();
                        error!("{}", err);
                    }
                }
            })
            .detach();
        handle
    }

    pub fn get_load_states(
        &self,
        id: impl Into<UntypedAssetId>,
    ) -> Option<(
        AssetLoadState,
        AssetDependencyLoadState,
        AssetRecursiveDependencyLoadState,
    )> {
        self.data
            .infos
            .read()
            .get(id.into())
            .map(|i| (i.load_state, i.dep_load_state, i.rec_dep_load_state))
    }

    /// Returns an active handle for the given path, if the asset at the given path has already started loading,
    /// or is still "alive".
    pub fn get_handle<'a, C: Component, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Option<Handle<C>> {
        let mut infos = self.data.infos.write();
        let path = path.into();
        infos.get_path_handle(path).map(|h| h.typed_unchecked())
    }

    /// Retrieve a handle for the given path. This will create a handle (and AssetInfo) if it does not exist
    pub(crate) fn get_path_handle(
        &self,
        path: AssetPath<'static>,
        type_id: TypeId,
    ) -> UntypedHandle {
        let mut infos = self.data.infos.write();
        infos.get_or_create_handle(path, type_id, false).0
    }

    pub async fn load_direct_with_meta_async<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
        meta: &dyn AssetMetaDyn,
    ) -> Result<LoadedAsset, AssetLoadError> {
        let path: AssetPath = path.into();
        // TODO: handle this error
        let loader = self
            .get_erased_asset_loader_with_type_name(meta.source_loader())
            .unwrap();
        self.load_with_meta_and_loader_internal(None, path, meta, &*loader, false)
            .await
    }

    pub async fn load_direct_async<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Result<LoadedAsset, AssetLoadError> {
        let path: AssetPath = path.into();
        let asset = self.load_async_internal(None, path, false).await?;
        Ok(asset)
    }

    async fn load_async_internal(
        &self,
        id: Option<UntypedAssetId>,
        asset_path: AssetPath<'_>,
        load_dependencies: bool,
    ) -> Result<LoadedAsset, AssetLoadError> {
        let (meta, loader) = match self.data.reader.read_meta(asset_path.path()).await {
            Ok(mut meta_reader) => {
                let mut meta_bytes = Vec::new();
                meta_reader
                    .read_to_end(&mut meta_bytes)
                    .await
                    .map_err(|_| AssetLoadError::AssetMetaReadError)?;
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
                (meta, loader)
            }
            Err(AssetReaderError::NotFound(_)) => {
                let loader = self.get_path_asset_loader(asset_path.path())?;
                let meta = loader.default_meta();
                (meta, loader)
            }
            Err(err) => return Err(err.into()),
        };

        self.load_with_meta_and_loader_internal(id, asset_path, &*meta, &*loader, load_dependencies)
            .await
    }

    async fn load_with_meta_and_loader_internal(
        &self,
        mut id: Option<UntypedAssetId>,
        asset_path: AssetPath<'_>,
        meta: &dyn AssetMetaDyn,
        loader: &dyn ErasedAssetLoader,
        load_dependencies: bool,
    ) -> Result<LoadedAsset, AssetLoadError> {
        if asset_path.label().is_some() {
            // if the path is to a label, the current id (if it was passed in) does not match the asset root type
            // we need to get a new asset id
            id.take();
        }
        let id = id.unwrap_or_else(|| {
            self.get_path_handle(
                asset_path.without_label().to_owned(),
                loader.asset_type_id(),
            )
            .id()
        });
        let mut reader = self.data.reader.read(asset_path.path()).await?;
        let load_context = LoadContext::new(self, id, &asset_path, load_dependencies);
        loader
            .load(&mut reader, meta.source_loader_settings(), load_context)
            .await
            .map_err(|e| AssetLoadError::AssetLoaderError {
                loader: loader.type_name(),
                path: asset_path.to_owned(),
                error: e,
            })
    }
}

pub fn handle_internal_asset_events(world: &mut World) {
    world.resource_scope(|world, assets: Mut<AssetServer>| {
        let mut infos = assets.data.infos.write();
        for event in assets.data.asset_event_receiver.try_iter() {
            match event {
                InternalAssetEvent::Loaded { loaded_asset } => {
                    infos.process_asset_load(loaded_asset, world);
                }
                InternalAssetEvent::Failed { id } => infos.process_asset_fail(id),
            }
        }
    })
}

/// The load state of an asset.
#[derive(Component, Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AssetLoadState {
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
pub enum AssetDependencyLoadState {
    /// Dependencies are still loading
    Loading,
    /// Dependencies have all loaded
    Loaded,
    /// One or more dependencies have failed to load
    Failed,
}

/// The recursive load state of an asset's dependencies.
#[derive(Component, Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AssetRecursiveDependencyLoadState {
    /// Dependencies in this asset's dependency tree are still loading
    Loading,
    /// Dependencies in this asset's dependency tree have all loaded
    Loaded,
    /// One or more dependencies have failed to load in this asset's dependency tree
    Failed,
}

#[derive(Error, Debug)]

pub enum AssetLoadError {
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
