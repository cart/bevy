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
mod handle;
mod id;
mod folder;
mod loader;
mod path;
mod reflect;
mod server;

pub use assets::*;
pub use bevy_asset_macros::Asset;
pub use event::*;
pub use futures_lite::{AsyncReadExt, AsyncWriteExt};
pub use handle::*;
pub use id::*;
pub use loader::*;
pub use path::*;
pub use reflect::*;
pub use server::*;

use crate::{
    io::{file::FileAssetWriter, processor_gated::ProcessorGatedReader, AssetWriter},
    processor::{AssetProcessor, AssetProcessorPlugin},
};
use bevy_app::{App, AppTypeRegistry, Plugin, PostUpdate};
use bevy_ecs::{schedule::IntoSystemConfigs, system::Resource, world::FromWorld};
use bevy_log::error;
use bevy_reflect::{FromReflect, GetTypeRegistration, Reflect};
use bevy_utils::HashMap;
use io::{file::FileAssetReader, AssetReader};
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
    const DEFAULT_FILE_DESTINATION: &str = ".imported_assets";

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
            watch_for_changes: false,
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
                AssetPlugin::Unprocessed { source, .. } => {
                    let source_reader = app
                        .world
                        .resource_mut::<AssetProviders>()
                        .get_source_reader(source);
                    app.insert_resource(AssetServer::new(source_reader));
                }
                AssetPlugin::Processed { destination, .. } => {
                    let destination_reader = app
                        .world
                        .resource_mut::<AssetProviders>()
                        .get_destination_reader(destination);
                    app.insert_resource(AssetServer::new(destination_reader));
                }
                AssetPlugin::ProcessedDev {
                    source,
                    destination,
                    ..
                } => {
                    app.add_plugin(AssetProcessorPlugin {
                        source: source.clone(),
                        destination: destination.clone(),
                    });
                    let loaders = {
                        let processor = app.world.resource::<AssetProcessor>();
                        processor.assets().data.loaders.clone()
                    };
                    let processor = app.world.resource::<AssetProcessor>().clone();
                    let destination_reader = app
                        .world
                        .resource_mut::<AssetProviders>()
                        .get_destination_reader(source);
                    let gated_reader = ProcessorGatedReader::new(destination_reader, processor);
                    // the main asset server shares loaders with the processor asset server
                    app.insert_resource(AssetServer::new_with_loaders(
                        Box::new(gated_reader),
                        loaders,
                    ));
                }
            }
        }
        app.add_systems(PostUpdate, server::handle_internal_asset_events);
    }
}

pub trait Asset: Send + Sync + 'static {
    fn for_each_dependency(&self, process: impl FnMut(UntypedAssetId));
}

#[derive(Default, Clone, Debug)]
pub enum AssetProvider {
    #[default]
    Default,
    Custom(String),
}

#[derive(Resource, Default)]
pub struct AssetProviders {
    readers: HashMap<String, Box<dyn FnMut() -> Box<dyn AssetReader> + Send + Sync>>,
    writers: HashMap<String, Box<dyn FnMut() -> Box<dyn AssetWriter> + Send + Sync>>,
}

impl AssetProviders {
    pub fn with_reader(
        mut self,
        provider: &str,
        reader: impl FnMut() -> Box<dyn AssetReader> + Send + Sync + 'static,
    ) -> Self {
        self.readers.insert(provider.to_string(), Box::new(reader));
        self
    }

    pub fn with_writer(
        mut self,
        provider: &str,
        writer: impl FnMut() -> Box<dyn AssetWriter> + Send + Sync + 'static,
    ) -> Self {
        self.writers.insert(provider.to_string(), Box::new(writer));
        self
    }

