use crate::{
    meta::AssetHash, Asset, AssetData, AssetEvent, AssetLoadError, AssetLoadFailedEvent, AssetPath,
    DependencyLoadState, ErasedLoadedAsset, InternalAssetEvent, LoadState,
    RecursiveDependencyLoadState, UntypedHandle,
};
use alloc::{borrow::ToOwned, boxed::Box, fmt::Debug, sync::Arc, vec::Vec};
use bevy_ecs::{
    entity::{ContainsEntity, Entity, EntityHandle, RemoteAllocator, WeakEntityHandle},
    world::World,
};
use bevy_platform::collections::{hash_map::Entry, HashMap, HashSet};
use bevy_tasks::Task;
use bevy_utils::TypeIdMap;
use core::{any::TypeId, task::Waker};
use crossbeam_channel::Sender;
use tracing::{error, warn};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct AssetInfo {
    pub(crate) handle: StrongOrWeakHandle,
    pub(crate) path: Option<AssetPath<'static>>,
    pub(crate) uuid: Option<Uuid>,
    pub(crate) load_state: LoadState,
    pub(crate) dep_load_state: DependencyLoadState,
    pub(crate) rec_dep_load_state: RecursiveDependencyLoadState,
    pub(crate) loaded_type_id: Option<TypeId>,
    loading_dependencies: HashSet<Entity>,
    failed_dependencies: HashSet<Entity>,
    loading_rec_dependencies: HashSet<Entity>,
    failed_rec_dependencies: HashSet<Entity>,
    dependents_waiting_on_load: HashSet<Entity>,
    dependents_waiting_on_recursive_dep_load: HashSet<Entity>,
    /// The asset paths required to load this asset. Hashes will only be set for processed assets.
    /// This is set using the value from [`LoadedAsset`].
    /// This will only be populated if [`AssetInfos::watching_for_changes`] is set to `true` to
    /// save memory.
    ///
    /// [`LoadedAsset`]: crate::loader::LoadedAsset
    loader_dependencies: HashMap<AssetPath<'static>, AssetHash>,
    /// List of tasks waiting for this asset to complete loading
    pub(crate) waiting_tasks: Vec<Waker>,
}

pub(crate) enum StrongOrWeakHandle {
    Strong(EntityHandle),
    Weak(WeakEntityHandle),
}

impl StrongOrWeakHandle {
    pub(crate) fn get_strong(&self) -> Option<EntityHandle> {
        match self {
            StrongOrWeakHandle::Strong(entity_handle) => Some(entity_handle.clone()),
            StrongOrWeakHandle::Weak(weak_entity_handle) => weak_entity_handle.upgrade(),
        }
    }

    fn weak(&self) -> WeakEntityHandle {
        match self {
            StrongOrWeakHandle::Strong(entity_handle) => entity_handle.weak(),
            StrongOrWeakHandle::Weak(weak_entity_handle) => weak_entity_handle.clone(),
        }
    }
}

impl Debug for StrongOrWeakHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Strong(arg0) => f
                .debug_tuple("Strong")
                .field(&arg0.data::<AssetData>())
                .finish(),
            Self::Weak(arg0) => f.debug_tuple("Weak").field(arg0).finish(),
        }
    }
}

impl AssetInfo {
    fn new(
        handle: StrongOrWeakHandle,
        path: Option<AssetPath<'static>>,
        uuid: Option<Uuid>,
    ) -> Self {
        Self {
            handle,
            path,
            uuid,
            load_state: LoadState::NotLoaded,
            dep_load_state: DependencyLoadState::NotLoaded,
            rec_dep_load_state: RecursiveDependencyLoadState::NotLoaded,
            loading_dependencies: HashSet::default(),
            failed_dependencies: HashSet::default(),
            loading_rec_dependencies: HashSet::default(),
            failed_rec_dependencies: HashSet::default(),
            loader_dependencies: HashMap::default(),
            dependents_waiting_on_load: HashSet::default(),
            dependents_waiting_on_recursive_dep_load: HashSet::default(),
            waiting_tasks: Vec::new(),
            loaded_type_id: None,
        }
    }
}

