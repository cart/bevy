use crate::{
    io::{AssetReaderError, Reader},
    meta::{AssetMeta, AssetMetaDyn, Settings, META_FORMAT_VERSION},
    path::AssetPath,
    saver::NullSaver,
    Asset, AssetLoadError, AssetServer, Assets, Handle, UntypedAssetId, UntypedHandle,
};
use bevy_ecs::world::World;
use bevy_utils::{BoxedFuture, HashSet};
use downcast_rs::{impl_downcast, Downcast};
use futures_lite::AsyncReadExt;
use ron::error::SpannedError;
use serde::{Deserialize, Serialize};
use std::{
    any::{Any, TypeId},
    path::Path,
};
use thiserror::Error;

pub trait AssetLoader: Send + Sync + 'static {
    type Asset: crate::Asset;
    type Settings: Settings + Default + Serialize + for<'a> Deserialize<'a>;
    /// Processes the asset in an asynchronous closure.
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        settings: &'a Self::Settings,
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, Result<Self::Asset, anyhow::Error>>;

    /// Returns a list of extensions supported by this asset loader, without the preceding dot.
    fn extensions(&self) -> &[&str];
}

pub trait ErasedAssetLoader: Send + Sync + 'static {
    /// Processes the asset in an asynchronous closure.
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        settings: &'a dyn Settings,
        load_context: LoadContext<'a>,
    ) -> BoxedFuture<'a, Result<ErasedLoadedAsset, AssetLoaderError>>;

    /// Returns a list of extensions supported by this asset loader, without the preceding dot.
    fn extensions(&self) -> &[&str];
    fn deserialize_meta(&self, meta: &[u8]) -> Result<Box<dyn AssetMetaDyn>, DeserializeMetaError>;
    fn default_meta(&self) -> Box<dyn AssetMetaDyn>;
    fn type_name(&self) -> &'static str;
    fn type_id(&self) -> TypeId;
    fn asset_type_id(&self) -> TypeId;
}

#[derive(Error, Debug)]
pub enum AssetLoaderError {
    #[error(transparent)]
    Load(#[from] anyhow::Error),
    #[error(transparent)]
    DeserializeMeta(#[from] DeserializeMetaError),
}

#[derive(Error, Debug)]
pub enum DeserializeMetaError {
    #[error("Failed to deserialize asset meta: {0:?}")]
    DeserializeSettings(#[from] SpannedError),
}

impl<L> ErasedAssetLoader for L
where
    L: AssetLoader + Send + Sync,
{
    /// Processes the asset in an asynchronous closure.
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        settings: &'a dyn Settings,
        mut load_context: LoadContext<'a>,
    ) -> BoxedFuture<'a, Result<ErasedLoadedAsset, AssetLoaderError>> {
        Box::pin(async move {
            let settings = settings
                .downcast_ref::<L::Settings>()
                .expect("AssetLoader settings should match the loader type");
            let asset =
                <L as AssetLoader>::load(&self, reader, settings, &mut load_context).await?;
            Ok(load_context.finish(asset).into())
        })
    }

    fn deserialize_meta(&self, meta: &[u8]) -> Result<Box<dyn AssetMetaDyn>, DeserializeMetaError> {
        let meta: AssetMeta<L, NullSaver, L> = ron::de::from_bytes(meta)?;
        Ok(Box::new(meta))
    }

    fn default_meta(&self) -> Box<dyn AssetMetaDyn> {
        Box::new(AssetMeta::<L, NullSaver, L> {
            meta_format_version: META_FORMAT_VERSION.to_string(),
            loader_settings: L::Settings::default(),
            loader: self.type_name().to_string(),
            processor: None,
        })
    }

    /// Returns a list of extensions supported by this asset loader, without the preceding dot.
    fn extensions(&self) -> &[&str] {
        <L as AssetLoader>::extensions(&self)
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<L>()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<L>()
    }

    fn asset_type_id(&self) -> TypeId {
        TypeId::of::<L::Asset>()
    }
}

pub struct LoadedAsset<A: Asset> {
    pub(crate) value: A,
    pub(crate) path: Option<AssetPath<'static>>,
    pub(crate) dependencies: HashSet<UntypedHandle>,
    pub(crate) load_dependencies: HashSet<AssetPath<'static>>,
}

impl<A: Asset> From<A> for LoadedAsset<A> {
    fn from(value: A) -> Self {
        LoadedAsset {
            value,
            path: None,
            dependencies: HashSet::default(),
            load_dependencies: HashSet::default(),
        }
    }
}

pub struct ErasedLoadedAsset {
    pub(crate) value: Box<dyn AssetContainer>,
    pub(crate) path: Option<AssetPath<'static>>,
    pub(crate) dependencies: HashSet<UntypedHandle>,
    #[allow(unused)]
    pub(crate) load_dependencies: HashSet<AssetPath<'static>>,
}

impl<A: Asset> From<LoadedAsset<A>> for ErasedLoadedAsset {
    fn from(asset: LoadedAsset<A>) -> Self {
        ErasedLoadedAsset {
            value: Box::new(asset.value),
            path: asset.path,
            dependencies: asset.dependencies,
            load_dependencies: asset.load_dependencies,
        }
    }
}

impl ErasedLoadedAsset {
    pub fn take<A: Asset>(self) -> Option<A> {
        self.value.downcast::<A>().map(|a| *a).ok()
    }

