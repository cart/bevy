use crate::{
    folder::LoadedFolder,
    io::{AssetReader, AssetReaderError},
    loader::{AssetLoader, AssetLoaderError, ErasedAssetLoader, LoadContext, LoadedAsset},
    meta::{AssetMetaDyn, AssetMetaMinimal},
    path::AssetPath,
    Asset, AssetHandleProvider, Assets, ErasedLoadedAsset, Handle, InternalAssetHandle,
    UntypedAssetId, UntypedHandle,
};
use bevy_ecs::prelude::*;
use bevy_log::{error, warn};
use bevy_tasks::IoTaskPool;
use bevy_utils::{Entry, HashMap, HashSet};
use crossbeam_channel::{Receiver, Sender};
use futures_lite::{AsyncReadExt, FutureExt, StreamExt};
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
    path: Option<AssetPath<'static>>,
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
    fn new(weak_handle: Weak<InternalAssetHandle>, path: Option<AssetPath<'static>>) -> Self {
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

#[derive(Copy, Clone, PartialEq, Eq)]
enum HandleLoadingMode {
    NotLoading,
    Request,
    Force,
}

impl AssetInfos {
    fn create_loading_handle(&mut self, type_id: TypeId) -> UntypedHandle {
        Self::create_handle_internal(&mut self.infos, &self.handle_providers, type_id, None, true)
    }

    fn create_handle_internal(
        infos: &mut HashMap<UntypedAssetId, AssetInfo>,
        handle_providers: &HashMap<TypeId, AssetHandleProvider>,
        type_id: TypeId,
        path: Option<AssetPath<'static>>,
        loading: bool,
    ) -> UntypedHandle {
        let provider = handle_providers.get(&type_id).unwrap_or_else(|| {
            panic!(
                "Cannot allocate a handle for asset of type {:?} because it does not exist",
                type_id
            )
        });

        let handle = provider.reserve_handle_internal(true);
        let mut info = AssetInfo::new(Arc::downgrade(&handle), path);
        if loading {
            info.load_state = AssetLoadState::Loading;
        }
        infos.insert(handle.id, info);
        UntypedHandle::Strong(handle)
    }

    /// Retrieves asset tracking data, or creates it if it doesn't exist.
    /// Returns true if an asset load should be kicked off
    fn get_or_create_path_handle(
        &mut self,
        path: AssetPath<'static>,
        type_id: TypeId,
        loading_mode: HandleLoadingMode,
    ) -> (UntypedHandle, bool) {
        match self.path_to_id.entry(path.clone()) {
            Entry::Occupied(entry) => {
                let id = *entry.get();
                // if there is a path_to_id entry, info always exists
                let info = self.infos.get_mut(&id).unwrap();
                let mut should_load = false;
                if loading_mode == HandleLoadingMode::Force
                    || (loading_mode == HandleLoadingMode::Request
                        && info.load_state == AssetLoadState::NotLoaded)
                {
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
                let should_load = match loading_mode {
                    HandleLoadingMode::NotLoading => false,
                    HandleLoadingMode::Request => true,
                    HandleLoadingMode::Force => true,
                };
                let handle = Self::create_handle_internal(
                    &mut self.infos,
                    &self.handle_providers,
                    type_id,
                    Some(path),
                    should_load,
                );
                entry.insert(handle.id());
                (handle, should_load)
            }
        }
    }

    fn get(&self, id: UntypedAssetId) -> Option<&AssetInfo> {
        self.infos.get(&id)
    }

    fn get_mut(&mut self, id: UntypedAssetId) -> Option<&mut AssetInfo> {
        self.infos.get_mut(&id)
    }

    fn get_path_handle(&self, path: AssetPath) -> Option<UntypedHandle> {
        let id = *self.path_to_id.get(&path)?;
        let info = self.infos.get(&id)?;
        let strong_handle = info.weak_handle.upgrade()?;
        Some(UntypedHandle::Strong(strong_handle))
    }

    // Returns `true` if the asset should be removed from the collection
    pub(crate) fn process_handle_drop(&mut self, id: UntypedAssetId) -> bool {
        Self::process_handle_drop_internal(&mut self.infos, &mut self.path_to_id, id)
    }

    fn process_asset_load(
        &mut self,
        loaded_asset_id: UntypedAssetId,
        loaded_asset: ErasedLoadedAsset,
        world: &mut World,
    ) {
        loaded_asset.value.insert(loaded_asset_id, world);
        let mut loading_deps = loaded_asset.dependencies.len();
        let mut failed_deps = 0;
        let mut loading_rec_deps = loaded_asset.dependencies.len();
        let mut failed_rec_deps = 0;
        for dep_id in loaded_asset.dependencies.iter() {
            if let Some(dep_info) = self.get_mut(dep_id.id()) {
                match dep_info.load_state {
                    AssetLoadState::NotLoaded | AssetLoadState::Loading => {
                        // If dependency is loading, wait for it.
                        dep_info.dependants_waiting_on_load.insert(loaded_asset_id);
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
                            .insert(loaded_asset_id);
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
                .get_mut(loaded_asset_id)
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
                    if let Some(path) = info.path {
                        path_to_id.remove(&path);
                    }
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
    Loaded {
        id: UntypedAssetId,
        loaded_asset: ErasedLoadedAsset,
    },
    Failed {
        id: UntypedAssetId,
    },
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
                    if let Err(err) = server.load_internal(Some(owned_handle), owned_path).await {
                        error!("{}", err);
                    }
                })
                .detach();
        }

        handle.typed_debug_checked()
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub(crate) async fn load_untyped_async<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Result<UntypedHandle, AssetLoadError> {
        self.load_internal(None, path.into()).await
    }

    async fn load_internal<'a>(
        &self,
        input_handle: Option<UntypedHandle>,
        mut path: AssetPath<'a>,
    ) -> Result<UntypedHandle, AssetLoadError> {
        let (meta, loader) = self.get_meta_and_loader(&path).await.map_err(|e| {
            // if there was an input handle, a "load" operation has already started, so we must produce a "failure" event, if
            // we cannot find the meta and loader
            if let Some(handle) = &input_handle {
                self.send_asset_event(InternalAssetEvent::Failed { id: handle.id() });
            }
            e
        })?;

        let has_label = path.label().is_none();

        let (handle, should_load) = match input_handle {
            Some(handle) => {
                // TODO: add requested type validation for sub assets
                if !has_label && handle.type_id() != loader.asset_type_id() {
                    return Err(AssetLoadError::RequestedHandleTypeMismatch {
                        path: path.to_owned(),
                        requested: handle.type_id(),
                        actual: loader.type_id(),
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

        if !should_load {
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
            .load_with_meta_and_loader(path, &*meta, &*loader, true)
            .await
        {
            Ok(loaded_asset) => {
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

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn add<A: Asset>(&self, asset: A) -> Handle<A> {
        self.load_asset(LoadedAsset::from(asset))
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_asset<A: Asset>(&self, asset: impl Into<LoadedAsset<A>>) -> Handle<A> {
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
    pub fn load_folder<P: AsRef<Path>>(&self, path: P) -> Handle<LoadedFolder> {
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
                        loaded_asset: LoadedAsset::from(LoadedFolder { handles }).into(),
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

    pub fn get_load_state(&self, id: impl Into<UntypedAssetId>) -> Option<AssetLoadState> {
        self.data.infos.read().get(id.into()).map(|i| i.load_state)
    }

    pub fn load_state(&self, id: impl Into<UntypedAssetId>) -> AssetLoadState {
        let id = id.into();
        let infos = self.data.infos.read();
        infos
            .get(id)
            .map(|i| i.load_state)
            .unwrap_or(AssetLoadState::NotLoaded)
    }

    /// Returns an active handle for the given path, if the asset at the given path has already started loading,
    /// or is still "alive".
    pub fn get_handle<'a, A: Asset, P: Into<AssetPath<'a>>>(&self, path: P) -> Option<Handle<A>> {
        self.get_handle_untyped(path)
            .map(|h| h.typed_debug_checked())
    }

    pub fn get_handle_untyped<'a, P: Into<AssetPath<'a>>>(&self, path: P) -> Option<UntypedHandle> {
        let infos = self.data.infos.read();
        let path = path.into();
        infos.get_path_handle(path)
    }

    pub fn get_asset_path(&self, id: impl Into<UntypedAssetId>) -> Option<AssetPath<'static>> {
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

    pub async fn load_direct_with_meta_async<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
        meta: &dyn AssetMetaDyn,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let path: AssetPath = path.into();
        // TODO: handle this error
        let loader = self
            .get_erased_asset_loader_with_type_name(meta.source_loader())
            .unwrap();
        self.load_with_meta_and_loader(path, meta, &*loader, false)
            .await
    }

    pub async fn load_direct_async<'a, P: Into<AssetPath<'a>>>(
        &self,
        path: P,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let path: AssetPath = path.into();
        let (meta, loader) = self.get_meta_and_loader(&path).await?;
        self.load_with_meta_and_loader(path, &*meta, &*loader, false)
            .await
    }

    async fn get_meta_and_loader(
        &self,
        asset_path: &AssetPath<'_>,
    ) -> Result<(Box<dyn AssetMetaDyn>, Arc<dyn ErasedAssetLoader>), AssetLoadError> {
        match self.data.reader.read_meta(asset_path.path()).await {
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
        meta: &dyn AssetMetaDyn,
        loader: &dyn ErasedAssetLoader,
        load_dependencies: bool,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let mut reader = self.data.reader.read(asset_path.path()).await?;
        let load_context = LoadContext::new(self, asset_path.to_owned(), load_dependencies);
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
                InternalAssetEvent::Loaded { id, loaded_asset } => {
                    infos.process_asset_load(id, loaded_asset, world);
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
    // TODO: producer of this error should look up friendly type names
    #[error("Requested handle of type {requested:?} for asset '{path}' does not match actual asset type {actual:?}")]
    RequestedHandleTypeMismatch {
        path: AssetPath<'static>,
        requested: TypeId,
        actual: TypeId,
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
