mod info;
mod loaders;

use crate::{
    folder::LoadedFolder,
    io::{
        AssetReaderError, AssetSource, AssetSourceEvent, AssetSourceId, AssetSources,
        AssetWriterError, ErasedAssetReader, MissingAssetSourceError, MissingAssetWriterError,
        MissingProcessedAssetReaderError, Reader,
    },
    loader::{AssetLoader, ErasedAssetLoader, LoadContext, LoadedAsset},
    meta::{
        loader_settings_meta_transform, AssetActionMinimal, AssetMetaDyn, AssetMetaMinimal,
        MetaTransform, Settings,
    },
    path::AssetPath,
    Asset, AssetData, AssetId, AssetMetaCheck, AssetReference, DeserializeMetaError,
    ErasedLoadedAsset, Handle, LoadFailed, LoadedWithDependencies, UnapprovedPathMode,
    UntypedHandle, VisitAssetDependencies,
};
use alloc::{borrow::ToOwned, boxed::Box, vec, vec::Vec};
use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
};
use bevy_diagnostic::{DiagnosticPath, Diagnostics};
use bevy_ecs::{entity::RemoteAllocator, prelude::*};
use bevy_platform::{
    collections::HashSet,
    sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};
use bevy_tasks::IoTaskPool;
use bevy_utils::default;
use core::{any::TypeId, future::Future, panic::AssertUnwindSafe, task::Poll};
use crossbeam_channel::{Receiver, Sender};
use futures_lite::{FutureExt, StreamExt};
use info::*;
use loaders::*;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Loads and tracks the state of [`Asset`] values from a configured [`AssetReader`](crate::io::AssetReader).
/// This can be used to kick off new asset loads and retrieve their current load states.
///
/// The general process to load an asset is:
/// 1. Initialize a new [`Asset`] type with the [`AssetServer`] via [`AssetApp::init_asset`], which
///    will internally call [`AssetServer::register_asset`] and set up related ECS [`Assets`]
///    storage and systems.
/// 2. Register one or more [`AssetLoader`]s for that asset with [`AssetApp::init_asset_loader`]
/// 3. Add the asset to your asset folder (defaults to `assets`).
/// 4. Call [`AssetServer::load`] with a path to your asset.
///
/// [`AssetServer`] can be cloned. It is backed by an [`Arc`] so clones will share state. Clones can be freely used in parallel.
///
/// [`AssetApp::init_asset`]: crate::AssetApp::init_asset
/// [`AssetApp::init_asset_loader`]: crate::AssetApp::init_asset_loader
#[derive(Resource, Clone)]
pub struct AssetServer {
    pub(crate) data: Arc<AssetServerData>,
}

/// Internal data used by [`AssetServer`]. This is intended to be used from within an [`Arc`].
pub(crate) struct AssetServerData {
    pub(crate) infos: RwLock<AssetInfos>,
    pub(crate) loaders: Arc<RwLock<AssetLoaders>>,
    pub(crate) remote_allocator: RemoteAllocator,
    asset_event_sender: Sender<InternalAssetEvent>,
    asset_event_receiver: Receiver<InternalAssetEvent>,
    sources: Arc<AssetSources>,
    mode: AssetServerMode,
    meta_check: AssetMetaCheck,
    unapproved_path_mode: UnapprovedPathMode,
}

/// The "asset mode" the server is currently in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetServerMode {
    /// This server loads unprocessed assets.
    Unprocessed,
    /// This server loads processed assets.
    Processed,
}

impl AssetServer {
    /// The number of loads that have been started by the server.
    pub const STARTED_LOAD_COUNT: DiagnosticPath = DiagnosticPath::const_new("started_load_count");

    /// Create a new instance of [`AssetServer`]. If `watch_for_changes` is true, the [`AssetReader`](crate::io::AssetReader) storage will watch for changes to
    /// asset sources and hot-reload them.
    pub fn new(
        sources: Arc<AssetSources>,
        remote_allocator: RemoteAllocator,
        mode: AssetServerMode,
        watching_for_changes: bool,
        unapproved_path_mode: UnapprovedPathMode,
    ) -> Self {
        Self::new_with_loaders(
            sources,
            Default::default(),
            remote_allocator,
            mode,
            AssetMetaCheck::Always,
            watching_for_changes,
            unapproved_path_mode,
        )
    }

    /// Create a new instance of [`AssetServer`]. If `watch_for_changes` is true, the [`AssetReader`](crate::io::AssetReader) storage will watch for changes to
    /// asset sources and hot-reload them.
    pub fn new_with_meta_check(
        sources: Arc<AssetSources>,
        remote_allocator: RemoteAllocator,
        mode: AssetServerMode,
        meta_check: AssetMetaCheck,
        watching_for_changes: bool,
        unapproved_path_mode: UnapprovedPathMode,
    ) -> Self {
        Self::new_with_loaders(
            sources,
            Default::default(),
            remote_allocator,
            mode,
            meta_check,
            watching_for_changes,
            unapproved_path_mode,
        )
    }

    pub(crate) fn new_with_loaders(
        sources: Arc<AssetSources>,
        loaders: Arc<RwLock<AssetLoaders>>,
        remote_allocator: RemoteAllocator,
        mode: AssetServerMode,
        meta_check: AssetMetaCheck,
        watching_for_changes: bool,
        unapproved_path_mode: UnapprovedPathMode,
    ) -> Self {
        let (asset_event_sender, asset_event_receiver) = crossbeam_channel::unbounded();
        let mut infos = AssetInfos::new(remote_allocator.clone());
        infos.watching_for_changes = watching_for_changes;
        Self {
            data: Arc::new(AssetServerData {
                sources,
                remote_allocator,
                mode,
                meta_check,
                asset_event_sender,
                asset_event_receiver,
                loaders,
                infos: RwLock::new(infos),
                unapproved_path_mode,
            }),
        }
    }