/// Tracks statistics of the asset server.
#[derive(Default, Clone, PartialEq, Eq)]
pub(crate) struct AssetServerStats {
    /// The number of load tasks that have been started.
    pub(crate) started_load_tasks: usize,
}

pub(crate) struct AssetInfos {
    path_to_entity: HashMap<AssetPath<'static>, Entity>,
    uuid_to_entity: HashMap<Uuid, Entity>,
    type_id_to_entity: HashMap<TypeId, Entity>,
    remote_allocator: RemoteAllocator,
    infos: HashMap<Entity, AssetInfo>,
    /// If set to `true`, this informs [`AssetInfos`] to track data relevant to watching for changes (such as `load_dependents`)
    /// This should only be set at startup.
    pub(crate) watching_for_changes: bool,
    /// Tracks assets that depend on the "key" asset path inside their asset loaders ("loader dependencies")
    /// This should only be set when watching for changes to avoid unnecessary work.
    pub(crate) loader_dependents: HashMap<AssetPath<'static>, HashSet<AssetPath<'static>>>,
    /// Tracks living labeled assets for a given source asset.
    /// This should only be set when watching for changes to avoid unnecessary work.
    pub(crate) living_labeled_assets: HashMap<AssetPath<'static>, HashSet<Box<str>>>,
    pub(crate) asset_type_data: TypeIdMap<AssetTypeData>,
    pub(crate) pending_tasks: HashMap<Entity, Task<()>>,
    /// The stats that have collected during usage of the asset server.
    pub(crate) stats: AssetServerStats,
}

pub(crate) struct AssetTypeData {
    pub(crate) type_name: &'static str,
    pub(crate) asset_event_senders: AssetEventSenders,
}

impl Debug for AssetInfos {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AssetInfos")
            .field("path_to_index", &self.path_to_entity)
            .field("infos", &self.infos)
            .finish()
    }
}

impl AssetInfos {
    pub fn new(remote_allocator: RemoteAllocator) -> Self {
        Self {
            remote_allocator,
            path_to_entity: Default::default(),
            uuid_to_entity: Default::default(),
            type_id_to_entity: Default::default(),
            infos: Default::default(),
            watching_for_changes: Default::default(),
            loader_dependents: Default::default(),
            living_labeled_assets: Default::default(),
            pending_tasks: Default::default(),
            stats: Default::default(),
            asset_type_data: Default::default(),
        }
    }

    fn create_handle_internal(
        infos: &mut HashMap<Entity, AssetInfo>,
        remote_allocator: &RemoteAllocator,
        living_labeled_assets: &mut HashMap<AssetPath<'static>, HashSet<Box<str>>>,
        data: AssetData,
        watching_for_changes: bool,
        hold_handle: bool,
        loading: bool,
    ) -> UntypedHandle {
        if watching_for_changes && let Some(path) = &data.path {
            let mut without_label = path.to_owned();
            if let Some(label) = without_label.take_label() {
                let labels = living_labeled_assets.entry(without_label).or_default();
                labels.insert(label.as_ref().into());
            }
        }

        let path = data.path.clone();
        let uuid = data.uuid.clone();
        let entity_handle = remote_allocator.alloc_handle_with_data(data);
        let info_handle = if hold_handle {
            StrongOrWeakHandle::Strong(entity_handle.clone())
        } else {
            StrongOrWeakHandle::Weak(entity_handle.weak())
        };
        let mut info = AssetInfo::new(info_handle, path, uuid);
        if loading {
            info.load_state = LoadState::Loading;
            info.dep_load_state = DependencyLoadState::Loading;
            info.rec_dep_load_state = RecursiveDependencyLoadState::Loading;
        }
        infos.insert(entity_handle.entity(), info);

        UntypedHandle(entity_handle)
    }

