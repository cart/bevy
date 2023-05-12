pub mod adapter;
pub mod io;
pub mod meta;
pub mod processor;
pub mod saver;

pub mod prelude {
    #[doc(hidden)]
    pub use crate::{
        Asset, AssetApp, AssetEvent, AssetId, AssetPlugin, AssetServer, Assets, Handle,
        UntypedHandle,
    };
}

mod assets;
mod event;
mod folder;
mod handle;
mod id;
mod loader;
mod path;
mod reflect;
mod server;

pub use assets::*;
pub use bevy_asset_macros::Asset;
pub use event::*;
pub use folder::*;
pub use futures_lite::{AsyncReadExt, AsyncWriteExt};
pub use handle::*;
pub use id::*;
pub use loader::*;
pub use path::*;
pub use reflect::*;
pub use server::*;

use crate::{
    io::{processor_gated::ProcessorGatedReader, AssetProvider, AssetProviders},
    processor::AssetProcessor,
};
use bevy_app::{App, AppTypeRegistry, Plugin, PostUpdate, Startup};
use bevy_ecs::{schedule::IntoSystemConfigs, world::FromWorld};
use bevy_log::error;
use bevy_reflect::{FromReflect, GetTypeRegistration, Reflect};
use std::{any::TypeId, sync::Arc};

pub enum AssetPlugin {
    Unprocessed {
        source: AssetProvider,
        watch_for_changes: bool,
    },
    Processed {
        destination: AssetProvider,
        watch_for_changes: bool,
    },
    ProcessedDev {
        source: AssetProvider,
        destination: AssetProvider,
        watch_for_changes: bool,
    },
}

impl Default for AssetPlugin {
    fn default() -> Self {
        Self::unprocessed()
    }
}

impl AssetPlugin {
    const DEFAULT_FILE_SOURCE: &str = "assets";
    /// NOTE: this is in the Default sub-folder to make this forward compatible with "import profiles"
    /// and to allow us to put the "processor transaction log" at `.imported_assets/log`
    const DEFAULT_FILE_DESTINATION: &str = ".imported_assets/Default";

    pub fn processed() -> Self {
        Self::Processed {
            destination: Default::default(),
            watch_for_changes: false,
        }
    }

    pub fn processed_dev() -> Self {
        Self::ProcessedDev {
            source: Default::default(),
            destination: Default::default(),
            watch_for_changes: true,
        }
    }

    pub fn unprocessed() -> Self {
        Self::Unprocessed {
            source: Default::default(),
            watch_for_changes: false,
        }
    }

    pub fn watch_for_changes(mut self) -> Self {
        error!("Watching for changes is not supported yet");
        match &mut self {
            AssetPlugin::Unprocessed {
                watch_for_changes, ..
            } => *watch_for_changes = true,
            AssetPlugin::Processed {
                watch_for_changes, ..
            } => *watch_for_changes = true,
            AssetPlugin::ProcessedDev {
                watch_for_changes, ..
            } => *watch_for_changes = true,
        };
        self
    }
}

impl Plugin for AssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetProviders>();
        {
            match self {
                AssetPlugin::Unprocessed {
                    source,
                    watch_for_changes,
                } => {
                    let source_reader = app
                        .world
                        .resource_mut::<AssetProviders>()
                        .get_source_reader(source);
                    app.insert_resource(AssetServer::new(source_reader, *watch_for_changes));
                }
                AssetPlugin::Processed {
                    destination,
                    watch_for_changes,
                } => {
                    let destination_reader = app
                        .world
                        .resource_mut::<AssetProviders>()
                        .get_destination_reader(destination);
                    app.insert_resource(AssetServer::new(destination_reader, *watch_for_changes));
                }
                AssetPlugin::ProcessedDev {
                    source,
                    destination,
                    watch_for_changes,
                } => {
                    let mut asset_providers = app.world.resource_mut::<AssetProviders>();
                    let processor = AssetProcessor::new(&mut *asset_providers, source, destination);
                    let destination_reader = asset_providers.get_destination_reader(source);
                    // the main asset server gates loads based on asset state
                    let gated_reader =
                        ProcessorGatedReader::new(destination_reader, processor.data.clone());
                    // the main asset server shares loaders with the processor asset server
                    app.insert_resource(AssetServer::new_with_loaders(
                        Box::new(gated_reader),
                        processor.server().data.loaders.clone(),
                        *watch_for_changes,
                    ))
                    .insert_resource(processor)
                    .add_systems(Startup, AssetProcessor::start);
                }
            }
        }
        app.init_asset::<LoadedFolder>()
            .add_systems(PostUpdate, server::handle_internal_asset_events);
    }
}