    pub(crate) fn read_infos(&self) -> RwLockReadGuard<'_, AssetInfos> {
        self.data
            .infos
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn write_infos(&self) -> RwLockWriteGuard<'_, AssetInfos> {
        self.data
            .infos
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn read_loaders(&self) -> RwLockReadGuard<'_, AssetLoaders> {
        self.data
            .loaders
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn write_loaders(&self) -> RwLockWriteGuard<'_, AssetLoaders> {
        self.data
            .loaders
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Retrieves the [`AssetSource`] for the given `source`.
    pub fn get_source<'a>(
        &self,
        source: impl Into<AssetSourceId<'a>>,
    ) -> Result<&AssetSource, MissingAssetSourceError> {
        self.data.sources.get(source.into())
    }

    /// Returns true if the [`AssetServer`] watches for changes.
    pub fn watching_for_changes(&self) -> bool {
        self.read_infos().watching_for_changes
    }

    /// Registers a new [`AssetLoader`]. [`AssetLoader`]s must be registered before they can be used.
    pub fn register_loader<L: AssetLoader>(&self, loader: L) {
        self.write_loaders().push(loader);
    }

    /// Returns the registered [`AssetLoader`] associated with the given extension, if it exists.
    pub async fn get_asset_loader_with_extension(
        &self,
        extension: &str,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForExtensionError> {
        let error = || MissingAssetLoaderForExtensionError {
            extensions: vec![extension.to_string()],
        };

        let loader = self
            .read_loaders()
            .get_by_extension(extension)
            .ok_or_else(error)?;
        loader.get().await.map_err(|_| error())
    }

    /// Returns the registered [`AssetLoader`] associated with the given type name, if it exists.
    pub async fn get_asset_loader_with_type_name(
        &self,
        type_name: &str,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForTypeNameError> {
        let error = || MissingAssetLoaderForTypeNameError {
            type_name: type_name.to_string(),
        };

        let loader = self
            .read_loaders()
            .get_by_name(type_name)
            .ok_or_else(error)?;
        loader.get().await.map_err(|_| error())
    }

    /// Retrieves the default [`AssetLoader`] for the given path, if one can be found.
    pub async fn get_path_asset_loader<'a>(
        &self,
        path: impl Into<AssetPath<'a>>,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForExtensionError> {
        let path = path.into();

        let error = || {
            let Some(full_extension) = path.get_full_extension() else {
                return MissingAssetLoaderForExtensionError {
                    extensions: Vec::new(),
                };
            };

            let mut extensions = vec![full_extension.to_string()];
            extensions.extend(
                AssetPath::iter_secondary_extensions(full_extension).map(ToString::to_string),
            );

            MissingAssetLoaderForExtensionError { extensions }
        };

        let loader = self.read_loaders().get_by_path(&path).ok_or_else(error)?;
        loader.get().await.map_err(|_| error())
    }

    /// Retrieves the default [`AssetLoader`] for the given [`Asset`] [`TypeId`], if one can be found.
    pub async fn get_asset_loader_with_asset_type_id(
        &self,
        type_id: TypeId,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForTypeIdError> {
        let error = || MissingAssetLoaderForTypeIdError { type_id };

        let loader = self.read_loaders().get_by_type(type_id).ok_or_else(error)?;
        loader.get().await.map_err(|_| error())
    }

    /// Retrieves the default [`AssetLoader`] for the given [`Asset`] type, if one can be found.
    pub async fn get_asset_loader_with_asset_type<A: Asset>(
        &self,
    ) -> Result<Arc<dyn ErasedAssetLoader>, MissingAssetLoaderForTypeIdError> {
        self.get_asset_loader_with_asset_type_id(TypeId::of::<A>())
            .await
    }

    /// Begins loading an [`Asset`] of type `A` stored at `path`. This will not block on the asset load. Instead,
    /// it returns a "strong" [`Handle`]. When the [`Asset`] is loaded (and enters [`LoadState::Loaded`]), it will be added to the
    /// associated [`Assets`] resource.
    ///
    /// Note that if the asset at this path is already loaded, this function will return the existing handle,
    /// and will not waste work spawning a new load task.
    ///
    /// In case the file path contains a hashtag (`#`), the `path` must be specified using [`Path`]
    /// or [`AssetPath`] because otherwise the hashtag would be interpreted as separator between
    /// the file path and the label. For example:
    ///
    /// ```no_run
    /// # use bevy_asset::{AssetServer, Handle, LoadedUntypedAsset};
    /// # use bevy_ecs::prelude::Res;
    /// # use std::path::Path;
    /// // `#path` is a label.
    /// # fn setup(asset_server: Res<AssetServer>) {
    /// # let handle: Handle<LoadedUntypedAsset> =
    /// asset_server.load("some/file#path");
    ///
    /// // `#path` is part of the file name.
    /// # let handle: Handle<LoadedUntypedAsset> =
    /// asset_server.load(Path::new("some/file#path"));
    /// # }
    /// ```
    ///
    /// Furthermore, if you need to load a file with a hashtag in its name _and_ a label, you can
    /// manually construct an [`AssetPath`].
    ///
    /// ```no_run
    /// # use bevy_asset::{AssetPath, AssetServer, Handle, LoadedUntypedAsset};
    /// # use bevy_ecs::prelude::Res;
    /// # use std::path::Path;
    /// # fn setup(asset_server: Res<AssetServer>) {
    /// # let handle: Handle<LoadedUntypedAsset> =
    /// asset_server.load(AssetPath::from_path(Path::new("some/file#path")).with_label("subasset"));
    /// # }
    /// ```
    ///
    /// You can check the asset's load state by reading [`AssetEvent`] events, calling [`AssetServer::load_state`], or checking
    /// the [`Assets`] storage to see if the [`Asset`] exists yet.
    ///
    /// The asset load will fail and an error will be printed to the logs if the asset stored at `path` is not of type `A`.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load<'a, A: Asset>(&self, path: impl Into<AssetReference<'a>>) -> Handle<A> {
        self.load_builder().load(path.into())
    }

    /// Returns a [`LoadBuilder`] that can be used to start more complex loads. See [`LoadBuilder`]
    /// for details.
    #[must_use = "the load doesn't start until LoadBuilder has been consumed"]
    pub fn load_builder(&self) -> LoadBuilder<'_> {
        LoadBuilder::new(self)
    }

    /// Same as [`load`](AssetServer::load), but you can load assets from unapproved paths
    /// if [`AssetPlugin::unapproved_path_mode`](super::AssetPlugin::unapproved_path_mode)
    /// is [`Deny`](UnapprovedPathMode::Deny).
    ///
    /// See [`UnapprovedPathMode`] and [`AssetPath::is_unapproved`]
    #[deprecated(
        note = "Use `asset_server.load_builder().override_unapproved().load(path)` instead"
    )]
    pub fn load_override<'a, A: Asset>(&self, path: impl Into<AssetReference<'a>>) -> Handle<A> {
        self.load_builder().override_unapproved().load(path.into())
    }

    /// Same as [`load`](Self::load), but the type of the asset to load is specified by the runtime
    /// `type_id`.
    #[deprecated(note = "Use `asset_server.load_builder().load_erased(type_id, path)` instead")]
    pub fn load_erased<'a>(
        &self,
        type_id: TypeId,
        path: impl Into<AssetPath<'a>>,
    ) -> UntypedHandle {
        self.load_builder().load_erased(type_id, path.into())
    }

    /// Begins loading an [`Asset`] of type `A` stored at `path` while holding a guard item.
    /// The guard item is dropped when either the asset is loaded or loading has failed.
    ///
    /// This function returns a "strong" [`Handle`]. When the [`Asset`] is loaded (and enters [`LoadState::Loaded`]), it will be added to the
    /// associated [`Assets`] resource.
    ///
    /// The guard item should notify the caller in its [`Drop`] implementation. See example `multi_asset_sync`.
    /// Synchronously this can be a [`Arc<AtomicU32>`] that decrements its counter, asynchronously this can be a `Barrier`.
    /// This function only guarantees the asset referenced by the [`Handle`] is loaded. If your asset is separated into
    /// multiple files, sub-assets referenced by the main asset might still be loading, depend on the implementation of the [`AssetLoader`].
    ///
    /// Additionally, you can check the asset's load state by reading [`AssetEvent`] events, calling [`AssetServer::load_state`], or checking
    /// the [`Assets`] storage to see if the [`Asset`] exists yet.
    ///
    /// The asset load will fail and an error will be printed to the logs if the asset stored at `path` is not of type `A`.
    #[deprecated(note = "Use `asset_server.load_builder().with_guard(guard).load(path)` instead")]
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_acquire<'a, A: Asset, G: Send + Sync + 'static>(
        &self,
        path: impl Into<AssetReference<'a>>,
        guard: G,
    ) -> Handle<A> {
        self.load_builder().with_guard(guard).load(path.into())
    }

    /// Same as [`load`](AssetServer::load_acquire), but you can load assets from unapproved paths
    /// if [`AssetPlugin::unapproved_path_mode`](super::AssetPlugin::unapproved_path_mode)
    /// is [`Deny`](UnapprovedPathMode::Deny).
    ///
    /// See [`UnapprovedPathMode`] and [`AssetPath::is_unapproved`]
    #[deprecated(
        note = "Use `asset_server.load_builder().with_guard(guard).override_unapproved().load(path)` instead"
    )]
    pub fn load_acquire_override<'a, A: Asset, G: Send + Sync + 'static>(
        &self,
        path: impl Into<AssetReference<'a>>,
        guard: G,
    ) -> Handle<A> {
        self.load_builder()
            .with_guard(guard)
            .override_unapproved()
            .load(path.into())
    }

    /// Begins loading an [`Asset`] of type `A` stored at `path`. The given `settings` function will override the asset's
    /// [`AssetLoader`] settings. The type `S` _must_ match the configured [`AssetLoader::Settings`] or `settings` changes
    /// will be ignored and an error will be printed to the log.
    #[deprecated(
        note = "Use `asset_server.load_builder().with_settings(settings).load(path)` instead"
    )]
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_with_settings<'a, A: Asset, S: Settings>(
        &self,
        path: impl Into<AssetReference<'a>>,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
    ) -> Handle<A> {
        self.load_builder()
            .with_settings(settings)
            .load(path.into())
    }

    /// Same as [`load`](AssetServer::load_with_settings), but you can load assets from unapproved paths
    /// if [`AssetPlugin::unapproved_path_mode`](super::AssetPlugin::unapproved_path_mode)
    /// is [`Deny`](UnapprovedPathMode::Deny).
    ///
    /// See [`UnapprovedPathMode`] and [`AssetPath::is_unapproved`]
    #[deprecated(
        note = "Use `asset_server.load_builder().with_settings(settings).override_unapproved().load(path)` instead"
    )]
    pub fn load_with_settings_override<'a, A: Asset, S: Settings>(
        &self,
        path: impl Into<AssetReference<'a>>,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
    ) -> Handle<A> {
        self.load_builder()
            .with_settings(settings)
            .override_unapproved()
            .load(path.into())
    }

    /// Begins loading an [`Asset`] of type `A` stored at `path` while holding a guard item.
    /// The guard item is dropped when either the asset is loaded or loading has failed.
    ///
    /// This function only guarantees the asset referenced by the [`Handle`] is loaded. If your asset is separated into
    /// multiple files, sub-assets referenced by the main asset might still be loading, depend on the implementation of the [`AssetLoader`].
    ///
    /// The given `settings` function will override the asset's
    /// [`AssetLoader`] settings. The type `S` _must_ match the configured [`AssetLoader::Settings`] or `settings` changes
    /// will be ignored and an error will be printed to the log.
    #[deprecated(
        note = "Use `asset_server.load_builder().with_guard(guard).with_settings(settings).load(path)` instead"
    )]
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_acquire_with_settings<'a, A: Asset, S: Settings, G: Send + Sync + 'static>(
        &self,
        path: impl Into<AssetReference<'a>>,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
        guard: G,
    ) -> Handle<A> {
        self.load_builder()
            .with_guard(guard)
            .with_settings(settings)
            .load(path.into())
    }

    /// Same as [`load`](AssetServer::load_acquire_with_settings), but you can load assets from unapproved paths
    /// if [`AssetPlugin::unapproved_path_mode`](super::AssetPlugin::unapproved_path_mode)
    /// is [`Deny`](UnapprovedPathMode::Deny).
    ///
    /// See [`UnapprovedPathMode`] and [`AssetPath::is_unapproved`]
    #[deprecated(
        note = "Use `asset_server.load_builder().with_guard(guard).with_settings(settings).override_unapproved().load(path)` instead"
    )]
    pub fn load_acquire_with_settings_override<
        'a,
        A: Asset,
        S: Settings,
        G: Send + Sync + 'static,
    >(
        &self,
        path: impl Into<AssetReference<'a>>,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
        guard: G,
    ) -> Handle<A> {
        self.load_builder()
            .with_guard(guard)
            .with_settings(settings)
            .override_unapproved()
            .load(path.into())
    }

    pub(crate) fn load_guarded<'a, G: Send + Sync + 'static>(
        &self,
        data: AssetData,
        guard: G,
        override_unapproved: bool,
    ) -> UntypedHandle {
        if let Some(path) = &data.path {
            if path.path() == Path::new("") {
                // TODO: Now that handles are always strong, this should probably be an error?
                // Or should we have a collection of "default UUID" handles?
                panic!("Attempted to load an asset with an empty path \"{path}\"!");
            }

            if path.is_unapproved() {
                match (&self.data.unapproved_path_mode, override_unapproved) {
                    (UnapprovedPathMode::Allow, _) | (UnapprovedPathMode::Deny, true) => {}
                    (UnapprovedPathMode::Deny, false) | (UnapprovedPathMode::Forbid, _) => {
                        // TODO: Now that handles are always strong, this should probably be an error?
                        // Or should we have a collection of "default UUID" handles?
                        panic!(
                            "Asset path {path} is unapproved. See UnapprovedPathMode for details."
                        );
                    }
                }
            }
        }

        let mut infos = self.write_infos();
        let (handle, should_load) = infos.get_or_create_handle(HandleLoadingMode::Request, data);

        if should_load {
            infos.stats.started_load_tasks += 1;

            // drop the lock on `AssetInfos` before spawning a task that may block on it in single-threaded
            #[cfg(any(target_arch = "wasm32", not(feature = "multi_threaded")))]
            drop(infos);

            let owned_handle = handle.clone();
            let server = self.clone();
            let task = IoTaskPool::get().spawn(async move {
                if let Err(err) = server.load_internal(owned_handle).await {
                    error!("{}", err);
                }
                drop(guard);
            });

            #[cfg(not(any(target_arch = "wasm32", not(feature = "multi_threaded"))))]
            {
                let mut infos = infos;
                infos.pending_tasks.insert(handle.entity(), task);
            }

            #[cfg(any(target_arch = "wasm32", not(feature = "multi_threaded")))]
            task.detach();
        }

        handle
    }

    /// Performs an async asset load. This will "reload" the asset if it already exists.
    async fn load_internal<'a>(&self, handle: UntypedHandle) -> Result<(), AssetLoadError> {
        let type_id_hint = handle.type_id_hint();

        let Some(path) = handle.path() else {
            // TODO: support UUID
            return Err(AssetLoadError::EmptyPath("".into()));
        };

        let (mut meta, loader, mut reader) = self
            .get_meta_loader_and_reader(&path, type_id_hint)
            .await
            .inspect_err(|e| {
                self.send_asset_event(InternalAssetEvent::Failed {
                    entity: handle.entity(),
                    error: e.clone(),
                });
            })?;

        if let Some(meta_transform) = handle.meta_transform() {
            (*meta_transform)(&mut *meta);
        }

        // We don't actually need to use _base_handle, but we do need to keep the handle alive.
        // Dropping it would cancel the load of the base asset, which would make the load of this
        // subasset never complete.
        let (base_path_entity, _base_handle, base_path) = if path.label().is_some() {
            let mut infos = self.write_infos();
            let base_path = path.without_label().into_owned();
            let base_handle = infos
                .get_or_create_handle(
                    HandleLoadingMode::Force,
                    AssetData {
                        path: Some(base_path.clone()),
                        type_id_hint: Some(loader.asset_type_id()),
                        ..default()
                    },
                )
                .0;
            (base_handle.entity(), Some(base_handle), base_path)
        } else {
            (handle.entity(), None, path.clone())
        };

        match self
            .load_with_settings_loader_and_reader(
                &base_path,
                meta.loader_settings().expect("meta is set to Load"),
                &*loader,
                &mut *reader,
                true,
                false,
            )
            .await
        {
            Ok(loaded_asset) => {
                if let Some(label) = path.label_cow() {
                    match loaded_asset.label_to_asset_index.get(&label) {
                        Some(labeled_asset) => {
                            let labeled_asset = &loaded_asset.labeled_assets[*labeled_asset];
                            // If we know the requested type then check it
                            // matches the labeled asset.
                            if let Some(type_id_hint) = type_id_hint
                                && type_id_hint != labeled_asset.asset.asset_type_id()
                            {
                                let error: AssetLoadError =
                                    Box::new(RequestedHandleTypeMismatchError {
                                        path: path.clone(),
                                        requested: type_id_hint,
                                        actual_asset_name: labeled_asset
                                            .asset
                                            .value
                                            .asset_type_name(),
                                        loader_name: loader.type_path(),
                                    })
                                    .into();
                                self.send_asset_event(InternalAssetEvent::Failed {
                                    entity: handle.entity(),
                                    error: error.clone(),
                                });
                                return Err(error);
                            }
                        }
                        None => {
                            let mut all_labels: Vec<String> = loaded_asset
                                .label_to_asset_index
                                .keys()
                                .map(|s| (**s).to_owned())
                                .collect();
                            all_labels.sort_unstable();
                            let error = AssetLoadError::MissingLabel {
                                base_path,
                                label: label.to_string(),
                                all_labels,
                            };
                            self.send_asset_event(InternalAssetEvent::Failed {
                                entity: handle.entity(),
                                error: error.clone(),
                            });
                            return Err(error);
                        }
                    }
                }

                self.send_asset_event(InternalAssetEvent::Loaded {
                    entity: base_path_entity,
                    loaded_asset,
                });
                Ok(())
            }
            Err(err) => {
                self.send_asset_event(InternalAssetEvent::Failed {
                    entity: handle.entity(),
                    error: err.clone(),
                });
                Err(err)
            }
        }
    }

    /// Kicks off a reload of the asset stored at the given path. This will only reload the asset if it currently loaded.
    pub fn reload<'a>(&self, path: impl Into<AssetPath<'a>>) {
        self.reload_internal(path, false);
    }

    fn reload_internal<'a>(&self, path: impl Into<AssetPath<'a>>, log: bool) {
        let server = self.clone();
        let path = path.into().into_owned();
        IoTaskPool::get()
            .spawn(async move {
                let handle = {
                    let mut infos = server.write_infos();
                    let handle = infos.get_path_handle(&path);
                    if handle.is_some() {
                        // Count each reload as a started load.
                        infos.stats.started_load_tasks += 1;
                    }
                    handle
                };

                if let Some(handle) = handle {
                    let mut reloaded = false;
                    match server.load_internal(handle).await {
                        Ok(_) => reloaded = true,
                        Err(err) => error!("{}", err),
                    }
                    if log && reloaded {
                        info!("Reloaded {}", path);
                    }
                }
            })
            .detach();
    }

    /// Queues a new asset to be tracked by the [`AssetServer`] and returns a [`Handle`] to it. This can be used to track
    /// dependencies of assets created at runtime.
    ///
    /// After the asset has been fully loaded by the [`AssetServer`], it will be spawned as a component on the [`Handle`] entity.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn add<A: Asset>(&self, asset: A) -> Handle<A> {
        self.load_asset(None, LoadedAsset::new_with_dependencies(asset))
    }

    /// Queues a new asset to be tracked by the [`AssetServer`] and returns a [`Handle`] to it. This can be used to track
    /// dependencies of assets created at runtime.
    ///
    /// It can later be loaded/referenced with [`AssetReference::Uuid`].
    ///
    /// After the asset has been fully loaded by the [`AssetServer`], it will be spawned as a component on the [`Handle`] entity.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn add_with_uuid<A: Asset>(&self, uuid: Uuid, asset: A) -> Handle<A> {
        self.load_asset(
            Some(AssetReference::Uuid(uuid)),
            LoadedAsset::new_with_dependencies(asset),
        )
    }

    /// Queues a new asset to be tracked by the [`AssetServer`] and returns a [`Handle`] to it. This can be used to track
    /// dependencies of assets created at runtime.
    ///
    /// It can later be loaded/referenced with [`AssetReference::Default`].
    ///
    /// After the asset has been fully loaded by the [`AssetServer`], it will be spawned as a component on the [`Handle`] entity.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn add_default<A: Asset>(&self, asset: A) -> Handle<A> {
        self.load_asset(
            Some(AssetReference::Default),
            LoadedAsset::new_with_dependencies(asset),
        )
    }

    pub(crate) fn load_asset<A: Asset>(
        &self,
        reference: Option<AssetReference<'static>>,
        asset: impl Into<LoadedAsset<A>>,
    ) -> Handle<A> {
        let loaded_asset: LoadedAsset<A> = asset.into();
        let erased_loaded_asset: ErasedLoadedAsset = loaded_asset.into();
        self.load_asset_untyped(reference, erased_loaded_asset)
            .typed_unchecked()
    }

    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub(crate) fn load_asset_untyped(
        &self,
        reference: Option<AssetReference<'static>>,
        asset: impl Into<ErasedLoadedAsset>,
    ) -> UntypedHandle {
        let loaded_asset = asset.into();
        let (_, uuid, is_default) = match reference {
            Some(reference) => reference.split(),
            None => (None, None, false),
        };
        let handle = self.get_or_create_handle(AssetData {
            uuid,
            is_default,
            type_id_hint: Some(loaded_asset.asset_type_id()),
            ..default()
        });
        self.send_asset_event(InternalAssetEvent::Loaded {
            entity: handle.entity(),
            loaded_asset,
        });
        handle
    }

    /// Queues a new asset to be tracked by the [`AssetServer`] and returns a [`Handle`] to it. This can be used to track
    /// dependencies of assets created at runtime.
    ///
    /// After the asset has been fully loaded, it will show up in the relevant [`Assets`] storage.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn add_async<A: Asset, E: core::error::Error + Send + Sync + 'static>(
        &self,
        future: impl Future<Output = Result<A, E>> + Send + 'static,
    ) -> Handle<A> {
        let handle = self.get_or_create_handle(AssetData::new::<A>());
        let entity = handle.entity();
        let event_sender = self.data.asset_event_sender.clone();

        let task = IoTaskPool::get().spawn(async move {
            match future.await {
                Ok(asset) => {
                    let loaded_asset = LoadedAsset::new_with_dependencies(asset).into();
                    event_sender
                        .send(InternalAssetEvent::Loaded {
                            entity,
                            loaded_asset,
                        })
                        .unwrap();
                }
                Err(error) => {
                    let error = AddAsyncError {
                        error: Arc::new(error),
                    };
                    error!("{error}");
                    event_sender
                        .send(InternalAssetEvent::Failed {
                            entity,
                            error: AssetLoadError::AddAsyncError(error),
                        })
                        .unwrap();
                }
            }
        });

        #[cfg(not(any(target_arch = "wasm32", not(feature = "multi_threaded"))))]
        self.write_infos().pending_tasks.insert(entity, task);

        #[cfg(any(target_arch = "wasm32", not(feature = "multi_threaded")))]
        task.detach();

        handle.typed_unchecked()
    }

    /// Loads all assets from the specified folder recursively. The [`LoadedFolder`] asset (when it loads) will
    /// contain handles to all assets in the folder. You can wait for all assets to load by checking the [`LoadedFolder`]'s
    /// [`RecursiveDependencyLoadState`].
    ///
    /// Loading the same folder multiple times will return the same handle. If the `file_watcher`
    /// feature is enabled, [`LoadedFolder`] handles will reload when a file in the folder is
    /// removed, added or moved. This includes files in subdirectories and moving, adding,
    /// or removing complete subdirectories.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the assets"]
    pub fn load_folder<'a>(&self, path: impl Into<AssetPath<'a>>) -> Handle<LoadedFolder> {
        let path = path.into().into_owned();
        let (handle, should_load) = self.write_infos().get_or_create_handle(
            HandleLoadingMode::Request,
            AssetData {
                path: Some(path.clone()),
                ..AssetData::new::<LoadedFolder>()
            },
        );
        let handle = handle.typed_unchecked();
        if !should_load {
            return handle;
        }
        let entity = handle.entity();
        self.write_infos().stats.started_load_tasks += 1;
        self.load_folder_internal(entity, path);

        handle
    }

    pub(crate) fn load_folder_internal(&self, entity: Entity, path: AssetPath) {
        async fn load_folder<'a>(
            source: AssetSourceId<'static>,
            path: &'a Path,
            reader: &'a dyn ErasedAssetReader,
            server: &'a AssetServer,
            handles: &'a mut Vec<UntypedHandle>,
        ) -> Result<(), AssetLoadError> {
            let is_dir = reader.is_directory(path).await?;
            if is_dir {
                let mut path_stream = reader.read_directory(path.as_ref()).await?;
                while let Some(child_path) = path_stream.next().await {
                    if reader.is_directory(&child_path).await? {
                        Box::pin(load_folder(
                            source.clone(),
                            &child_path,
                            reader,
                            server,
                            handles,
                        ))
                        .await?;
                    } else {
                        let path = child_path.to_str().expect("Path should be a valid string.");
                        let asset_path = AssetPath::parse(path).with_source(source.clone());
                        match server.load_builder().load_untyped_async(asset_path).await {
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

        let path = path.into_owned();
        let server = self.clone();
        IoTaskPool::get()
            .spawn(async move {
                let Ok(source) = server.get_source(path.source()) else {
                    error!(
                        "Failed to load {path}. AssetSource {} does not exist",
                        path.source()
                    );
                    return;
                };

                let asset_reader = match server.data.mode {
                    AssetServerMode::Unprocessed => source.reader(),
                    AssetServerMode::Processed => match source.processed_reader() {
                        Ok(reader) => reader,
                        Err(_) => {
                            error!(
                                "Failed to load {path}. AssetSource {} does not have a processed AssetReader",
                                path.source()
                            );
                            return;
                        }
                    },
                };

                let mut handles = Vec::new();
                match load_folder(source.id(), path.path(), asset_reader, &server, &mut handles).await {
                    Ok(_) => server.send_asset_event(InternalAssetEvent::Loaded {
                        entity,
                        loaded_asset: LoadedAsset::new_with_dependencies(
                            LoadedFolder { handles },
                        )
                        .into(),
                    }),
                    Err(err) => {
                        error!("Failed to load folder. {err}");
                        server.send_asset_event(InternalAssetEvent::Failed { entity, error: err });
                    },
                }
            })
            .detach();
    }

    fn send_asset_event(&self, event: InternalAssetEvent) {
        self.data.asset_event_sender.send(event).unwrap();
    }

    /// Retrieves all loads states for the given asset id.
    pub fn get_load_states(
        &self,
        entity: impl Into<Entity>,
    ) -> Option<(LoadState, DependencyLoadState, RecursiveDependencyLoadState)> {
        self.read_infos().get(entity.into()).map(|i| {
            (
                i.load_state.clone(),
                i.dep_load_state.clone(),
                i.rec_dep_load_state.clone(),
            )
        })
    }

    /// Retrieves the main [`LoadState`] of a given asset `id`.
    ///
    /// Note that this is "just" the root asset load state. To get the load state of
    /// its dependencies or recursive dependencies, see [`AssetServer::get_dependency_load_state`]
    /// and [`AssetServer::get_recursive_dependency_load_state`] respectively.
    pub fn get_load_state(&self, entity: impl Into<Entity>) -> Option<LoadState> {
        self.read_infos()
            .get(entity.into())
            .map(|i| i.load_state.clone())
    }

    /// Retrieves the [`DependencyLoadState`] of a given asset `id`'s dependencies.
    ///
    /// Note that this is only the load state of direct dependencies of the root asset. To get
    /// the load state of the root asset itself or its recursive dependencies, see
    /// [`AssetServer::get_load_state`] and [`AssetServer::get_recursive_dependency_load_state`] respectively.
    pub fn get_dependency_load_state(
        &self,
        entity: impl Into<Entity>,
    ) -> Option<DependencyLoadState> {
        self.read_infos()
            .get(entity.into())
            .map(|i| i.dep_load_state.clone())
    }

    /// Retrieves the main [`RecursiveDependencyLoadState`] of a given asset `id`'s recursive dependencies.
    ///
    /// Note that this is only the load state of recursive dependencies of the root asset. To get
    /// the load state of the root asset itself or its direct dependencies only, see
    /// [`AssetServer::get_load_state`] and [`AssetServer::get_dependency_load_state`] respectively.
    pub fn get_recursive_dependency_load_state(
        &self,
        entity: impl Into<Entity>,
    ) -> Option<RecursiveDependencyLoadState> {
        self.read_infos()
            .get(entity.into())
            .map(|i| i.rec_dep_load_state.clone())
    }

    /// Retrieves the main [`LoadState`] of a given asset `id`.
    ///
    /// This is the same as [`AssetServer::get_load_state`] except the result is unwrapped. If
    /// the result is None, [`LoadState::NotLoaded`] is returned.
    pub fn load_state(&self, entity: impl Into<Entity>) -> LoadState {
        self.get_load_state(entity.into())
            .unwrap_or(LoadState::NotLoaded)
    }

    /// Retrieves the [`DependencyLoadState`] of a given asset `id`.
    ///
    /// This is the same as [`AssetServer::get_dependency_load_state`] except the result is unwrapped. If
    /// the result is None, [`DependencyLoadState::NotLoaded`] is returned.
    pub fn dependency_load_state(&self, entity: impl Into<Entity>) -> DependencyLoadState {
        self.get_dependency_load_state(entity)
            .unwrap_or(DependencyLoadState::NotLoaded)
    }

    /// Retrieves the  [`RecursiveDependencyLoadState`] of a given asset `id`.
    ///
    /// This is the same as [`AssetServer::get_recursive_dependency_load_state`] except the result is unwrapped. If
    /// the result is None, [`RecursiveDependencyLoadState::NotLoaded`] is returned.
    pub fn recursive_dependency_load_state(
        &self,
        entity: impl Into<Entity>,
    ) -> RecursiveDependencyLoadState {
        self.get_recursive_dependency_load_state(entity)
            .unwrap_or(RecursiveDependencyLoadState::NotLoaded)
    }

    /// Convenience method that returns true if the asset has been loaded.
    pub fn is_loaded(&self, entity: impl Into<Entity>) -> bool {
        matches!(self.load_state(entity), LoadState::Loaded)
    }

    /// Convenience method that returns true if the asset and all of its direct dependencies have been loaded.
    pub fn is_loaded_with_direct_dependencies(&self, entity: impl Into<Entity>) -> bool {
        matches!(
            self.get_load_states(entity),
            Some((LoadState::Loaded, DependencyLoadState::Loaded, _))
        )
    }

    /// Convenience method that returns true if the asset, all of its dependencies, and all of its recursive
    /// dependencies have been loaded.
    pub fn is_loaded_with_dependencies(&self, entity: impl Into<Entity>) -> bool {
        matches!(
            self.get_load_states(entity),
            Some((
                LoadState::Loaded,
                DependencyLoadState::Loaded,
                RecursiveDependencyLoadState::Loaded
            ))
        )
    }

    /// Returns true if all of `value`s dependencies (included recursive dependencies) are loaded.
    ///
    /// This allows querying for whether all the handles in a resource or component are loaded.
    pub fn are_dependencies_loaded(&self, value: &impl VisitAssetDependencies) -> bool {
        let infos = self.read_infos();
        let mut loaded = true;
        value.visit_dependencies(&mut |asset_id| {
            let Some(info) = infos.get(asset_id.entity()) else {
                // If the asset ID is no longer valid, we consider that as not loaded.
                loaded = false;
                return;
            };

            if !info.rec_dep_load_state.is_loaded() {
                loaded = false;
            }
        });
        loaded
    }

    /// Returns true if all of `value`s dependencies (excluding recursive dependencies) are loaded.
    ///
    /// This allows querying for whether all the handles in a resource or component are loaded.
    pub fn are_direct_dependencies_loaded(&self, value: &impl VisitAssetDependencies) -> bool {
        let infos = self.read_infos();
        let mut loaded = true;
        value.visit_dependencies(&mut |asset_id| {
            let Some(info) = infos.get(asset_id.entity()) else {
                // If the asset ID is no longer valid, we consider that as not loaded.
                loaded = false;
                return;
            };

            if !info.dep_load_state.is_loaded() {
                loaded = false;
            }
        });
        loaded
    }

    /// Returns an active handle for the given path, if the asset at the given path has already started loading,
    /// or is still "alive".
    pub fn get_handle<'a, A: Asset>(&self, path: impl Into<AssetPath<'a>>) -> Option<Handle<A>> {
        self.read_infos()
            .get_path_handle(&path.into())
            .map(UntypedHandle::typed_unchecked)
    }

    /// Get a `Handle` from an `AssetId`.
    ///
    /// This only returns `Some` if `id` is derived from a `Handle` that was
    /// loaded through an `AssetServer`, otherwise it returns `None`.
    ///
    /// Consider using [`Assets::get_strong_handle`] in the case the `Handle`
    /// comes from [`Assets::add`].
    pub fn get_id_handle<A: Asset>(&self, id: AssetId<A>) -> Option<Handle<A>> {
        self.get_entity_handle_untyped(id.entity())
            .map(UntypedHandle::typed_unchecked)
    }

    /// Get an `UntypedHandle` from an `UntypedAssetId`.
    /// See [`AssetServer::get_id_handle`] for details.
    pub fn get_entity_handle_untyped(&self, entity: Entity) -> Option<UntypedHandle> {
        self.read_infos().get_entity_handle(entity)
    }

    /// Returns `true` if the given `id` corresponds to an asset that is managed by this [`AssetServer`].
    /// Otherwise, returns `false`.
    pub fn is_managed(&self, entity: impl Into<Entity>) -> bool {
        self.read_infos().contains_key(entity.into())
    }

    /// Returns an active untyped asset id for the given path, if the asset at the given path has already started loading,
    /// or is still "alive".
    /// Returns the first ID in the event of multiple assets being registered against a single path.
    pub fn get_path_entity<'a>(&self, path: impl Into<AssetPath<'a>>) -> Option<Entity> {
        let infos = self.read_infos();
        let path = path.into();
        infos.get_path_entity(&path)
    }

    /// Returns an active untyped handle for the given path, if the asset at the given path has already started loading,
    /// or is still "alive".
    /// Returns the first handle in the event of multiple assets being registered against a single path.
    ///
    /// # See also
    /// [`get_handles_untyped`][Self::get_handles_untyped] for all handles.
    pub fn get_handle_untyped<'a>(&self, path: impl Into<AssetPath<'a>>) -> Option<UntypedHandle> {
        let path = path.into();
        self.read_infos().get_path_handle(&path)
    }

    /// Returns the path for the given `id`, if it has one.
    pub fn get_path(&self, entity: impl Into<Entity>) -> Option<AssetPath<'_>> {
        let infos = self.read_infos();
        let info = infos.get(entity.into())?;
        Some(info.path.as_ref()?.clone())
    }

    /// Returns the [`AssetServerMode`] this server is currently in.
    pub fn mode(&self) -> AssetServerMode {
        self.data.mode
    }

    /// Pre-register a loader that will later be added.
    ///
    /// Assets loaded with matching extensions will be blocked until the
    /// real loader is added.
    pub fn preregister_loader<L: AssetLoader>(&self, extensions: &[&str]) {
        self.write_loaders().reserve::<L>(extensions);
    }

    pub fn init_asset<A: Asset>(&self) {
        self.write_infos()
            .typed_asset_event_senders
            .insert(TypeId::of::<A>(), AssetEventSenders::new::<A>());
    }

    /// Retrieve a handle for the given path. This will create a handle (and [`AssetInfo`]) if it does not exist
    pub(crate) fn get_or_create_handle(&self, data: AssetData) -> UntypedHandle {
        self.write_infos()
            .get_or_create_handle(HandleLoadingMode::NotLoading, data)
            .0
    }

    pub(crate) async fn get_meta_loader_and_reader<'a>(
        &'a self,
        asset_path: &'a AssetPath<'_>,
        asset_type_id: Option<TypeId>,
    ) -> Result<
        (
            Box<dyn AssetMetaDyn>,
            Arc<dyn ErasedAssetLoader>,
            Box<dyn Reader + 'a>,
        ),
        AssetLoadError,
    > {
        let source = self.get_source(asset_path.source())?;
        let asset_reader = match self.data.mode {
            AssetServerMode::Unprocessed => source.reader(),
            AssetServerMode::Processed => source.processed_reader()?,
        };
        let read_meta = match &self.data.meta_check {
            AssetMetaCheck::Always => true,
            AssetMetaCheck::Paths(paths) => paths.contains(asset_path),
            AssetMetaCheck::Never => false,
        };

        // Scope the meta reader up here. This allows the reader to be "transactional": for sources
        // that want to lock the asset before reading it (e.g., with a RwLock), this allows the meta
        // reader to take the RwLock, and since it overlaps with the asset reader, the asset reader
        // can "take over" the RwLock before the meta reader gets dropped.
        let mut meta_reader;

        let (meta, loader) = if read_meta {
            match asset_reader.read_meta(asset_path.path()).await {
                Ok(new_meta_reader) => {
                    meta_reader = new_meta_reader;
                    let mut meta_bytes = vec![];
                    meta_reader
                        .read_to_end(&mut meta_bytes)
                        .await
                        .map_err(|err| AssetLoadError::AssetReaderError(err.into()))?;
                    // TODO: this isn't fully minimal yet. we only need the loader
                    let minimal: AssetMetaMinimal =
                        ron::de::from_bytes(&meta_bytes).map_err(|e| {
                            AssetLoadError::DeserializeMeta {
                                path: asset_path.clone_owned(),
                                error: DeserializeMetaError::DeserializeMinimal(e).into(),
                            }
                        })?;
                    let loader_name = match minimal.asset {
                        AssetActionMinimal::Load { loader } => loader,
                        AssetActionMinimal::Process { .. } => {
                            return Err(AssetLoadError::CannotLoadProcessedAsset {
                                path: asset_path.clone_owned(),
                            })
                        }
                        AssetActionMinimal::Ignore => {
                            return Err(AssetLoadError::CannotLoadIgnoredAsset {
                                path: asset_path.clone_owned(),
                            })
                        }
                    };
                    let loader = self.get_asset_loader_with_type_name(&loader_name).await?;
                    let meta = loader.deserialize_meta(&meta_bytes).map_err(|e| {
                        AssetLoadError::DeserializeMeta {
                            path: asset_path.clone_owned(),
                            error: e.into(),
                        }
                    })?;

                    (meta, loader)
                }
                Err(AssetReaderError::NotFound(_)) => {
                    // TODO: Handle error transformation
                    let loader = { self.read_loaders().find(asset_type_id, asset_path) };

                    let error = || AssetLoadError::MissingAssetLoader {
                        asset_type_id,
                        asset_path: asset_path.to_string(),
                    };

                    let loader = loader.ok_or_else(error)?.get().await.map_err(|_| error())?;

                    let meta = loader.default_meta();
                    (meta, loader)
                }
                Err(err) => return Err(err.into()),
            }
        } else {
            let loader = { self.read_loaders().find(asset_type_id, asset_path) };

            let error = || AssetLoadError::MissingAssetLoader {
                asset_type_id,
                asset_path: asset_path.to_string(),
            };

            let loader = loader.ok_or_else(error)?.get().await.map_err(|_| error())?;

            let meta = loader.default_meta();
            (meta, loader)
        };
        let reader = asset_reader.read(asset_path.path()).await?;
        Ok((meta, loader, reader))
    }

    pub(crate) async fn load_with_settings_loader_and_reader(
        &self,
        asset_path: &AssetPath<'_>,
        settings: &dyn Settings,
        loader: &dyn ErasedAssetLoader,
        reader: &mut dyn Reader,
        load_dependencies: bool,
        populate_hashes: bool,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        // TODO: experiment with this
        let asset_path = asset_path.clone_owned();
        let load_context =
            LoadContext::new(self, asset_path.clone(), load_dependencies, populate_hashes);
        let load = AssertUnwindSafe(loader.load(reader, settings, load_context)).catch_unwind();
        #[cfg(feature = "trace")]
        let load = {
            use tracing::Instrument;

            let span = tracing::info_span!(
                "asset loading",
                loader = loader.type_path(),
                asset = asset_path.to_string()
            );
            load.instrument(span)
        };
        load.await
            .map_err(|_| AssetLoadError::AssetLoaderPanic {
                path: asset_path.clone_owned(),
                loader_name: loader.type_path(),
            })?
            .map_err(|e| {
                AssetLoadError::AssetLoaderError(AssetLoaderError {
                    path: asset_path.clone_owned(),
                    loader_name: loader.type_path(),
                    error: e.into(),
                })
            })
    }

    /// Returns a future that will suspend until the specified asset and its dependencies finish
    /// loading.
    ///
    /// # Errors
    ///
    /// This will return an error if the asset or any of its dependencies fail to load,
    /// or if the asset has not been queued up to be loaded.
    pub async fn wait_for_asset<A: Asset>(
        &self,
        // NOTE: We take a reference to a handle so we know it will outlive the future,
        // which ensures the handle won't be dropped while waiting for the asset.
        handle: &Handle<A>,
    ) -> Result<(), WaitForAssetError> {
        self.wait_for_asset_id(handle.entity()).await
    }

    /// Returns a future that will suspend until the specified asset and its dependencies finish
    /// loading.
    ///
    /// # Errors
    ///
    /// This will return an error if the asset or any of its dependencies fail to load,
    /// or if the asset has not been queued up to be loaded.
    pub async fn wait_for_asset_untyped(
        &self,
        // NOTE: We take a reference to a handle so we know it will outlive the future,
        // which ensures the handle won't be dropped while waiting for the asset.
        handle: &UntypedHandle,
    ) -> Result<(), WaitForAssetError> {
        self.wait_for_asset_id(handle.entity()).await
    }

    /// Returns a future that will suspend until the specified asset and its dependencies finish
    /// loading.
    ///
    /// Note that since an asset ID does not count as a reference to the asset,
    /// the future returned from this method will *not* keep the asset alive.
    /// This may lead to the asset unexpectedly being dropped while you are waiting for it to
    /// finish loading.
    ///
    /// When calling this method, make sure a strong handle is stored elsewhere to prevent the
    /// asset from being dropped.
    /// If you have access to an asset's strong [`Handle`], you should prefer to call
    /// [`AssetServer::wait_for_asset`]
    /// or [`wait_for_asset_untyped`](Self::wait_for_asset_untyped) to ensure the asset finishes
    /// loading.
    ///
    /// # Errors
    ///
    /// This will return an error if the asset or any of its dependencies fail to load,
    /// or if the asset has not been queued up to be loaded.
    pub async fn wait_for_asset_id(
        &self,
        entity: impl Into<Entity>,
    ) -> Result<(), WaitForAssetError> {
        let entity = entity.into();
        core::future::poll_fn(move |cx| self.wait_for_asset_id_poll_fn(cx, entity)).await
    }

    /// Used by [`wait_for_asset_id`](AssetServer::wait_for_asset_id) in [`poll_fn`](core::future::poll_fn).
    fn wait_for_asset_id_poll_fn(
        &self,
        cx: &mut core::task::Context<'_>,
        entity: Entity,
    ) -> Poll<Result<(), WaitForAssetError>> {
        let infos = self.read_infos();

        let Some(info) = infos.get(entity) else {
            return Poll::Ready(Err(WaitForAssetError::NotLoaded));
        };

        match (&info.load_state, &info.rec_dep_load_state) {
            (LoadState::Loaded, RecursiveDependencyLoadState::Loaded) => Poll::Ready(Ok(())),
            // Return an error immediately if the asset is not in the process of loading
            (LoadState::NotLoaded, _) => Poll::Ready(Err(WaitForAssetError::NotLoaded)),
            // If the asset is loading, leave our waker behind
            (LoadState::Loading, _)
            | (_, RecursiveDependencyLoadState::Loading)
            | (LoadState::Loaded, RecursiveDependencyLoadState::NotLoaded) => {
                // Check if our waker is already there
                let has_waker = info
                    .waiting_tasks
                    .iter()
                    .any(|waker| waker.will_wake(cx.waker()));

                if has_waker {
                    return Poll::Pending;
                }

                let mut infos = {
                    // Must drop read-only guard to acquire write guard
                    drop(infos);
                    self.write_infos()
                };

                let Some(info) = infos.get_mut(entity) else {
                    return Poll::Ready(Err(WaitForAssetError::NotLoaded));
                };

                // If the load state changed while reacquiring the lock, immediately
                // reawaken the task
                let is_loading = matches!(
                    (&info.load_state, &info.rec_dep_load_state),
                    (LoadState::Loading, _)
                        | (_, RecursiveDependencyLoadState::Loading)
                        | (LoadState::Loaded, RecursiveDependencyLoadState::NotLoaded)
                );

                if !is_loading {
                    cx.waker().wake_by_ref();
                } else {
                    // Leave our waker behind
                    info.waiting_tasks.push(cx.waker().clone());
                }

                Poll::Pending
            }
            (LoadState::Failed(error), _) => {
                Poll::Ready(Err(WaitForAssetError::Failed(error.clone())))
            }
            (_, RecursiveDependencyLoadState::Failed(error)) => {
                Poll::Ready(Err(WaitForAssetError::DependencyFailed(error.clone())))
            }
        }
    }

    /// Writes the default loader meta file for the provided `path`.
    ///
    /// This function only generates meta files that simply load the path directly. To generate a
    /// meta file that will use the default asset processor for the path, see
    /// [`AssetProcessor::write_default_meta_file_for_path`].
    ///
    /// Note if there is already a meta file for `path`, this function returns
    /// `Err(WriteDefaultMetaError::MetaAlreadyExists)`.
    ///
    /// [`AssetProcessor::write_default_meta_file_for_path`]:  crate::AssetProcessor::write_default_meta_file_for_path
    pub async fn write_default_loader_meta_file_for_path(
        &self,
        path: impl Into<AssetPath<'_>>,
    ) -> Result<(), WriteDefaultMetaError> {
        let path = path.into();
        let loader = self.get_path_asset_loader(&path).await?;

        let meta = loader.default_meta();
        let serialized_meta = meta.serialize();

        let source = self.get_source(path.source())?;

        let reader = source.reader();
        match reader.read_meta_bytes(path.path()).await {
            Ok(_) => return Err(WriteDefaultMetaError::MetaAlreadyExists),
            Err(AssetReaderError::NotFound(_)) => {
                // The meta file couldn't be found so just fall through.
            }
            Err(AssetReaderError::Io(err)) => {
                return Err(WriteDefaultMetaError::IoErrorFromExistingMetaCheck(err))
            }
            Err(AssetReaderError::HttpError(err)) => {
                return Err(WriteDefaultMetaError::HttpErrorFromExistingMetaCheck(err))
            }
        }

        let writer = source.writer()?;
        writer
            .write_meta_bytes(path.path(), &serialized_meta)
            .await?;

        Ok(())
    }
}

