use crate::{
    update_asset_storage_system, Asset, AssetDynamic, AssetLoader, AssetSerializer, AssetServer,
    Handle, HandleId, RefChange,
};
use bevy_app::{prelude::Events, AppBuilder};
use bevy_ecs::{FromResources, IntoQuerySystem, ResMut};
use bevy_type_registry::RegisterType;
use bevy_utils::HashMap;
use crossbeam_channel::Sender;
use std::fmt::Debug;

/// Events that happen on assets of type `T`
pub enum AssetEvent<T: Asset> {
    Created { handle: Handle<T> },
    Modified { handle: Handle<T> },
    Removed { handle: Handle<T> },
}

impl<T: Asset> Debug for AssetEvent<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetEvent::Created { handle } => f
                .debug_struct(&format!(
                    "AssetEvent<{}>::Created",
                    std::any::type_name::<T>()
                ))
                .field("handle", &handle.id)
                .finish(),
            AssetEvent::Modified { handle } => f
                .debug_struct(&format!(
                    "AssetEvent<{}>::Modified",
                    std::any::type_name::<T>()
                ))
                .field("handle", &handle.id)
                .finish(),
            AssetEvent::Removed { handle } => f
                .debug_struct(&format!(
                    "AssetEvent<{}>::Removed",
                    std::any::type_name::<T>()
                ))
                .field("handle", &handle.id)
                .finish(),
        }
    }
}

/// Stores Assets of a given type and tracks changes to them.
pub struct Assets<T: Asset> {
    assets: HashMap<HandleId, T>,
    ref_counts: HashMap<HandleId, usize>,
    events: Events<AssetEvent<T>>,
    pub(crate) ref_change_sender: Sender<RefChange>,
}

impl<T: Asset> Assets<T> {
    pub(crate) fn new(ref_change_sender: Sender<RefChange>) -> Self {
        Assets {
            assets: HashMap::default(),
            ref_counts: HashMap::default(),
            events: Events::default(),
            ref_change_sender,
        }
    }

    pub fn add(&mut self, asset: T) -> Handle<T> {
        let id = HandleId::random::<T>();
        self.assets.insert(id, asset);
        self.events.send(AssetEvent::Created {
            handle: Handle::weak(id),
        });
        self.get_handle(id)
    }

    pub fn set<H: Into<HandleId>>(&mut self, handle: H, asset: T) -> Handle<T> {
        let id: HandleId = handle.into();
        if self.assets.insert(id, asset).is_some() {
            self.events.send(AssetEvent::Modified {
                handle: Handle::weak(id),
            });
        } else {
            self.events.send(AssetEvent::Created {
                handle: Handle::weak(id),
            });
        }

        self.get_handle(id)
    }

    pub fn set_untracked<H: Into<HandleId>>(&mut self, handle: H, asset: T) {
        let id: HandleId = handle.into();
        if self.assets.insert(id, asset).is_some() {
            self.events.send(AssetEvent::Modified {
                handle: Handle::weak(id),
            });
        } else {
            self.events.send(AssetEvent::Created {
                handle: Handle::weak(id),
            });
        }
    }

    pub fn get<H: Into<HandleId>>(&self, handle: H) -> Option<&T> {
        self.assets.get(&handle.into())
    }

    pub fn get_mut<H: Into<HandleId>>(&mut self, handle: H) -> Option<&mut T> {
        let id: HandleId = handle.into();
        self.events.send(AssetEvent::Modified {
            handle: Handle::weak(id),
        });
        self.assets.get_mut(&id)
    }

    pub fn get_handle<H: Into<HandleId>>(&self, handle: H) -> Handle<T> {
        Handle::strong(handle.into(), self.ref_change_sender.clone())
    }