pub trait Asset: AssetDependencyVisitor + Send + Sync + 'static {}

pub trait AssetDependencyVisitor {
    // TODO: should this be an owned handle or can it be an UntypedAssetId
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedHandle));
}

impl<A: Asset> AssetDependencyVisitor for Handle<A> {
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedHandle)) {
        visit(self.clone().untyped())
    }
}

impl AssetDependencyVisitor for UntypedHandle {
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedHandle)) {
        visit(self.clone())
    }
}

impl<A: Asset> AssetDependencyVisitor for Vec<Handle<A>> {
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedHandle)) {
        for dependency in self.iter() {
            visit(dependency.clone().untyped())
        }
    }
}

impl AssetDependencyVisitor for Vec<UntypedHandle> {
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedHandle)) {
        for dependency in self.iter() {
            visit(dependency.clone())
        }
    }
}

pub trait AssetApp {
    fn register_asset_loader<L: AssetLoader>(&mut self, loader: L) -> &mut Self;
    fn init_asset_loader<L: AssetLoader + FromWorld>(&mut self) -> &mut Self;
    fn init_asset<A: Asset>(&mut self) -> &mut Self;
    /// Registers the asset type `T` using `[App::register]`,
    /// and adds [`ReflectAsset`] type data to `T` and [`ReflectHandle`] type data to [`Handle<T>`] in the type registry.
    ///
    /// This enables reflection code to access assets. For detailed information, see the docs on [`ReflectAsset`] and [`ReflectHandle`].
    fn register_asset_reflect<A>(&mut self) -> &mut Self
    where
        A: Asset + Reflect + FromReflect + GetTypeRegistration;
}

impl AssetApp for App {
    fn register_asset_loader<L: AssetLoader>(&mut self, loader: L) -> &mut Self {
        self.world.resource::<AssetServer>().register_loader(loader);
        self
    }

    fn init_asset_loader<L: AssetLoader + FromWorld>(&mut self) -> &mut Self {
        let loader = L::from_world(&mut self.world);
        self.register_asset_loader(loader)
    }

    fn init_asset<A: Asset>(&mut self) -> &mut Self {
        let assets = Assets::<A>::default();
        self.world.resource::<AssetServer>().register_asset(&assets);
        if self.world.contains_resource::<AssetProcessor>() {
            let processor = self.world.resource::<AssetProcessor>();
            // The processor should have its own handle provider separate from the Asset storage
            // to ensure the id spaces are entirely separate. Not _strictly_ necessary, but
            // desirable.
            processor
                .server()
                .register_handle_provider(AssetHandleProvider::new(
                    TypeId::of::<A>(),
                    Arc::new(AssetIndexAllocator::default()),
                ));
        }
        self.insert_resource(assets)
            .add_event::<AssetEvent<A>>()
            .register_type::<Handle<A>>()
            .add_systems(
                PostUpdate,
                Assets::<A>::track_assets.after(server::handle_internal_asset_events),
            )
    }

    fn register_asset_reflect<A>(&mut self) -> &mut Self
    where
        A: Asset + Reflect + FromReflect + GetTypeRegistration,
    {
        let type_registry = self.world.resource::<AppTypeRegistry>();
        {
            let mut type_registry = type_registry.write();

            type_registry.register::<A>();
            type_registry.register::<Handle<A>>();
            type_registry.register_type_data::<A, ReflectAsset>();
            type_registry.register_type_data::<Handle<A>, ReflectHandle>();
        }

        self
    }
}

#[macro_export]
macro_rules! load_internal_asset {
    ($app: ident, $handle: expr, $path_str: expr, $loader: expr) => {{
        let mut assets = $app.world.resource_mut::<$crate::Assets<_>>();
        assets.insert($handle, ($loader)(include_str!($path_str)));
    }};
}

#[macro_export]
macro_rules! load_internal_binary_asset {
    ($app: ident, $handle: expr, $path_str: expr, $loader: expr) => {{
        let mut assets = $app.world.resource_mut::<$crate::Assets<_>>();
        assets.insert($handle, ($loader)(include_bytes!($path_str).as_ref()));
    }};
}