    pub fn get<A: Asset>(&self) -> Option<&A> {
        self.value.downcast_ref::<A>()
    }

    pub fn asset_type_id(&self) -> TypeId {
        (&*self.value).type_id()
    }

    pub fn path(&self) -> Option<&AssetPath<'static>> {
        self.path.as_ref()
    }
}

pub trait AssetContainer: Downcast + Any + Send + Sync + 'static {
    fn insert(self: Box<Self>, id: UntypedAssetId, world: &mut World);
}

impl_downcast!(AssetContainer);

impl<A: Asset> AssetContainer for A {
    fn insert(self: Box<Self>, id: UntypedAssetId, world: &mut World) {
        world.resource_mut::<Assets<A>>().insert(id.typed(), *self);
    }
}

pub struct LoadContext<'a> {
    asset_server: &'a AssetServer,
    should_load_dependencies: bool,
    asset_path: AssetPath<'static>,
    dependencies: HashSet<UntypedHandle>,
    load_dependencies: HashSet<AssetPath<'static>>,
}

impl<'a> LoadContext<'a> {
    pub(crate) fn new(
        asset_server: &'a AssetServer,
        asset_path: AssetPath<'static>,
        load_dependencies: bool,
    ) -> Self {
        Self {
            asset_server,
            asset_path,
            should_load_dependencies: load_dependencies,
            dependencies: HashSet::default(),
            load_dependencies: HashSet::default(),
        }
    }

    /// Begins an assetload
    pub fn begin_labeled_asset(&self, label: String) -> LoadContext {
        LoadContext::new(
            self.asset_server,
            self.asset_path.with_label(label),
            self.should_load_dependencies,
        )
    }

    pub fn labeled_asset_scope<A: Asset>(
        &mut self,
        label: String,
        load: impl FnOnce(&mut LoadContext) -> A,
    ) -> Handle<A> {
        let mut context = self.begin_labeled_asset(label);
        let asset = load(&mut context);
        self.load_asset(context.finish(asset))
    }

    pub fn add_labeled_asset<A: Asset>(&mut self, label: String, asset: A) -> Handle<A> {
        self.labeled_asset_scope(label, |_| asset)
    }

    pub fn has_labeled_asset(&self, label: &str) -> bool {
        let path = self.asset_path.with_label(label);
        self.asset_server.get_handle_untyped(path).is_some()
    }

    pub fn finish<A: Asset>(self, value: A) -> LoadedAsset<A> {
        LoadedAsset {
            value,
            path: Some(self.asset_path),
            dependencies: self.dependencies,
            load_dependencies: self.load_dependencies,
        }
    }

    /// Gets the source path for this load context.
    pub fn path(&self) -> &Path {
        self.asset_path.path()
    }

    /// Gets the source asset path for this load context.
    pub fn asset_path(&self) -> &AssetPath {
        &self.asset_path
    }

    /// Gets the source asset path for this load context.
    pub async fn read_asset_bytes<'b>(
        &mut self,
        path: &'b Path,
    ) -> Result<Vec<u8>, AssetReaderError> {
        self.load_dependencies
            .insert(AssetPath::new(path.to_owned(), None));
        let mut reader = self.asset_server.reader().read(path).await?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(bytes)
    }

    /// Retrieves a handle for the asset at the given path and adds that path as a dependency of the asset.
    /// If the current context is a normal [`AssetServer::load`], an actual asset load will be kicked off immediately, which ensures the load happens
    /// as soon as possible.
    /// If the current context is an [`AssetServer::load_direct_async`] (such as in the [`AssetProcessor`](crate::processor::AssetProcessor)),
    /// a load will not be kicked off automatically. It is then the calling context's responsibility to begin a load if necessary.
    pub fn load<'b, A: Asset, P: Into<AssetPath<'b>>>(&mut self, path: P) -> Handle<A> {
        let path = path.into().to_owned();
        let handle = if self.should_load_dependencies {
            self.asset_server.load(path.clone())
        } else {
            self.asset_server
                .get_or_create_path_handle(path.clone(), TypeId::of::<A>())
                .typed_debug_checked()
        };
        self.dependencies.insert(handle.clone().untyped());
        handle
    }

    pub fn load_asset<A: Asset>(&mut self, asset: impl Into<LoadedAsset<A>>) -> Handle<A> {
        let handle = self.asset_server.load_asset(asset);
        self.dependencies.insert(handle.clone().untyped());
        handle
    }

    pub fn get_label_handle<'c, A: Asset>(&mut self, label: &str) -> Handle<A> {
        let path = self.asset_path.with_label(label);
        let handle = self
            .asset_server
            .get_or_create_path_handle(path.to_owned(), TypeId::of::<A>())
            .typed_debug_checked();
        self.dependencies.insert(handle.clone().untyped());
        handle
    }

    pub async fn load_direct_async<'b, P: Into<AssetPath<'b>>>(
        &mut self,
        path: P,
    ) -> Result<ErasedLoadedAsset, AssetLoadError> {
        let path = path.into();
        self.load_dependencies.insert(path.to_owned());
        self.asset_server.load_direct_async(path).await
    }
}