/// A builder for initiating a more complex load than the one provided by [`AssetServer::load`].
///
/// For example, a load may look like:
///
/// ```ignore
/// asset_server
///     .load_builder()
///     .with_settings(settings)
///     .override_unapproved()
///     .load("my.path")
/// ```
pub struct LoadBuilder<'a> {
    /// The asset server on which the load is invoked.
    asset_server: &'a AssetServer,
    /// A function to modify the meta for an asset loader. In practice, this just mutates the loader
    /// settings of a load.
    meta_transform: Option<MetaTransform>,
    /// Whether unapproved paths are allowed to be loaded.
    override_unapproved: bool,
    /// A "guard" that is held until the load has fully completed.
    guard: Option<Box<dyn Send + Sync + 'static>>,
}

impl<'a> LoadBuilder<'a> {
    /// Begins building a new load on the given `asset_server`.
    #[must_use = "the load doesn't start until LoadBuilder has been consumed"]
    fn new(asset_server: &'a AssetServer) -> Self {
        Self {
            asset_server,
            meta_transform: None,
            override_unapproved: false,
            guard: None,
        }
    }

    /// Use the given `settings` function to override the asset's [`AssetLoader`] settings.
    ///
    /// The type `S` must match the configured [`AssetLoader::Settings`] or `settings` changes will
    /// be ignored and an error will be printed to the log.
    ///
    /// Repeatedly calling this method will "chain" the operations (matching the order of these
    /// calls).
    #[must_use = "the load doesn't start until LoadBuilder has been consumed"]
    pub fn with_settings<S: Settings>(
        mut self,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
    ) -> Self {
        let new_transform = loader_settings_meta_transform(settings);
        if let Some(prev_transform) = self.meta_transform.take() {
            self.meta_transform = Some(Box::new(move |meta| {
                prev_transform(meta);
                new_transform(meta);
            }));
        } else {
            self.meta_transform = Some(new_transform);
        }
        self
    }