    pub fn get_or_insert_with<H: Into<HandleId>>(
        &mut self,
        handle: H,
        insert_fn: impl FnOnce() -> T,
    ) -> &mut T {
        let mut event = None;
        let id: HandleId = handle.into();
        let borrowed = self.assets.entry(id).or_insert_with(|| {
            event = Some(AssetEvent::Created {
                handle: Handle::weak(id),
            });
            insert_fn()
        });

        if let Some(event) = event {
            self.events.send(event);
        }
        borrowed
    }

    pub fn iter(&self) -> impl Iterator<Item = (HandleId, &T)> {
        self.assets.iter().map(|(k, v)| (*k, v))
    }

    pub fn ids<'a>(&'a self) -> impl Iterator<Item = HandleId> + 'a {
        self.assets.keys().cloned()
    }

    pub fn remove<H: Into<HandleId>>(&mut self, handle: H) -> Option<T> {
        let id: HandleId = handle.into();
        if let Some(ref_count) = self.ref_counts.remove(&id) {
            // TODO: we cant remove assets when there are active handles. sort out the right way to handle this
            if ref_count != 0 {
                debug_assert!(
                    false,
                    "Attempted to remove an asset when there were still active handles"
                );
                self.ref_counts.insert(id, ref_count);
                return None;
            }
        }
        println!("removed {} {:?}", std::any::type_name::<T>(), id);
        self.events.send(AssetEvent::Removed {
            handle: Handle::weak(id),
        });
        self.assets.remove(&id)
    }

    pub fn asset_event_system(
        mut events: ResMut<Events<AssetEvent<T>>>,
        mut assets: ResMut<Assets<T>>,
    ) {
        events.extend(assets.events.drain())
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}

/// [AppBuilder] extension methods for adding new asset types
pub trait AddAsset {
    fn add_asset<T>(&mut self) -> &mut Self
    where
        T: Asset + AssetDynamic;
    fn init_asset_loader<T>(&mut self) -> &mut Self
    where
        T: AssetLoader + FromResources;
    fn add_asset_loader<T>(&mut self, loader: T) -> &mut Self
    where
        T: AssetLoader;
    fn add_asset_serializer<T>(&mut self, serializer: T) -> &mut Self
    where
        T: AssetSerializer;
    fn init_asset_serializer<T>(&mut self) -> &mut Self
    where
        T: AssetSerializer + FromResources;
}

impl AddAsset for AppBuilder {
    fn add_asset<T>(&mut self) -> &mut Self
    where
        T: Asset + AssetDynamic,
    {
        let assets = {
            let asset_server = self.resources().get::<AssetServer>().unwrap();
            asset_server.register_asset_type::<T>()
        };

        self.add_resource(assets)
            .register_component::<Handle<T>>()
            .add_system_to_stage(
                super::stage::ASSET_EVENTS,
                Assets::<T>::asset_event_system.system(),
            )
            .add_system_to_stage(
                crate::stage::LOAD_ASSETS,
                update_asset_storage_system::<T>.system(),
            )
            .add_event::<AssetEvent<T>>()
    }

    fn init_asset_loader<T>(&mut self) -> &mut Self
    where
        T: AssetLoader + FromResources,
    {
        self.add_asset_loader(T::from_resources(self.resources()))
    }

    fn add_asset_loader<T>(&mut self, loader: T) -> &mut Self
    where
        T: AssetLoader,
    {
        self.resources()
            .get_mut::<AssetServer>()
            .expect("AssetServer does not exist. Consider adding it as a resource.")
            .add_loader(loader);
        self
    }

    fn add_asset_serializer<T>(&mut self, serializer: T) -> &mut Self
    where
        T: AssetSerializer,
    {
        self.resources()
            .get_mut::<AssetServer>()
            .expect("AssetServer does not exist. Consider adding it as a resource.")
            .add_serializer(serializer);
        self
    }

    fn init_asset_serializer<T>(&mut self) -> &mut Self
    where
        T: AssetSerializer + FromResources,
    {
        self.add_asset_serializer(T::from_resources(self.resources()))
    }
}
