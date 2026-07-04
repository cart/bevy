//! Add methods on `World` to simplify loading assets when all
//! you have is a `World`.

use bevy_ecs::{
    component::{Component, Mutable},
    system::Commands,
    world::{Mut, World},
};
use uuid::Uuid;

use crate::{
    meta::Settings, Asset, AssetData, AssetId, AssetReference, AssetServer, Handle, LoadBuilder,
};

/// An extension trait for methods for working with assets directly from a [`World`].
pub trait DirectAssetAccessExt {
    /// Insert an asset similarly to [`Assets::add`].
    #[deprecated(since = "0.19.0", note = "use World::spawn_asset instead")]
    fn add_asset<A: Asset>(&mut self, asset: impl Into<A>) -> Handle<A>;

    /// Insert an asset similarly to [`Assets::add`].
    fn spawn_asset<A: Asset>(&mut self, asset: A) -> Handle<A>;
    /// Insert an asset similarly to [`Assets::add`].
    fn spawn_asset_with_uuid<A: Asset>(&mut self, uuid: Uuid, asset: impl Into<A>) -> Handle<A>;

    /// Reserves an asset handle of type `A`.
    fn reserve_asset_handle<A: Asset>(&mut self) -> Handle<A>;

    /// Gets an asset from its [`AssetId`].
    ///
    /// This function also accepts [`&Handle`].
    ///
    /// [`&Handle`]: Handle
    fn get_asset<A: Asset>(&self, id: impl Into<AssetId<A>>) -> Option<&A>;

    /// Gets an asset mutably from its [`AssetId`].
    ///
    /// This function also accepts [`&Handle`].
    ///
    /// [`&Handle`]: Handle
    fn get_asset_mut<A: Asset + Component<Mutability = Mutable>>(
        &mut self,
        id: impl Into<AssetId<A>>,
    ) -> Option<Mut<'_, A>>;

    /// Load an asset similarly to [`AssetServer::load`].
    fn load_asset<'a, A: Asset>(&self, path: impl Into<AssetReference<'a>>) -> Handle<A>;

    /// Creates a new [`LoadBuilder`] similar to [`AssetServer::load_builder`].
    fn load_builder(&self) -> LoadBuilder<'_>;

    /// Load an asset with settings, similarly to [`AssetServer::load_with_settings`].
    #[deprecated(note = "Use `world.load_builder().with_settings(settings).load(path)`")]
    fn load_asset_with_settings<'a, A: Asset, S: Settings>(
        &self,
        path: impl Into<AssetReference<'a>>,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
    ) -> Handle<A>;
}

impl DirectAssetAccessExt for World {
    /// Insert an asset similarly to [`Assets::add`].
    fn add_asset<'a, A: Asset>(&mut self, asset: impl Into<A>) -> Handle<A> {
        self.spawn_asset(asset.into())
    }

    /// Insert an asset similarly to [`Assets::add`].
    fn spawn_asset<'a, A: Asset>(&mut self, asset: A) -> Handle<A> {
        let entity_handle = self.spawn(asset).handle_with_data(AssetData::new::<A>());
        entity_handle.into()
    }

    fn spawn_asset_with_uuid<A: Asset>(&mut self, uuid: Uuid, asset: impl Into<A>) -> Handle<A> {
        let entity_handle = self.spawn(asset.into()).handle_with_data(AssetData {
            uuid: Some(uuid),
            ..AssetData::new::<A>()
        });
        entity_handle.into()
    }

    fn reserve_asset_handle<A: Asset>(&mut self) -> Handle<A> {
        self.spawn_empty()
            .handle_with_data(AssetData::new::<A>())
            .into()
    }

    fn get_asset<A: Asset>(&self, id: impl Into<AssetId<A>>) -> Option<&A> {
        self.get_entity(id.into().entity).ok()?.get::<A>()
    }

    fn get_asset_mut<A: Asset + Component<Mutability = Mutable>>(
        &mut self,
        id: impl Into<AssetId<A>>,
    ) -> Option<Mut<'_, A>> {
        self.get_entity_mut(id.into().entity).ok()?.into_mut::<A>()
    }

    /// Load an asset similarly to [`AssetServer::load`].
    ///
    /// # Panics
    /// If `self` doesn't have an [`AssetServer`] resource initialized yet.
    fn load_asset<'a, A: Asset>(&self, path: impl Into<AssetReference<'a>>) -> Handle<A> {
        self.resource::<AssetServer>().load(path)
    }

    /// Creates a new [`LoadBuilder`] similar to [`AssetServer::load_builder`].
    ///
    /// # Panics
    /// If `self` doesn't have an [`AssetServer`] resource initialized yet.
    fn load_builder(&self) -> LoadBuilder<'_> {
        self.resource::<AssetServer>().load_builder()
    }

    /// Load an asset with settings, similarly to [`AssetServer::load_with_settings`].
    ///
    /// # Panics
    /// If `self` doesn't have an [`AssetServer`] resource initialized yet.
    fn load_asset_with_settings<'a, A: Asset, S: Settings>(
        &self,
        path: impl Into<AssetReference<'a>>,
        settings: impl Fn(&mut S) + Send + Sync + 'static,
    ) -> Handle<A> {
        self.resource::<AssetServer>()
            .load_builder()
            .with_settings(settings)
            .load(path.into())
    }
}

pub trait AssetCommands {
    fn spawn_asset<A: Asset>(&mut self, asset: A) -> Handle<A>;
    fn reserve_handle<A: Asset>(&mut self) -> Handle<A>;
}

impl<'w, 's> AssetCommands for Commands<'w, 's> {
    fn spawn_asset<A: Asset>(&mut self, asset: A) -> Handle<A> {
        let entity_handle = self
            .entity_allocator()
            .alloc_handle_with_data(AssetData::new::<A>());
        let entity = entity_handle.0.entity;
        let weak = entity_handle.weak();
        self.queue(move |world: &mut World| {
            world.spawn_at(entity, (asset, weak)).unwrap();
        });
        entity_handle.into()
    }

    fn reserve_handle<A: Asset>(&mut self) -> Handle<A> {
        let entity_handle = self
            .entity_allocator()
            .alloc_handle_with_data(AssetData::new::<A>());
        self.spawn(entity_handle.weak());
        entity_handle.into()
    }
}