    /// Loads from unapproved paths are allowed, even if
    /// [`AssetPlugin::unapproved_path_mode`](crate::AssetPlugin::unapproved_path_mode) is
    /// [`Deny`](crate::UnapprovedPathMode::Deny).
    #[must_use = "the load doesn't start until LoadBuilder has been consumed"]
    pub fn override_unapproved(mut self) -> Self {
        self.override_unapproved = true;
        self
    }

    /// Sets the guard item that is held during the load.
    ///
    /// The guard item is dropped when either the asset is loaded or loading has failed. This allows
    /// the [`Drop`] implementation of the guard item to notify the caller in some way. See the
    /// `multi_asset_sync` example for usage.
    ///
    /// Only the last guard is kept. The previous guards are dropped before the load begins.
    #[must_use = "the load doesn't start until LoadBuilder has been consumed"]
    pub fn with_guard(mut self, guard: impl Send + Sync + 'static) -> Self {
        if self.guard.is_some() {
            warn!("Adding a second guard to a LoadBuilder drops the first guard! This is likely a mistake.");
        }
        // If guard is already a box, then we might end up double-boxing, which is sad. But this is
        // almost certainly not worth caring about.
        self.guard = Some(Box::new(guard));
        self
    }

    /// Begins loading an [`Asset`] of type `A` stored at `path`. This will not block on the asset load. Instead,
    /// it returns a "strong" [`Handle`]. When the [`Asset`] is loaded (and enters [`LoadState::Loaded`]), it will be added to the
    /// associated [`Assets`] resource.
    ///
    /// Note that if the asset at this path is already loaded, this function will return the existing handle,
    /// and will not waste work spawning a new load task.
    ///
    /// This matches the behavior of [`AssetServer::load`], but supporting all other features of the
    /// builder. See its docs for more details.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load<'b, A: Asset>(mut self, reference: impl Into<AssetReference<'b>>) -> Handle<A> {
        let meta_transform = self.meta_transform.take();
        let (path, uuid, is_default) = reference.into().into_owned().split();
        self.load_internal(AssetData {
            path,
            uuid,
            is_default,
            meta_transform,
            ..AssetData::new::<A>()
        })
        .typed_unchecked()
    }

    /// Same as [`load`](Self::load), but without a type hint, meaning the default loader will be used.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_untyped<'b>(mut self, asset_path: impl Into<AssetPath<'b>>) -> UntypedHandle {
        let meta_transform = self.meta_transform.take();
        self.load_internal(AssetData {
            path: Some(asset_path.into().into_owned()),
            meta_transform,
            ..Default::default()
        })
    }

    /// Same as [`load`](Self::load), but the type of the asset to load is specified by the runtime
    /// `type_id`.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub fn load_erased<'b>(
        mut self,
        type_id: TypeId,
        asset_path: impl Into<AssetPath<'b>>,
    ) -> UntypedHandle {
        let meta_transform = self.meta_transform.take();
        self.load_internal(AssetData {
            path: Some(asset_path.into().into_owned()),
            type_id_hint: Some(type_id),
            meta_transform,
            ..Default::default()
        })
    }

    /// Asynchronously load an asset that you do not know the type of statically. If you _do_ know the type of the asset,
    /// you should use [`AssetServer::load`]. If you don't know the type of the asset, but you can't use an async method,
    /// consider using [`AssetServer::load_untyped`].
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    pub async fn load_untyped_async<'b>(
        self,
        asset_path: impl Into<AssetPath<'b>>,
    ) -> Result<UntypedHandle, AssetLoadError> {
        let path: AssetPath = asset_path.into();
        if path.path() == Path::new("") {
            return Err(AssetLoadError::EmptyPath(path.into_owned()));
        }

        let (handle, should_load) = {
            let mut infos = self.asset_server.write_infos();
            let (handle, should_load) = infos.get_or_create_handle(
                HandleLoadingMode::Request,
                AssetData {
                    path: Some(path.clone().into_owned()),
                    meta_transform: self.meta_transform,
                    ..Default::default()
                },
            );
            if should_load {
                infos.stats.started_load_tasks += 1;
            }
            (handle, should_load)
        };
        if should_load {
            self.asset_server.load_internal(handle.clone()).await?;
        }
        Ok(handle)
    }

    /// Begins a (deferred) load for an asset with the given `type_id` and `type_name`.
    #[must_use = "not using the returned strong handle may result in the unexpected release of the asset"]
    fn load_internal(self, data: AssetData) -> UntypedHandle {
        self.asset_server
            .load_guarded(data, self.guard, self.override_unapproved)
    }
}