    /// Retrieves asset tracking data, or creates it if it doesn't exist.
    /// Returns true if an asset load should be kicked off
    pub(crate) fn get_or_create_handle(
        &mut self,
        loading_mode: HandleLoadingMode,
        data: AssetData,
    ) -> (UntypedHandle, bool) {
        if let Some(path) = &data.path {
            match self.path_to_entity.entry(path.clone()) {
                Entry::Occupied(mut entry) => {
                    let entity = *entry.get();
                    let (handle, should_load) = Self::occupied_entry(
                        &mut self.infos,
                        &self.remote_allocator,
                        &mut self.living_labeled_assets,
                        self.watching_for_changes,
                        entity,
                        data,
                        loading_mode,
                        false,
                    );
                    *entry.get_mut() = handle.entity();
                    (handle, should_load)
                }
                // The entry does not exist, so this is a "fresh" asset load. We must create a new handle
                Entry::Vacant(entry) => {
                    let (handle, should_load) = Self::vacant_entry(
                        &mut self.infos,
                        &self.remote_allocator,
                        &mut self.living_labeled_assets,
                        self.watching_for_changes,
                        data,
                        loading_mode,
                        false,
                    );
                    entry.insert(handle.entity());
                    (handle, should_load)
                }
            }
        } else if let Some(uuid) = &data.uuid {
            match self.uuid_to_entity.entry(*uuid) {
                Entry::Occupied(mut entry) => {
                    let entity = *entry.get();
                    let (handle, _should_load) = Self::occupied_entry(
                        &mut self.infos,
                        &self.remote_allocator,
                        &mut self.living_labeled_assets,
                        self.watching_for_changes,
                        entity,
                        data,
                        loading_mode,
                        true,
                    );
                    *entry.get_mut() = handle.entity();
                    (handle, false)
                }
                Entry::Vacant(entry) => {
                    let (handle, _) = Self::vacant_entry(
                        &mut self.infos,
                        &self.remote_allocator,
                        &mut self.living_labeled_assets,
                        self.watching_for_changes,
                        data,
                        loading_mode,
                        true,
                    );
                    entry.insert(handle.entity());
                    (handle, false)
                }
            }
        } else if data.is_default {
            if let Some(type_id) = data.type_id_hint {
                match self.type_id_to_entity.entry(type_id) {
                    Entry::Occupied(mut entry) => {
                        let entity = *entry.get();
                        let (handle, _should_load) = Self::occupied_entry(
                            &mut self.infos,
                            &self.remote_allocator,
                            &mut self.living_labeled_assets,
                            self.watching_for_changes,
                            entity,
                            data,
                            loading_mode,
                            true,
                        );
                        *entry.get_mut() = handle.entity();
                        (handle, false)
                    }
                    Entry::Vacant(entry) => {
                        let (handle, _) = Self::vacant_entry(
                            &mut self.infos,
                            &self.remote_allocator,
                            &mut self.living_labeled_assets,
                            self.watching_for_changes,
                            data,
                            loading_mode,
                            true,
                        );
                        entry.insert(handle.entity());
                        (handle, false)
                    }
                }
            } else {
                error!("Attempted to create a handle to a default asset for a type, but a type was not defined");
                (
                    Self::create_handle_internal(
                        &mut self.infos,
                        &self.remote_allocator,
                        &mut self.living_labeled_assets,
                        data,
                        self.watching_for_changes,
                        false,
                        true,
                    ),
                    false,
                )
            }
        } else {
            (
                Self::create_handle_internal(
                    &mut self.infos,
                    &self.remote_allocator,
                    &mut self.living_labeled_assets,
                    data,
                    self.watching_for_changes,
                    false,
                    true,
                ),
                false,
            )
        }
    }

