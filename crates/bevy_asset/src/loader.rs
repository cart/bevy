use crate::{
    io::{AssetReaderError, Reader},
    meta::{AssetMeta, AssetMetaDyn, Settings, META_FORMAT_VERSION},
    path::AssetPath,
    saver::NullSaver,
    Asset, AssetLoadError, AssetServer, Assets, Handle, UntypedAssetId, UntypedHandle,
};
use bevy_ecs::world::World;
use bevy_utils::{BoxedFuture, HashMap};
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
    ) -> BoxedFuture<'a, Result<LoadedAsset, AssetLoaderError>>;

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
    ) -> BoxedFuture<'a, Result<LoadedAsset, AssetLoaderError>> {
        Box::pin(async move {
            let settings = settings
                .downcast_ref::<L::Settings>()
                .expect("AssetLoader settings should match the loader type");
            let asset =
                <L as AssetLoader>::load(&self, reader, settings, &mut load_context).await?;
            Ok(LoadedAsset {
                id: load_context.asset_id,
                asset: Box::new(asset),
                labeled_assets: load_context.labeled_assets,
                dependencies: load_context.dependencies,
            })
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

pub struct LoadedAsset {
    pub id: UntypedAssetId,
    pub asset: Box<dyn AssetContainer>,
    pub labeled_assets: HashMap<String, LoadedAsset>,
    pub dependencies: HashMap<UntypedAssetId, AssetPath<'static>>,
}

impl LoadedAsset {
    pub fn take<A: Asset>(self) -> Option<A> {
        self.asset.downcast::<A>().map(|a| *a).ok()
    }

    pub fn get<A: Asset>(&self) -> Option<&A> {
        self.asset.downcast_ref::<A>()
    }
}

pub trait AssetContainer: Downcast + Any + Send + Sync + 'static {
    fn insert(self: Box<Self>, id: UntypedAssetId, world: &mut World);
}

impl_downcast!(AssetContainer);

impl<A: Asset> AssetContainer for A {
    fn insert(self: Box<Self>, id: UntypedAssetId, world: &mut World) {
        world
            .resource_mut::<Assets<A>>()
            .insert(id.typed_unchecked(), *self);
    }
}

pub struct LoadContext<'a> {
    pub(crate) asset_server: &'a AssetServer,
    pub(crate) labeled_assets: HashMap<String, LoadedAsset>,
    // TODO: merge with labeled_assets?
    // TODO: maybe don't use this approach at all?
    pub(crate) labeled_handles: HashMap<String, UntypedHandle>,
    pub(crate) asset_path: &'a AssetPath<'a>,
    pub(crate) asset_id: UntypedAssetId,
    pub(crate) dependencies: HashMap<UntypedAssetId, AssetPath<'static>>,
    pub(crate) load_dependencies: Vec<AssetPath<'static>>,
    pub(crate) should_load_dependencies: bool,
}

impl<'a> LoadContext<'a> {
    pub(crate) fn new(
        asset_server: &'a AssetServer,
        asset_id: UntypedAssetId,
        asset_path: &'a AssetPath<'a>,
        load_dependencies: bool,
    ) -> Self {
        Self {
            asset_server,
            asset_path,
            asset_id,
            should_load_dependencies: load_dependencies,
            dependencies: HashMap::new(),
            load_dependencies: Vec::new(),
            labeled_assets: Default::default(),
            labeled_handles: Default::default(),
        }
    }

    /// Gets the source path for this load context.
    pub fn path(&self) -> &Path {
        self.asset_path.path()
    }

    /// Gets the source asset path for this load context.
    pub fn asset_path(&self) -> &AssetPath {
        self.asset_path
    }