/// A system that manages internal [`AssetServer`] events, such as finalizing asset loads.
pub fn handle_internal_asset_events(world: &mut World) {
    world.resource_scope(|world, server: Mut<AssetServer>| {
        let mut infos = server.write_infos();
        for event in server.data.asset_event_receiver.try_iter() {
            match event {
                InternalAssetEvent::Loaded {
                    entity,
                    loaded_asset,
                } => {
                    infos.process_asset_load(
                        entity,
                        loaded_asset,
                        world,
                        &server.data.asset_event_sender,
                    );
                }
                InternalAssetEvent::LoadedWithDependencies { entity } => {
                    world.trigger(LoadedWithDependencies { entity });
                    if let Some(loaded_type_id) = infos.get(entity).and_then(|i| i.loaded_type_id) {
                        if let Some(senders) = infos.typed_asset_event_senders.get(&loaded_type_id) {
                            (senders.loaded_with_dependencies)(world, entity);
                        } else {
                            warn!("Failed to trigger LoadedWithDependencies event for asset type {loaded_type_id:?}. This asset type wasn't registered with the AssetServer.");
                        }
                    }
                    if let Some(info) = infos.get_mut(entity) {
                        for waker in info.waiting_tasks.drain(..) {
                            waker.wake();
                        }
                    }
                }
                InternalAssetEvent::Failed { entity, error } => {
                    infos.process_asset_fail(entity, error.clone());
                    world.trigger(LoadFailed { entity, error: error.clone() });
                    if let Some(asset_info) = infos.get(entity) &&
                        let Some(type_id) = asset_info.loaded_type_id &&
                        let Some(path) = &asset_info.path {
                        if let Some(senders) = infos.typed_asset_event_senders.get(&type_id) {
                            (senders.failed)(world, entity, error, path.clone());
                        } else {
                            warn!("Failed to trigger AssetLoadFailedEvent for asset type {type_id:?}. This asset type wasn't registered with the AssetServer.");
                        }
                    }
                }
            }
        }

        // The following code all deals with hot-reloading, which we can skip if the server isn't
        // watching for changes.
        if !infos.watching_for_changes {
            return;
        }

        fn queue_ancestors(
            asset_path: &AssetPath,
            infos: &AssetInfos,
            paths_to_reload: &mut HashSet<AssetPath<'static>>,
        ) {
            if let Some(dependents) = infos.loader_dependents.get(asset_path) {
                for dependent in dependents {
                    paths_to_reload.insert(dependent.to_owned());
                    queue_ancestors(dependent, infos, paths_to_reload);
                }
            }
        }

        let mut folders_to_reload = Vec::default();
        let mut reload_parent_folders =
            |path: &PathBuf, source: &AssetSourceId<'static>, infos: &mut AssetInfos| {
                let mut new_loads = 0;
                for parent in path.ancestors().skip(1) {
                    let parent_asset_path =
                        AssetPath::from(parent.to_path_buf()).with_source(source.clone());
                    if let Some(folder_handle) = infos.get_path_handle(&parent_asset_path) {
                        info!(
                            "Reloading folder {parent_asset_path} because the content has changed"
                        );
                        new_loads += 1;
                        folders_to_reload.push((folder_handle, parent_asset_path.clone()));
                    }
                }
                infos.stats.started_load_tasks += new_loads;
            };

        let mut paths_to_reload = <HashSet<_>>::default();
        let mut reload_path =
            |path: PathBuf, source: &AssetSourceId<'static>, infos: &AssetInfos| {
                let path = AssetPath::from(path).with_source(source);
                queue_ancestors(&path, infos, &mut paths_to_reload);
                paths_to_reload.insert(path);
            };

        let mut handle_event = |source: AssetSourceId<'static>, event: AssetSourceEvent| {
            match event {
                AssetSourceEvent::AddedAsset(path) => {
                    reload_parent_folders(&path, &source, &mut infos);
                    reload_path(path, &source, &infos);
                }
                // TODO: if the asset was processed and the processed file was changed, the first modified event
                // should be skipped?
                AssetSourceEvent::ModifiedAsset(path) | AssetSourceEvent::ModifiedMeta(path) => {
                    reload_path(path, &source, &infos);
                }
                AssetSourceEvent::RenamedFolder { old, new } => {
                    reload_parent_folders(&old, &source, &mut infos);
                    reload_parent_folders(&new, &source, &mut infos);
                }
                AssetSourceEvent::RemovedAsset(path)
                | AssetSourceEvent::RemovedFolder(path)
                | AssetSourceEvent::AddedFolder(path) => {
                    reload_parent_folders(&path, &source, &mut infos);
                }
                _ => {}
            }
        };

        for source in server.data.sources.iter() {
            match server.data.mode {
                AssetServerMode::Unprocessed => {
                    if let Some(receiver) = source.event_receiver() {
                        while let Ok(event) = receiver.try_recv() {
                            handle_event(source.id(), event);
                        }
                    }
                }
                AssetServerMode::Processed => {
                    if let Some(receiver) = source.processed_event_receiver() {
                        while let Ok(event) = receiver.try_recv() {
                            handle_event(source.id(), event);
                        }
                    }
                }
            }
        }

        // Drop the lock on `AssetInfos` before spawning a task that may block on it in
        // single-threaded.
        #[cfg(any(target_arch = "wasm32", not(feature = "multi_threaded")))]
        drop(infos);

        for (handle, path) in folders_to_reload {
            server.load_folder_internal(handle.entity(), path);
        }
        for path in paths_to_reload {
            server.reload_internal(path, true);
        }

        #[cfg(not(any(target_arch = "wasm32", not(feature = "multi_threaded"))))]
        infos
            .pending_tasks
            .retain(|_, load_task| !load_task.is_finished());
    });
}