    fn occupied_entry(
        infos: &mut HashMap<Entity, AssetInfo>,
        remote_allocator: &RemoteAllocator,
        living_labeled_assets: &mut HashMap<AssetPath<'static>, HashSet<Box<str>>>,
        watching_for_changes: bool,
        entity: Entity,
        data: AssetData,
        loading_mode: HandleLoadingMode,
        hold_handle: bool,
    ) -> (UntypedHandle, bool) {
        // if there is a path_to_id entry, info always exists
        let info = infos.get_mut(&entity).unwrap();
        let mut should_load = false;
        if loading_mode == HandleLoadingMode::Force
            || (loading_mode == HandleLoadingMode::Request
                && matches!(info.load_state, LoadState::NotLoaded | LoadState::Failed(_)))
        {
            info.load_state = LoadState::Loading;
            info.dep_load_state = DependencyLoadState::Loading;
            info.rec_dep_load_state = RecursiveDependencyLoadState::Loading;
            should_load = true;
        }

        if let Some(entity_handle) = info.handle.get_strong() {
            // If we can upgrade the handle, there is at least one live handle right now,
            // The asset load has already kicked off (and maybe completed), so we can just
            // return a strong handle
            (UntypedHandle(entity_handle), should_load)
        } else {
            // Asset meta exists, but all live handles were dropped. This means the `track_assets` system
            // hasn't been run yet to remove the current asset
            // We must create a new strong handle
            let handle = Self::create_handle_internal(
                infos,
                remote_allocator,
                living_labeled_assets,
                data,
                watching_for_changes,
                hold_handle,
                true,
            );
            (handle, true)
        }
    }
    fn vacant_entry(
        infos: &mut HashMap<Entity, AssetInfo>,
        remote_allocator: &RemoteAllocator,
        living_labeled_assets: &mut HashMap<AssetPath<'static>, HashSet<Box<str>>>,
        watching_for_changes: bool,
        data: AssetData,
        loading_mode: HandleLoadingMode,
        hold_handle: bool,
    ) -> (UntypedHandle, bool) {
        let should_load = match loading_mode {
            HandleLoadingMode::NotLoading => false,
            HandleLoadingMode::Request | HandleLoadingMode::Force => true,
        };
        (
            Self::create_handle_internal(
                infos,
                remote_allocator,
                living_labeled_assets,
                data,
                watching_for_changes,
                hold_handle,
                should_load,
            ),
            should_load,
        )
    }

    pub(crate) fn get(&self, entity: Entity) -> Option<&AssetInfo> {
        self.infos.get(&entity)
    }

    pub(crate) fn contains_key(&self, entity: Entity) -> bool {
        self.infos.contains_key(&entity)
    }

    pub(crate) fn get_mut(&mut self, entity: Entity) -> Option<&mut AssetInfo> {
        self.infos.get_mut(&entity)
    }

    pub(crate) fn get_path_entity(&self, path: &AssetPath<'_>) -> Option<Entity> {
        self.path_to_entity.get(path).copied()
    }

    pub(crate) fn get_path_handle(&self, path: &AssetPath<'_>) -> Option<UntypedHandle> {
        let entity = *self.path_to_entity.get(path)?;
        self.get_entity_handle(entity)
    }

    pub(crate) fn get_entity_handle(&self, entity: Entity) -> Option<UntypedHandle> {
        let info = self.infos.get(&entity)?;
        let entity_handle = info.handle.get_strong()?;
        Some(UntypedHandle(entity_handle))
    }

    pub(crate) fn process_handle_drop(&mut self, entity: Entity) {
        let Entry::Occupied(entry) = self.infos.entry(entity) else {
            return;
        };

        self.pending_tasks.remove(&entity);

        let info = entry.remove();
        if let Some(path) = &info.path {
            if self.watching_for_changes {
                Self::remove_dependents_and_labels(
                    &info,
                    &mut self.loader_dependents,
                    path,
                    &mut self.living_labeled_assets,
                );
            }

            self.path_to_entity.remove(path);
        };

        if let Some(uuid) = &info.uuid {
            self.uuid_to_entity.remove(uuid);
        }
    }