    /// Gets the source asset path for this load context.
    pub async fn read_asset_bytes<'b>(
        &mut self,
        path: &'b Path,
    ) -> Result<Vec<u8>, AssetReaderError> {
        self.load_dependencies
            .push(AssetPath::new(path.to_owned(), None));
        let mut reader = self.asset_server.reader().read(path).await?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(bytes)
    }

    /// Returns `true` if the load context contains an asset with the specified label.
    pub fn has_labeled_asset(&self, label: &str) -> bool {
        self.labeled_assets.contains_key(label)
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
                .get_path_handle(path.clone(), TypeId::of::<A>())
                .typed_unchecked()
        };
        self.dependencies.insert(handle.id().untyped(), path);
        handle
    }

    /// Returns the handle for a labeled asset, if it exists.
    pub fn get_labeled_handle<'b, A: Asset>(&mut self, label: &str) -> Option<Handle<A>> {
        let handle = self.labeled_handles.get(label)?.clone().typed();
        let path = AssetPath::new(self.path().to_owned(), Some(label.to_string()));
        self.dependencies.insert(handle.id().untyped(), path);
        Some(handle)
    }

    pub async fn load_direct_async<'b, P: Into<AssetPath<'b>>>(
        &mut self,
        path: P,
    ) -> Result<LoadedAsset, AssetLoadError> {
        let path = path.into();
        self.load_dependencies.push(path.to_owned());
        self.asset_server.load_direct_async(path).await
    }

    /// This will start a new load context for a labeled asset. This ensures that all `load` calls made in this context
    /// will be associated with the labeled asset (instead of the root asset directly returned by the loader).  
    /// Call [`LabeledLoadContext::finish`] to provide the final asset value and return to the main load context.
    //
    // DESIGN NOTE: If we required Asset types to be able to enumerate their dependencies, we could probably do away with
    // this "context scoping". However that would also mean we don't kick off asset loads _during_ the loader, which could
    // result in assets loading faster.
    pub fn begin_labeled_asset<'b>(&'b mut self, label: String) -> LabeledLoadContext<'b, 'a> {
        LabeledLoadContext {
            load_context: self,
            label,
            dependencies: Default::default(),
        }
    }
}

pub struct LabeledLoadContext<'a, 'b> {
    load_context: &'a mut LoadContext<'b>,
    dependencies: HashMap<UntypedAssetId, AssetPath<'static>>,
    label: String,
}

impl<'a, 'b> LabeledLoadContext<'a, 'b> {
    /// Gets the source path for this load context.
    pub fn path(&self) -> &Path {
        self.load_context.path()
    }
    pub fn load<'c, A: Asset, P: Into<AssetPath<'c>>>(&mut self, path: P) -> Handle<A> {
        let path = path.into().to_owned();
        let handle = if self.load_context.should_load_dependencies {
            self.load_context.asset_server.load(path.clone())
        } else {
            self.load_context
                .asset_server
                .get_path_handle(path.clone(), TypeId::of::<A>())
                .typed_unchecked()
        };
        self.dependencies.insert(handle.id().untyped(), path);
        handle
    }

    pub fn get_labeled_handle<'c, A: Asset>(&mut self, label: &str) -> Option<Handle<A>> {
        let untyped_handle = self.load_context.labeled_handles.get(label)?;
        let handle = untyped_handle.clone().typed();
        let path = AssetPath::new(self.path().to_owned(), Some(label.to_string()));
        self.dependencies.insert(handle.id().untyped(), path);
        Some(handle)
    }

    pub async fn load_direct_async<'c, P: Into<AssetPath<'c>>>(
        &mut self,
        path: P,
    ) -> Result<LoadedAsset, AssetLoadError> {
        let path = path.into();
        // The top level asset owns all load dependencies
        self.load_context.load_dependencies.push(path.to_owned());
        self.load_context.asset_server.load_direct_async(path).await
    }

    pub fn finish<A: Asset>(self, asset: A) -> Handle<A> {
        let path = AssetPath::new(
            self.load_context.path().to_owned(),
            Some(self.label.clone()),
        );
        let handle = self
            .load_context
            .asset_server
            .get_path_handle(path.clone(), TypeId::of::<A>())
            .typed_unchecked();
        let id = handle.id().untyped();
        // main asset should depend on labeled asset for correct recursive load state
        self.load_context.dependencies.insert(id, path.clone());
        self.load_context
            .labeled_handles
            .insert(self.label.clone(), handle.clone().untyped());
        self.load_context.labeled_assets.insert(
            self.label,
            LoadedAsset {
                id,
                asset: Box::new(asset),
                labeled_assets: Default::default(),
                dependencies: self.dependencies,
            },
        );
        handle
    }
}