/// A system publishing asset server statistics to [`bevy_diagnostic`].
pub fn publish_asset_server_diagnostics(
    asset_server: Res<AssetServer>,
    mut diagnostics: Diagnostics,
) {
    let infos = asset_server.read_infos();
    diagnostics.add_measurement(&AssetServer::STARTED_LOAD_COUNT, || {
        infos.stats.started_load_tasks as _
    });
}

/// Internal events for asset load results
pub(crate) enum InternalAssetEvent {
    Loaded {
        entity: Entity,
        loaded_asset: ErasedLoadedAsset,
    },
    LoadedWithDependencies {
        entity: Entity,
    },
    Failed {
        entity: Entity,
        error: AssetLoadError,
    },
}

/// The load state of an asset.
#[derive(Component, Clone, Debug)]
pub enum LoadState {
    /// The asset has not started loading yet
    NotLoaded,

    /// The asset is in the process of loading.
    Loading,

    /// The asset has been loaded and has been added to the [`World`]
    Loaded,

    /// The asset failed to load. The underlying [`AssetLoadError`] is
    /// referenced by [`Arc`] clones in all related [`DependencyLoadState`]s
    /// and [`RecursiveDependencyLoadState`]s in the asset's dependency tree.
    Failed(Arc<AssetLoadError>),
}

