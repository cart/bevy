use crate::{
    path::{AssetPath, SourcePathId},
    LabelId,
};
use bevy_utils::HashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceMeta<T = ()> {
    pub assets: Vec<AssetMeta>,
    pub loader: Uuid,
    pub hash: u64,
    pub config: T,
    // TODO: loader id
    // TODO: redirects
}

impl<T> SourceMeta<T>
where
    T: Default,
{
    pub fn new(loader: Uuid, hash: u64) -> Self {
        Self {
            assets: Default::default(),
            config: T::default(),
            hash,
            loader,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetMeta {
    pub label: Option<String>,
    pub dependencies: Vec<AssetPath<'static>>,
    pub type_uuid: Uuid,
    // TODO: hash
}

/// Info about a specific asset, such as its path and its current load state
#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub meta: SourceMeta,
    pub path: PathBuf,
    pub asset_types: HashMap<LabelId, Uuid>,
    pub load_state: LoadState,
    pub committed_assets: usize,
    pub version: usize,
}

impl SourceInfo {
    pub fn is_loaded(&self) -> bool {
        self.committed_assets == self.meta.assets.len()
    }

    pub fn get_asset_type(&self, label_id: LabelId) -> Option<Uuid> {
        self.asset_types.get(&label_id).cloned()
    }
}

/// The load state of an asset
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LoadState {
    Loading,
    Loaded,
    Failed,
}

#[derive(Default)]
pub struct AssetSources {
    source_infos: HashMap<SourcePathId, SourceInfo>,
}

impl AssetSources {
    pub fn add(&mut self, source_info: SourceInfo) {
        self.source_infos
            .insert(SourcePathId::from(source_info.path.as_path()), source_info);
    }

    pub fn set_load_state(
        &mut self,
        source_path_id: SourcePathId,
        load_state: LoadState,
        version: usize,
    ) {
        if let Some(source_info) = self.source_infos.get_mut(&source_path_id) {
            if version == source_info.version {
                source_info.load_state = load_state;
            }
        }
    }

    pub fn get_load_state(&self, source_path_id: SourcePathId) -> Option<LoadState> {
        self.source_infos
            .get(&source_path_id)
            .map(|source_info| source_info.load_state.clone())
    }

    pub fn get(&self, source_path_id: SourcePathId) -> Option<&SourceInfo> {
        self.source_infos.get(&source_path_id)
    }

    pub fn get_mut(&mut self, source_path_id: SourcePathId) -> Option<&mut SourceInfo> {
        self.source_infos.get_mut(&source_path_id)
    }

    pub fn increment_load_count(
        &mut self,
        source_path_id: SourcePathId,
    ) -> Option<&mut SourceInfo> {
        self.source_infos.get_mut(&source_path_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SourceInfo> {
        self.source_infos.values()
    }
}