    /// Updates [`AssetInfo`] / load state for an asset that has finished loading (and relevant dependencies / dependents).
    pub(crate) fn process_asset_load(
        &mut self,
        loaded_entity: Entity,
        loaded_asset: ErasedLoadedAsset,
        world: &mut World,
        sender: &Sender<InternalAssetEvent>,
    ) {
        let loaded_type_id = loaded_asset.asset_type_id();
        // Process all the labeled assets first so that they don't get skipped due to the "parent"
        // not having its handle alive.
        for asset in loaded_asset.labeled_assets {
            self.process_asset_load(asset.handle.entity(), asset.asset, world, sender);
        }

        let mut loading_deps = loaded_asset.dependencies;
        let mut failed_deps = <HashSet<_>>::default();
        let mut dep_error = None;
        let mut loading_rec_deps = loading_deps.clone();
        let mut failed_rec_deps = <HashSet<_>>::default();
        let mut rec_dep_error = None;
        loading_deps.retain(|dep_id| {
            if let Some(dep_info) = self.get_mut(*dep_id) {
                match dep_info.rec_dep_load_state {
                    RecursiveDependencyLoadState::Loading
                    | RecursiveDependencyLoadState::NotLoaded => {
                        // If dependency is loading, wait for it.
                        dep_info
                            .dependents_waiting_on_recursive_dep_load
                            .insert(loaded_entity);
                    }
                    RecursiveDependencyLoadState::Loaded => {
                        // If dependency is loaded, reduce our count by one
                        loading_rec_deps.remove(dep_id);
                    }
                    RecursiveDependencyLoadState::Failed(ref error) => {
                        if rec_dep_error.is_none() {
                            rec_dep_error = Some(error.clone());
                        }
                        failed_rec_deps.insert(*dep_id);
                        loading_rec_deps.remove(dep_id);
                    }
                }
                match dep_info.load_state {
                    LoadState::NotLoaded | LoadState::Loading => {
                        // If dependency is loading, wait for it.
                        dep_info.dependents_waiting_on_load.insert(loaded_entity);
                        true
                    }
                    LoadState::Loaded => {
                        // If dependency is loaded, reduce our count by one
                        false
                    }
                    LoadState::Failed(ref error) => {
                        if dep_error.is_none() {
                            dep_error = Some(error.clone());
                        }
                        failed_deps.insert(*dep_id);
                        false
                    }
                }
            } else {
                // the dependency id does not exist, which implies it was manually removed or never existed in the first place
                warn!(
                    "Dependency {} from asset {} is unknown. This asset's dependency load status will not switch to 'Loaded' until the unknown dependency is loaded.",
                    dep_id, loaded_entity
                );
                true
            }
        });

        let dep_load_state = match (loading_deps.len(), failed_deps.len()) {
            (0, 0) => DependencyLoadState::Loaded,
            (_loading, 0) => DependencyLoadState::Loading,
            (_loading, _failed) => DependencyLoadState::Failed(dep_error.unwrap()),
        };

        let rec_dep_load_state = match (loading_rec_deps.len(), failed_rec_deps.len()) {
            (0, 0) => {
                sender
                    .send(InternalAssetEvent::LoadedWithDependencies {
                        entity: loaded_entity,
                    })
                    .unwrap();
                RecursiveDependencyLoadState::Loaded
            }
            (_loading, 0) => RecursiveDependencyLoadState::Loading,
            (_loading, _failed) => RecursiveDependencyLoadState::Failed(rec_dep_error.unwrap()),
        };

        let (dependents_waiting_on_load, dependents_waiting_on_rec_load) = {
            let watching_for_changes = self.watching_for_changes;
            // if watching for changes, track reverse loader dependencies for hot reloading
            if watching_for_changes {
                let info = self
                    .infos
                    .get(&loaded_entity)
                    .expect("Asset info should always exist at this point");
                if let Some(asset_path) = &info.path {
                    for loader_dependency in loaded_asset.loader_dependencies.keys() {
                        let dependents = self
                            .loader_dependents
                            .entry(loader_dependency.clone())
                            .or_default();
                        dependents.insert(asset_path.clone());
                    }
                }
            }
            let info = self
                .get_mut(loaded_entity)
                .expect("Asset info should always exist at this point");
            info.loading_dependencies = loading_deps;
            info.failed_dependencies = failed_deps;
            info.loading_rec_dependencies = loading_rec_deps;
            info.failed_rec_dependencies = failed_rec_deps;
            info.load_state = LoadState::Loaded;
            info.loaded_type_id = Some(loaded_type_id);
            info.dep_load_state = dep_load_state;
            info.rec_dep_load_state = rec_dep_load_state.clone();
            if watching_for_changes {
                info.loader_dependencies = loaded_asset.loader_dependencies;
            }

            loaded_asset
                .value
                .insert(loaded_entity, info.handle.weak(), world);
            let dependents_waiting_on_rec_load =
                if rec_dep_load_state.is_loaded() || rec_dep_load_state.is_failed() {
                    Some(core::mem::take(
                        &mut info.dependents_waiting_on_recursive_dep_load,
                    ))
                } else {
                    None
                };

            (
                core::mem::take(&mut info.dependents_waiting_on_load),
                dependents_waiting_on_rec_load,
            )
        };

        for id in dependents_waiting_on_load {
            if let Some(info) = self.get_mut(id) {
                info.loading_dependencies.remove(&loaded_entity);
                if info.loading_dependencies.is_empty() && !info.dep_load_state.is_failed() {
                    // send dependencies loaded event
                    info.dep_load_state = DependencyLoadState::Loaded;
                }
            }
        }

        if let Some(dependents_waiting_on_rec_load) = dependents_waiting_on_rec_load {
            match rec_dep_load_state {
                RecursiveDependencyLoadState::Loaded => {
                    for dep_id in dependents_waiting_on_rec_load {
                        Self::propagate_loaded_state(self, loaded_entity, dep_id, sender);
                    }
                }
                RecursiveDependencyLoadState::Failed(ref error) => {
                    for dep_id in dependents_waiting_on_rec_load {
                        Self::propagate_failed_state(self, loaded_entity, dep_id, error);
                    }
                }
                RecursiveDependencyLoadState::Loading | RecursiveDependencyLoadState::NotLoaded => {
                    // dependents_waiting_on_rec_load should be None in this case
                    unreachable!("`Loading` and `NotLoaded` state should never be propagated.")
                }
            }
        }
    }