impl LoadState {
    /// Returns `true` if this instance is [`LoadState::Loading`]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    /// Returns `true` if this instance is [`LoadState::Loaded`]
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded)
    }

    /// Returns `true` if this instance is [`LoadState::Failed`]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// The load state of an asset's dependencies.
#[derive(Component, Clone, Debug)]
pub enum DependencyLoadState {
    /// The asset has not started loading yet
    NotLoaded,

    /// Dependencies are still loading
    Loading,

    /// Dependencies have all loaded
    Loaded,

    /// One or more dependencies have failed to load. The underlying [`AssetLoadError`]
    /// is referenced by [`Arc`] clones in all related [`LoadState`] and
    /// [`RecursiveDependencyLoadState`]s in the asset's dependency tree.
    Failed(Arc<AssetLoadError>),
}

impl DependencyLoadState {
    /// Returns `true` if this instance is [`DependencyLoadState::Loading`]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    /// Returns `true` if this instance is [`DependencyLoadState::Loaded`]
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded)
    }

    /// Returns `true` if this instance is [`DependencyLoadState::Failed`]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// The recursive load state of an asset's dependencies.
#[derive(Component, Clone, Debug)]
pub enum RecursiveDependencyLoadState {
    /// The asset has not started loading yet
    NotLoaded,