    pub fn get_source_reader(&mut self, provider: &AssetProvider) -> Box<dyn AssetReader> {
        match provider {
            AssetProvider::Default => {
                Box::new(FileAssetReader::new(AssetPlugin::DEFAULT_FILE_SOURCE))
            }
            AssetProvider::Custom(provider) => {
                let get_reader = self
                    .readers
                    .get_mut(provider)
                    .unwrap_or_else(|| panic!("Asset Provider {} does not exist", provider));
                (get_reader)()
            }
        }
    }
    pub fn get_destination_reader(&mut self, provider: &AssetProvider) -> Box<dyn AssetReader> {
        match provider {
            AssetProvider::Default => {
                Box::new(FileAssetReader::new(AssetPlugin::DEFAULT_FILE_DESTINATION))
            }
            AssetProvider::Custom(provider) => {
                let get_reader = self
                    .readers
                    .get_mut(provider)
                    .unwrap_or_else(|| panic!("Asset Provider {} does not exist", provider));
                (get_reader)()
            }
        }
    }

    pub fn get_source_writer(&mut self, provider: &AssetProvider) -> Box<dyn AssetWriter> {
        match provider {
            AssetProvider::Default => {
                Box::new(FileAssetWriter::new(AssetPlugin::DEFAULT_FILE_SOURCE))
            }
            AssetProvider::Custom(provider) => {
                let get_writer = self
                    .writers
                    .get_mut(provider)
                    .unwrap_or_else(|| panic!("Asset Provider {} does not exist", provider));
                (get_writer)()
            }
        }
    }
    pub fn get_destination_writer(&mut self, provider: &AssetProvider) -> Box<dyn AssetWriter> {
        match provider {
            AssetProvider::Default => {
                Box::new(FileAssetWriter::new(AssetPlugin::DEFAULT_FILE_DESTINATION))
            }
            AssetProvider::Custom(provider) => {
                let get_writer = self
                    .writers
                    .get_mut(provider)
                    .unwrap_or_else(|| panic!("Asset Provider {} does not exist", provider));
                (get_writer)()
            }
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
                .assets()
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

#[cfg(test)]
mod tests {
    use crate as bevy_asset;
    use crate::{
        handle::Handle,
        io::{
            gated::{GateOpener, GatedReader},
            memory::{Dir, MemoryAssetReader},
            Reader,
        },
        loader::{AssetLoader, LoadContext},
        Asset, AssetApp, AssetDependencyLoadState, AssetEvent, AssetId, AssetLoadState,
        AssetPlugin, AssetProvider, AssetProviders, AssetRecursiveDependencyLoadState, AssetServer,
        Assets,
    };
    use bevy_app::{App, Update};
    use bevy_core::TaskPoolPlugin;
    use bevy_ecs::prelude::*;
    use bevy_log::LogPlugin;
    use bevy_utils::BoxedFuture;
    use futures_lite::AsyncReadExt;
    use serde::{Deserialize, Serialize};
    use std::path::Path;

    #[derive(Asset, Debug)]
    pub struct CoolText {
        text: String,
        embedded: String,
        dependencies: Vec<Handle<CoolText>>,
        sub_texts: Vec<Handle<SubText>>,
    }

    #[derive(Asset, Debug)]
    pub struct SubText {
        text: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct CoolTextRon {
        text: String,
        dependencies: Vec<String>,
        embedded_dependencies: Vec<String>,
        sub_texts: Vec<String>,
    }

    #[derive(Default)]
    struct CoolTextLoader;

    impl AssetLoader for CoolTextLoader {
        type Asset = CoolText;

        type Settings = ();

        fn load<'a>(
            &'a self,
            reader: &'a mut Reader,
            _settings: &'a Self::Settings,
            load_context: &'a mut LoadContext,
        ) -> BoxedFuture<'a, Result<Self::Asset, anyhow::Error>> {
            Box::pin(async move {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).await?;
                let mut ron: CoolTextRon = ron::de::from_bytes(&bytes)?;
                let mut embedded = String::new();
                for dep in ron.embedded_dependencies {
                    let loaded = load_context.load_direct_async(&dep).await?;
                    let cool = loaded.get::<CoolText>().unwrap();
                    embedded.push_str(&cool.text);
                }
                Ok(CoolText {
                    text: ron.text,
                    embedded,
                    dependencies: ron
                        .dependencies
                        .iter()
                        .map(|p| load_context.load(p))
                        .collect(),
                    sub_texts: ron
                        .sub_texts
                        .drain(..)
                        .map(|text| load_context.add_labeled_asset(text.clone(), SubText { text }))
                        .collect(),
                })
            })
        }

        fn extensions(&self) -> &[&str] {
            &["cool.ron"]
        }
    }

    fn test_app(dir: Dir) -> (App, GateOpener) {
        let mut app = App::new();
        let (gated_memory_reader, gate_opener) = GatedReader::new(MemoryAssetReader { root: dir });
        app.insert_resource(
            AssetProviders::default()
                .with_reader("Test", move || Box::new(gated_memory_reader.clone())),
        )
        .add_plugin(TaskPoolPlugin::default())
        .add_plugin(LogPlugin::default())
        .add_plugin(AssetPlugin::Unprocessed {
            source: AssetProvider::Custom("Test".to_string()),
            watch_for_changes: false,
        });
        (app, gate_opener)
    }

    fn run_app_until(app: &mut App, mut predicate: impl FnMut(&mut World) -> Option<()>) {
        for _ in 0..LARGE_ITERATION_COUNT {
            app.update();
            if (predicate)(&mut app.world).is_some() {
                return;
            }
        }

        panic!("Ran out of loops to return `Some` from `predicate`");
    }

    const LARGE_ITERATION_COUNT: usize = 5;

    fn get<'a, A: Asset>(world: &'a World, id: AssetId<A>) -> Option<&'a A> {
        world.resource::<Assets<A>>().get(id)
    }

    #[test]
    fn load_dependencies() {
        let dir = Dir::default();

        let a_path = "a.cool.ron";
        let a_ron = r#"
(
    text: "a",
    dependencies: [
        "foo/b.cool.ron",
        "c.cool.ron",
    ],
    embedded_dependencies: [],
    sub_texts: [],
)"#;
        let b_path = "foo/b.cool.ron";
        let b_ron = r#"
(
    text: "b",
    dependencies: [],
    embedded_dependencies: [],
    sub_texts: [],
)"#;

        let c_path = "c.cool.ron";
        let c_ron = r#"
(
    text: "c",
    dependencies: [
        "d.cool.ron",
    ],
    embedded_dependencies: ["a.cool.ron", "foo/b.cool.ron"],
    sub_texts: ["hello"],
)"#;

        let d_path = "d.cool.ron";
        let d_ron = r#"
(
    text: "d",
    dependencies: [],
    embedded_dependencies: [],
    sub_texts: [],
)"#;

        dir.insert_asset_text(Path::new(a_path), a_ron);
        dir.insert_asset_text(Path::new(b_path), b_ron);
        dir.insert_asset_text(Path::new(c_path), c_ron);
        dir.insert_asset_text(Path::new(d_path), d_ron);

        fn store_asset_events(
            mut reader: EventReader<AssetEvent<CoolText>>,
            mut storage: ResMut<StoredEvents>,
        ) {
            storage.0.extend(reader.iter().cloned());
        }

        #[derive(Resource, Default)]
        struct StoredEvents(Vec<AssetEvent<CoolText>>);

        #[derive(Resource)]
        struct IdResults {
            b_id: AssetId<CoolText>,
            c_id: AssetId<CoolText>,
            d_id: AssetId<CoolText>,
        }

        let (mut app, gate_opener) = test_app(dir);
        app.init_asset::<CoolText>()
            .init_asset::<SubText>()
            .init_resource::<StoredEvents>()
            .register_asset_loader(CoolTextLoader)
            .add_systems(Update, store_asset_events);
        let asset_server = app.world.resource::<AssetServer>().clone();
        let handle: Handle<CoolText> = asset_server.load(a_path);
        let a_id = handle.id();
        let entity = app.world.spawn(handle).id();
        app.update();
        {
            let a_text = get::<CoolText>(&app.world, a_id);
            let (a_load, a_deps, a_rec_deps) = asset_server.get_load_states(a_id).unwrap();
            assert!(a_text.is_none(), "a's asset should not exist yet");
            assert_eq!(a_load, AssetLoadState::Loading, "a should still be loading");
            assert_eq!(
                a_deps,
                AssetDependencyLoadState::Loading,
                "a deps should still be loading"
            );
            assert_eq!(
                a_rec_deps,
                AssetRecursiveDependencyLoadState::Loading,
                "a recursive deps should still be loading"
            );
        }

        // Allow "a" to load ... wait for it to finish loading and validate results
        // Dependencies are still gated so they should not be loaded yet
        gate_opener.open(a_path);
        run_app_until(&mut app, |world| {
            let a_text = get::<CoolText>(world, a_id)?;
            let (a_load, a_deps, a_rec_deps) = asset_server.get_load_states(a_id).unwrap();
            assert_eq!(a_text.text, "a");
            assert_eq!(a_text.dependencies.len(), 2);
            assert_eq!(a_load, AssetLoadState::Loaded, "a is loaded");
            assert_eq!(a_deps, AssetDependencyLoadState::Loading);
            assert_eq!(a_rec_deps, AssetRecursiveDependencyLoadState::Loading);

            let b_id = a_text.dependencies[0].id();
            let b_text = get::<CoolText>(world, b_id);
            let (b_load, b_deps, b_rec_deps) = asset_server.get_load_states(b_id).unwrap();
            assert!(b_text.is_none(), "b component should not exist yet");
            assert_eq!(b_load, AssetLoadState::Loading);
            assert_eq!(b_deps, AssetDependencyLoadState::Loading);
            assert_eq!(b_rec_deps, AssetRecursiveDependencyLoadState::Loading);

            let c_id = a_text.dependencies[1].id();
            let c_text = get::<CoolText>(world, c_id);
            let (c_load, c_deps, c_rec_deps) = asset_server.get_load_states(c_id).unwrap();
            assert!(c_text.is_none(), "c component should not exist yet");
            assert_eq!(c_load, AssetLoadState::Loading);
            assert_eq!(c_deps, AssetDependencyLoadState::Loading);
            assert_eq!(c_rec_deps, AssetRecursiveDependencyLoadState::Loading);
            Some(())
        });

        // Allow "b" to load ... wait for it to finish loading and validate results
        // "c" should not be loaded yet
        gate_opener.open(b_path);
        run_app_until(&mut app, |world| {
            let a_text = get::<CoolText>(world, a_id)?;
            let (a_load, a_deps, a_rec_deps) = asset_server.get_load_states(a_id).unwrap();
            assert_eq!(a_text.text, "a");
            assert_eq!(a_text.dependencies.len(), 2);
            assert_eq!(a_load, AssetLoadState::Loaded);
            assert_eq!(a_deps, AssetDependencyLoadState::Loading);
            assert_eq!(a_rec_deps, AssetRecursiveDependencyLoadState::Loading);

            let b_id = a_text.dependencies[0].id();
            let b_text = get::<CoolText>(world, b_id)?;
            let (b_load, b_deps, b_rec_deps) = asset_server.get_load_states(b_id).unwrap();
            assert_eq!(b_text.text, "b");
            assert_eq!(b_load, AssetLoadState::Loaded);
            assert_eq!(b_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(b_rec_deps, AssetRecursiveDependencyLoadState::Loaded);

            let c_id = a_text.dependencies[1].id();
            let c_text = get::<CoolText>(world, c_id);
            let (c_load, c_deps, c_rec_deps) = asset_server.get_load_states(c_id).unwrap();
            assert!(c_text.is_none(), "c component should not exist yet");
            assert_eq!(c_load, AssetLoadState::Loading);
            assert_eq!(c_deps, AssetDependencyLoadState::Loading);
            assert_eq!(c_rec_deps, AssetRecursiveDependencyLoadState::Loading);
            Some(())
        });

        // Allow "c" to load ... wait for it to finish loading and validate results
        // all "a" dependencies should be loaded now
        gate_opener.open(c_path);

        // Re-open a and b gates to allow c to load embedded deps (gates are closed after each load)
        gate_opener.open(a_path);
        gate_opener.open(b_path);
        run_app_until(&mut app, |world| {
            let a_text = get::<CoolText>(world, a_id)?;
            let (a_load, a_deps, a_rec_deps) = asset_server.get_load_states(a_id).unwrap();
            assert_eq!(a_text.text, "a");
            assert_eq!(a_text.embedded, "");
            assert_eq!(a_text.dependencies.len(), 2);
            assert_eq!(a_load, AssetLoadState::Loaded);

            let b_id = a_text.dependencies[0].id();
            let b_text = get::<CoolText>(world, b_id)?;
            let (b_load, b_deps, b_rec_deps) = asset_server.get_load_states(b_id).unwrap();
            assert_eq!(b_text.text, "b");
            assert_eq!(b_text.embedded, "");
            assert_eq!(b_load, AssetLoadState::Loaded);
            assert_eq!(b_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(b_rec_deps, AssetRecursiveDependencyLoadState::Loaded);

            let c_id = a_text.dependencies[1].id();
            let c_text = get::<CoolText>(world, c_id)?;
            let (c_load, c_deps, c_rec_deps) = asset_server.get_load_states(c_id).unwrap();
            assert_eq!(c_text.text, "c");
            assert_eq!(c_text.embedded, "ab");
            assert_eq!(c_load, AssetLoadState::Loaded);
            assert_eq!(
                c_deps,
                AssetDependencyLoadState::Loading,
                "c deps should not be loaded yet because d has not loaded"
            );
            assert_eq!(
                c_rec_deps,
                AssetRecursiveDependencyLoadState::Loading,
                "c rec deps should not be loaded yet because d has not loaded"
            );

            let sub_text_id = c_text.sub_texts[0].id();
            let sub_text = get::<SubText>(world, sub_text_id)
                .expect("subtext should exist if c exists. it came from the same loader");
            assert_eq!(sub_text.text, "hello");
            let (sub_text_load, sub_text_deps, sub_text_rec_deps) =
                asset_server.get_load_states(sub_text_id).unwrap();
            assert_eq!(sub_text_load, AssetLoadState::Loaded);
            assert_eq!(sub_text_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(sub_text_rec_deps, AssetRecursiveDependencyLoadState::Loaded);

            let d_id = c_text.dependencies[0].id();
            let d_text = get::<CoolText>(world, d_id);
            let (d_load, d_deps, d_rec_deps) = asset_server.get_load_states(d_id).unwrap();
            assert!(d_text.is_none(), "d component should not exist yet");
            assert_eq!(d_load, AssetLoadState::Loading);
            assert_eq!(d_deps, AssetDependencyLoadState::Loading);
            assert_eq!(d_rec_deps, AssetRecursiveDependencyLoadState::Loading);

            assert_eq!(
                a_deps,
                AssetDependencyLoadState::Loaded,
                "If c has been loaded, the a deps should all be considered loaded"
            );
            assert_eq!(
                a_rec_deps,
                AssetRecursiveDependencyLoadState::Loading,
                "d is not loaded, so a's recursive deps should still be loading"
            );
            world.insert_resource(IdResults { b_id, c_id, d_id });
            Some(())
        });

        gate_opener.open(d_path);
        run_app_until(&mut app, |world| {
            let a_text = get::<CoolText>(world, a_id)?;
            let (_a_load, _a_deps, a_rec_deps) = asset_server.get_load_states(a_id).unwrap();
            let c_id = a_text.dependencies[1].id();
            let c_text = get::<CoolText>(world, c_id)?;
            let (c_load, c_deps, c_rec_deps) = asset_server.get_load_states(c_id).unwrap();
            assert_eq!(c_text.text, "c");
            assert_eq!(c_text.embedded, "ab");
            assert_eq!(c_load, AssetLoadState::Loaded);
            assert_eq!(c_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(c_rec_deps, AssetRecursiveDependencyLoadState::Loaded);

            let d_id = c_text.dependencies[0].id();
            let d_text = get::<CoolText>(world, d_id)?;
            let (d_load, d_deps, d_rec_deps) = asset_server.get_load_states(d_id).unwrap();
            assert_eq!(d_text.text, "d");
            assert_eq!(d_text.embedded, "");
            assert_eq!(d_load, AssetLoadState::Loaded);
            assert_eq!(d_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(d_rec_deps, AssetRecursiveDependencyLoadState::Loaded);

            assert_eq!(
                a_rec_deps,
                AssetRecursiveDependencyLoadState::Loaded,
                "d is loaded, so a's recursive deps should be loaded"
            );
            Some(())
        });

        {
            let mut texts = app.world.resource_mut::<Assets<CoolText>>();
            let a = texts.get_mut(a_id).unwrap();
            a.text = "Changed".to_string();
        }

        app.world.despawn(entity);
        app.update();
        assert_eq!(
            app.world.resource::<Assets<CoolText>>().len(),
            0,
            "CoolText asset entities should be despawned when no more handles exist"
        );
        app.update();
        // this requires a second update because the parent asset was freed in the previous app.update()
        assert_eq!(
            app.world.resource::<Assets<SubText>>().len(),
            0,
            "SubText asset entities should be despawned when no more handles exist"
        );
        let events = app.world.remove_resource::<StoredEvents>().unwrap();
        let id_results = app.world.remove_resource::<IdResults>().unwrap();
        let expected_events = vec![
            AssetEvent::Added { id: a_id },
            AssetEvent::Added {
                id: id_results.b_id,
            },
            AssetEvent::Added {
                id: id_results.c_id,
            },
            AssetEvent::Added {
                id: id_results.d_id,
            },
            AssetEvent::Modified { id: a_id },
            AssetEvent::Removed { id: a_id },
            AssetEvent::Removed {
                id: id_results.b_id,
            },
            AssetEvent::Removed {
                id: id_results.c_id,
            },
            AssetEvent::Removed {
                id: id_results.d_id,
            },
        ];
        assert_eq!(events.0, expected_events);
    }

    #[test]
    fn failure_load_states() {
        let dir = Dir::default();

        let a_path = "a.cool.ron";
        let a_ron = r#"
(
    text: "a",
    dependencies: [
        "b.cool.ron",
        "c.cool.ron",
    ],
    embedded_dependencies: [],
    sub_texts: []
)"#;
        let b_path = "b.cool.ron";
        let b_ron = r#"
(
    text: "b",
    dependencies: [],
    embedded_dependencies: [],
    sub_texts: []
)"#;

        let c_path = "c.cool.ron";
        let c_ron = r#"
(
    text: "c",
    dependencies: [
        "d.cool.ron",
    ],
    embedded_dependencies: [],
    sub_texts: []
)"#;

        let d_path = "d.cool.ron";
        let d_ron = r#"
(
    text: "d",
    dependencies: [],
    OH NO THIS ASSET IS MALFORMED
    embedded_dependencies: [],
    sub_texts: []
)"#;

        dir.insert_asset_text(Path::new(a_path), a_ron);
        dir.insert_asset_text(Path::new(b_path), b_ron);
        dir.insert_asset_text(Path::new(c_path), c_ron);
        dir.insert_asset_text(Path::new(d_path), d_ron);

        let (mut app, gate_opener) = test_app(dir);
        app.init_asset::<CoolText>()
            .register_asset_loader(CoolTextLoader);
        let asset_server = app.world.resource::<AssetServer>().clone();
        let handle: Handle<CoolText> = asset_server.load(a_path);
        let a_id = handle.id();
        {
            let other_handle: Handle<CoolText> = asset_server.load(a_path);
            assert_eq!(
                other_handle, handle,
                "handles from consecutive load calls should be equal"
            );
            assert_eq!(
                other_handle.id(),
                handle.id(),
                "handle ids from consecutive load calls should be equal"
            );
        }

        app.world.spawn(handle);
        gate_opener.open(a_path);
        gate_opener.open(b_path);
        gate_opener.open(c_path);
        gate_opener.open(d_path);

        run_app_until(&mut app, |world| {
            let a_text = get::<CoolText>(world, a_id)?;
            let (a_load, a_deps, a_rec_deps) = asset_server.get_load_states(a_id).unwrap();

            let b_id = a_text.dependencies[0].id();
            let b_text = get::<CoolText>(world, b_id)?;
            let (b_load, b_deps, b_rec_deps) = asset_server.get_load_states(b_id).unwrap();

            let c_id = a_text.dependencies[1].id();
            let c_text = get::<CoolText>(world, c_id)?;
            let (c_load, c_deps, c_rec_deps) = asset_server.get_load_states(c_id).unwrap();

            let d_id = c_text.dependencies[0].id();
            let d_text = get::<CoolText>(world, d_id);
            let (d_load, d_deps, d_rec_deps) = asset_server.get_load_states(d_id).unwrap();
            if d_load != AssetLoadState::Failed {
                // wait until d has exited the loading state
                return None;
            }

            assert!(d_text.is_none());
            assert_eq!(d_load, AssetLoadState::Failed);
            assert_eq!(d_deps, AssetDependencyLoadState::Failed);
            assert_eq!(d_rec_deps, AssetRecursiveDependencyLoadState::Failed);

            assert_eq!(a_text.text, "a");
            assert_eq!(a_load, AssetLoadState::Loaded);
            assert_eq!(a_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(a_rec_deps, AssetRecursiveDependencyLoadState::Failed);

            assert_eq!(b_text.text, "b");
            assert_eq!(b_load, AssetLoadState::Loaded);
            assert_eq!(b_deps, AssetDependencyLoadState::Loaded);
            assert_eq!(b_rec_deps, AssetRecursiveDependencyLoadState::Loaded);

            assert_eq!(c_text.text, "c");
            assert_eq!(c_load, AssetLoadState::Loaded);
            assert_eq!(c_deps, AssetDependencyLoadState::Failed);
            assert_eq!(c_rec_deps, AssetRecursiveDependencyLoadState::Failed);

            Some(())
        });
    }

    #[test]
    fn manual_asset_management() {
        let mut app = App::new();
        app.add_plugin(AssetPlugin::unprocessed())
            .init_asset::<CoolText>();
        let hello = "hello".to_string();
        let empty = "".to_string();

        let id = {
            let handle = {
                let mut texts = app.world.resource_mut::<Assets<CoolText>>();
                texts.add(CoolText {
                    text: hello.clone(),
                    embedded: empty.clone(),
                    dependencies: Vec::new(),
                    sub_texts: Vec::new(),
                })
            };

            app.update();

            {
                let text = app
                    .world
                    .resource::<Assets<CoolText>>()
                    .get(&handle)
                    .unwrap();
                assert_eq!(text.text, hello);
            }
            handle.id()
        };
        // handle is dropped
        app.update();
        assert!(
            app.world.resource::<Assets<CoolText>>().get(id).is_none(),
            "asset has no handles, so it should have been dropped last update"
        );
    }
}