    /// Recursively propagates loaded state up the dependency tree.
    fn propagate_loaded_state(
        infos: &mut AssetInfos,
        loaded_id: Entity,
        waiting_id: Entity,
        sender: &Sender<InternalAssetEvent>,
    ) {
        let dependents_waiting_on_rec_load = if let Some(info) = infos.get_mut(waiting_id) {
            info.loading_rec_dependencies.remove(&loaded_id);
            if info.loading_rec_dependencies.is_empty() && info.failed_rec_dependencies.is_empty() {
                info.rec_dep_load_state = RecursiveDependencyLoadState::Loaded;
                if info.load_state.is_loaded() {
                    sender
                        .send(InternalAssetEvent::LoadedWithDependencies { entity: waiting_id })
                        .unwrap();
                }
                Some(core::mem::take(
                    &mut info.dependents_waiting_on_recursive_dep_load,
                ))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(dependents_waiting_on_rec_load) = dependents_waiting_on_rec_load {
            for dep_id in dependents_waiting_on_rec_load {
                Self::propagate_loaded_state(infos, waiting_id, dep_id, sender);
            }
        }
    }

    /// Recursively propagates failed state up the dependency tree
    fn propagate_failed_state(
        infos: &mut AssetInfos,
        failed_id: Entity,
        waiting_id: Entity,
        error: &Arc<AssetLoadError>,
    ) {
        let dependents_waiting_on_rec_load = if let Some(info) = infos.get_mut(waiting_id) {
            info.loading_rec_dependencies.remove(&failed_id);
            info.failed_rec_dependencies.insert(failed_id);
            info.rec_dep_load_state = RecursiveDependencyLoadState::Failed(error.clone());
            Some(core::mem::take(
                &mut info.dependents_waiting_on_recursive_dep_load,
            ))
        } else {
            None
        };

        if let Some(dependents_waiting_on_rec_load) = dependents_waiting_on_rec_load {
            for dep_id in dependents_waiting_on_rec_load {
                Self::propagate_failed_state(infos, waiting_id, dep_id, error);
            }
        }
    }

    pub(crate) fn process_asset_fail(&mut self, failed_index: Entity, error: AssetLoadError) {
        // Check whether the handle has been dropped since the asset was loaded.
        if !self.infos.contains_key(&failed_index) {
            return;
        }

        let error = Arc::new(error);
        let (dependents_waiting_on_load, dependents_waiting_on_rec_load) = {
            let Some(info) = self.get_mut(failed_index) else {
                // The asset was already dropped.
                return;
            };
            info.load_state = LoadState::Failed(error.clone());
            info.dep_load_state = DependencyLoadState::Failed(error.clone());
            info.rec_dep_load_state = RecursiveDependencyLoadState::Failed(error.clone());
            for waker in info.waiting_tasks.drain(..) {
                waker.wake();
            }
            (
                core::mem::take(&mut info.dependents_waiting_on_load),
                core::mem::take(&mut info.dependents_waiting_on_recursive_dep_load),
            )
        };

        for waiting_id in dependents_waiting_on_load {
            if let Some(info) = self.get_mut(waiting_id) {
                info.loading_dependencies.remove(&failed_index);
                info.failed_dependencies.insert(failed_index);
                // don't overwrite DependencyLoadState if already failed to preserve first error
                if !info.dep_load_state.is_failed() {
                    info.dep_load_state = DependencyLoadState::Failed(error.clone());
                }
            }
        }

        for waiting_id in dependents_waiting_on_rec_load {
            Self::propagate_failed_state(self, failed_index, waiting_id, &error);
        }
    }

    fn remove_dependents_and_labels(
        info: &AssetInfo,
        loader_dependents: &mut HashMap<AssetPath<'static>, HashSet<AssetPath<'static>>>,
        path: &AssetPath<'static>,
        living_labeled_assets: &mut HashMap<AssetPath<'static>, HashSet<Box<str>>>,
    ) {
        for loader_dependency in info.loader_dependencies.keys() {
            if let Some(dependents) = loader_dependents.get_mut(loader_dependency) {
                dependents.remove(path);
            }
        }

        let Some(label) = path.label() else {
            return;
        };

        let mut without_label = path.to_owned();
        without_label.remove_label();

        let Entry::Occupied(mut entry) = living_labeled_assets.entry(without_label) else {
            return;
        };

        entry.get_mut().remove(label);
        if entry.get().is_empty() {
            entry.remove();
        }
    }
}
/// Determines how a handle should be initialized
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum HandleLoadingMode {
    /// The handle is for an asset that isn't loading/loaded yet.
    NotLoading,
    /// The handle is for an asset that is being _requested_ to load (if it isn't already loading)
    Request,
    /// The handle is for an asset that is being forced to load (even if it has already loaded)
    Force,
}

pub(crate) struct AssetEventSenders {
    pub(crate) loaded_with_dependencies: fn(&mut World, Entity),
    pub(crate) failed: fn(&mut World, Entity, AssetLoadError, AssetPath<'static>),
}

impl AssetEventSenders {
    pub(crate) fn new<A: Asset>() -> Self {
        Self {
            loaded_with_dependencies: |world, entity| {
                world.write_message(AssetEvent::<A>::LoadedWithDependencies { id: entity.into() });
            },
            failed: |world, entity, error, path| {
                world.write_message(AssetLoadFailedEvent::<A> {
                    id: entity.into(),
                    error,
                    path,
                });
            },
        }
    }
}