    /// Dependencies in this asset's dependency tree are still loading
    Loading,

    /// Dependencies in this asset's dependency tree have all loaded
    Loaded,

    /// One or more dependencies have failed to load in this asset's dependency
    /// tree. The underlying [`AssetLoadError`] is referenced by [`Arc`] clones
    /// in all related [`LoadState`]s and [`DependencyLoadState`]s in the asset's
    /// dependency tree.
    Failed(Arc<AssetLoadError>),
}

impl RecursiveDependencyLoadState {
    /// Returns `true` if this instance is [`RecursiveDependencyLoadState::Loading`]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    /// Returns `true` if this instance is [`RecursiveDependencyLoadState::Loaded`]
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded)
    }

    /// Returns `true` if this instance is [`RecursiveDependencyLoadState::Failed`]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// An error that occurs when the requested handle type doesn't match the actual loaded asset type.
#[derive(Error, Debug, Clone)]
#[error("Requested handle of type {requested:?} for asset '{path}' does not match actual asset type '{actual_asset_name}', which used loader '{loader_name}'")]
pub struct RequestedHandleTypeMismatchError {
    /// The path of the asset.
    pub path: AssetPath<'static>,
    /// The requested type id of handle.
    pub requested: TypeId,
    /// The actual loaded asset type name.
    pub actual_asset_name: &'static str,
    /// The loader name used to load the asset.
    pub loader_name: &'static str,
}

/// An error that occurs during an [`Asset`] load.
#[derive(Error, Debug, Clone)]
#[expect(
    missing_docs,
    reason = "Adding docs to the variants would not add information beyond the error message and the names"
)]
pub enum AssetLoadError {
    #[error("Attempted to load an asset with an empty path \"{0}\"")]
    EmptyPath(AssetPath<'static>),
    #[error(transparent)]
    RequestedHandleTypeMismatch(#[from] Box<RequestedHandleTypeMismatchError>),
    #[error("Could not find an asset loader matching: Asset Type: {asset_type_id:?}; Path: {asset_path:?};")]
    MissingAssetLoader {
        asset_type_id: Option<TypeId>,
        asset_path: String,
    },
    #[error(transparent)]
    MissingAssetLoaderForExtension(#[from] MissingAssetLoaderForExtensionError),
    #[error(transparent)]
    MissingAssetLoaderForTypeName(#[from] MissingAssetLoaderForTypeNameError),
    #[error(transparent)]
    MissingAssetLoaderForTypeIdError(#[from] MissingAssetLoaderForTypeIdError),
    #[error(transparent)]
    AssetReaderError(#[from] AssetReaderError),
    #[error(transparent)]
    MissingAssetSourceError(#[from] MissingAssetSourceError),
    #[error(transparent)]
    MissingProcessedAssetReaderError(#[from] MissingProcessedAssetReaderError),
    #[error("Encountered an error while reading asset metadata bytes")]
    AssetMetaReadError,
    #[error("Failed to deserialize meta for asset {path}: {error}")]
    DeserializeMeta {
        path: AssetPath<'static>,
        error: Box<DeserializeMetaError>,
    },
    #[error("Asset '{path}' is configured to be processed. It cannot be loaded directly.")]
    #[from(ignore)]
    CannotLoadProcessedAsset { path: AssetPath<'static> },
    #[error("Asset '{path}' is configured to be ignored. It cannot be loaded.")]
    #[from(ignore)]
    CannotLoadIgnoredAsset { path: AssetPath<'static> },
    #[error("Failed to load asset '{path}', asset loader '{loader_name}' panicked")]
    AssetLoaderPanic {
        path: AssetPath<'static>,
        loader_name: &'static str,
    },
    #[error(transparent)]
    AssetLoaderError(#[from] AssetLoaderError),
    #[error(transparent)]
    AddAsyncError(#[from] AddAsyncError),
    #[error("The file at '{}' does not contain the labeled asset '{}'; it contains the following {} assets: {}",
            base_path,
            label,
            all_labels.len(),
            all_labels.iter().map(|l| format!("'{l}'")).collect::<Vec<_>>().join(", "))]
    MissingLabel {
        base_path: AssetPath<'static>,
        label: String,
        all_labels: Vec<String>,
    },
}

/// An error that can occur during asset loading.
#[derive(Error, Debug, Clone)]
#[error("Failed to load asset '{path}' with asset loader '{loader_name}': {error}")]
pub struct AssetLoaderError {
    path: AssetPath<'static>,
    loader_name: &'static str,
    error: Arc<BevyError>,
}

impl AssetLoaderError {
    /// The path of the asset that failed to load.
    pub fn path(&self) -> &AssetPath<'static> {
        &self.path
    }

    /// The error the loader reported when attempting to load the asset.
    ///
    /// If you know the type of the error the asset loader returned, you can use
    /// [`BevyError::downcast_ref()`] to get it.
    pub fn error(&self) -> &BevyError {
        &self.error
    }
}

/// An error that occurs while resolving an asset added by `add_async`.
#[derive(Error, Debug, Clone)]
#[error("An error occurred while resolving an asset added by `add_async`: {error}")]
pub struct AddAsyncError {
    error: Arc<dyn core::error::Error + Send + Sync + 'static>,
}

/// An error that occurs when an [`AssetLoader`] is not registered for a given extension.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("no `AssetLoader` found{}", format_missing_asset_ext(extensions))]
pub struct MissingAssetLoaderForExtensionError {
    extensions: Vec<String>,
}

/// An error that occurs when an [`AssetLoader`] is not registered for a given [`core::any::type_name`].
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("no `AssetLoader` found with the name '{type_name}'")]
pub struct MissingAssetLoaderForTypeNameError {
    /// The type name that was not found.
    pub type_name: String,
}

/// An error that occurs when an [`AssetLoader`] is not registered for a given [`Asset`] [`TypeId`].
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("no `AssetLoader` found with the ID '{type_id:?}'")]
pub struct MissingAssetLoaderForTypeIdError {
    /// The type ID that was not found.
    pub type_id: TypeId,
}

fn format_missing_asset_ext(exts: &[String]) -> String {
    if !exts.is_empty() {
        format!(
            " for the following extension{}: {}",
            if exts.len() > 1 { "s" } else { "" },
            exts.join(", ")
        )
    } else {
        " for file with no extension".to_string()
    }
}

impl core::fmt::Debug for AssetServer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AssetServer")
            .field("info", &self.data.infos.read())
            .finish()
    }
}

/// An error when attempting to wait asynchronously for an [`Asset`] to load.
#[derive(Error, Debug, Clone)]
pub enum WaitForAssetError {
    /// The asset is not being loaded; waiting for it is meaningless.
    #[error("tried to wait for an asset that is not being loaded")]
    NotLoaded,
    /// The asset failed to load.
    #[error(transparent)]
    Failed(Arc<AssetLoadError>),
    /// A dependency of the asset failed to load.
    #[error(transparent)]
    DependencyFailed(Arc<AssetLoadError>),
}

#[derive(Error, Debug)]
pub enum WriteDefaultMetaError {
    #[error(transparent)]
    MissingAssetLoader(#[from] MissingAssetLoaderForExtensionError),
    #[error(transparent)]
    MissingAssetSource(#[from] MissingAssetSourceError),
    #[error(transparent)]
    MissingAssetWriter(#[from] MissingAssetWriterError),
    #[error("failed to write default asset meta file: {0}")]
    FailedToWriteMeta(#[from] AssetWriterError),
    #[error("asset meta file already exists, so avoiding overwrite")]
    MetaAlreadyExists,
    #[error("encountered an I/O error while reading the existing meta file: {0}")]
    IoErrorFromExistingMetaCheck(Arc<std::io::Error>),
    #[error("encountered HTTP status {0} when reading the existing meta file")]
    HttpErrorFromExistingMetaCheck(u16),
}
