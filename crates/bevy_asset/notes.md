## Requirements

* **Asset Preprocessing**: it should be possible to "preprocess" / "compile" / "crunch" assets at "development time" rather than when the game starts up. This enables offloading expensive work from deployed apps, faster asset loading, less runtime memory usage, etc.  
* **Asset `.meta` files**:  asset configuration files stored adjacent to the asset in question, which both assigns them a unique and stable asset id and allows the user to configure asset-type-specific settings. These settings should be accessible during the pre-processing phase. Modifying a `.meta` file should trigger a re-processing / re-load of the asset. It should be possible to configure asset loaders from the meta file.
* **Asset Dependencies**: assets should be able to define dependencies on other assets. this might be split out into "hard" and "soft" dependencies, where "hard" dependencies must be available during pre-processing and invalidate parent assets when they change. And "soft" dependencies are just loaded when the parent is loaded (at runtime).  
    * Do we need 3 types? Available during preprocessing, available at runtime (dep blocks parent ready state), not available at runtime (dep does not block parent ready state, must be manually loaded)
* **Runtime Asset Loading**: it should be (optionally) possible to load arbitrary assets dynamically at runtime. This necessitates being able to deploy and run the asset server alongside Bevy Apps on _all platforms_. For example, we should be able to invoke the shader compiler at runtime, stream scenes from sources like the internet, etc. To keep deployed binaries (and startup times) small, the runtime asset server configuration should be configurable with different settings compared to the "pre processor asset server".
* **Multiple Backends**: It would be nice if we could load assets from a variety of sources (filesystems, the internet, remote asset servers, etc).
    * Ideally this is a "pluggable interface / trait"
* **Asset Packing**: store compressed groups of processed assets, ideally multiple configurable groups
    * Prior Art: Unity Asset Bundles, Distill packs
* **Asset Handoff**: It should be possible to hold a "live" asset handle, which correlates to runtime data, without actually holding the asset in memory. Ex: it must be possible to hold a reference to a GPU mesh generated from a "mesh asset" without keeping the mesh data in CPU memory 
* **Hot Reloading**: Changes to assets should result in re-processing and re-loading

## High Level Structure

* Inputs
    * Raw asset files
    * .meta files
        * Unique ID
        * Hash
        * Asset Loader ID
        * Config
        * Asset References?
* Outputs
    * "Processed" Asset Files, in byte form
    * .meta files, if they dont exist
    * Metadata database: "asset graph"
* Architecture
    * AssetLoader: reads bytes of a given type into memory (deserializer)
    * AssetProcessor: reads bytes of a given type and produces bytes 
    * AssetServer: loads assets, processes assets
        * AssetStorage: filesystem, db, network
            * path->{bytes, metadata} interface
            * also handles queries?
* Asset Lifecycle
    * Asset is loaded 

## Open Questions

* How are assets stored and referenced? Is the `Assets<T>` collection still a good idea? Should handles have an `Arc<T>`?
* Are assets just entities? This would allow inline assets in scenes to piggyback on the existing impl
    * This could facilitate "unloading" and associating new context with handles while keeping the core reference alive
```rust
#[derive(Component)]
struct Asset<T: Asset> {
    value: Option<Arc<T>>,
}
```
* How are "runtime" assets created / how will we track their dependencies? Do we need to?
    * `let strong_handle = asset_server.add(SomeMesh)`?`
* Are assets "just" refcounted entities? AssetHandle<T>(EntityHandle<T>)
    * This would mean asset spawning would need access to entity collection. Either `&Assets` param or piggyback on `&mut Commands` with `commands.load("asset_path")`. Give that asset loading uses state, `&Assets` is probably the only solution
* Are assets mutable? If so, how?
    * query.get(asset_entity).mutate()? asset_server.queue_change(handle, value)?
    * entity-driven change detection will be more expensive
* How should asset metadata be stored on disk?
    * lmdb, sled, SQLite, custom
* Should processors be chainable (ex: a->b->c)
* Images vs textures
    * Do prefabs have Handle<Image> (just one of many ways to represent an image on the CPU) or Handle<Texture> (GPU texture)
* Renderer Assets vs App Assets
    * How does this work? Copying over is bad
* Migrations:
    * Done with bevy reflect / patching
        * Prove this would work in practice. Bevy Reflect is relatively unconstrained. What type of "unrecoverable breakage" can occur?
    * Ties in to Undo/Redo ... reflect-property-based-edit-history? 
* Platform-specific processed assets:
    * Need to be able to produce per-platform outputs (ex: optimize textures for mobile)
* Are unique ids required? If we use paths as the canonical id, everything can be lazy / we don't need a global view outside of the filesystem
* Support loading assets as a given type using their type as a hint (ex: `let x: Handle<Texture> = assets.load("asset.png")` vs `let x: Handle<Image> = asset.load("asset.png")` )
* Asset streaming (ex: "stream audio while it is playing")
    * Break asset up into "pieces" which can be loaded on demand, with default "pieces"
* "broken paths" and how to fix them
    * 3rd party formats (gltf): authored externally, should almost certainly be fixed in original tool?
        * _could_ fix as part of config, given that it will be converted to normal engine systems
    * built in formats: identify broken path errors and run editor tool to fix them

## Scenarios

### Bevy Scene

* Scenes will have hard dependencies on other scenes, meshes, textures etc
    * When a scene is considered "loaded" by the app, all meshes / textures / subscenes should be "loaded"
    * When all instances of a scene are unloaded, all meshes / textures / subscenes should be unloaded
* Scenes should be (optionally) processed into efficient, easy to load binary formats
* Scene Loading
    * AssetLoader reads bytes into Bevy Reflect-backed scene format (Things with Box<dyn Reflect>)
    * Scene contains "local" entity ids and strong asset paths / UUIDs

### Shaders

* Shader is defined in "Bevy WGSL", which includes imports
* Dependencies are resolved during initial WGSL load
* WGSL (and deps) are compiled into SPIRV
* This can happen ahead of time or at runtime

### Prefabs

* Prefabs will consist of weak handles to assets that are upgraded to strong handles on spawn
    * WeakHandle in the prefab instance itself. Handle on the entity components spawned from it
    * In code, users will kick off asset loads

### Asset Processing

* Loaders: Load(Source) -> InMemory
* Savers: Save(InMemory) -> Destination (which can be a new source)

* Preprocessor has AssetServer configured to Load, Process, and Save source folder to destination folder
* Game AssetServer configured to load from destination folder

#### How to handle import metadata

* Comes down to two main options:
    * Database
        * Pros
            * Transactional
            * Efficient
            * _Potentially_ faster startup: no need to scan a bunch of meta files. Just mmap the db and run the query on the info you need
        * Cons
            * Requires pulling in a database dependency
                * Anything "efficient" is going to have compatibility issues due to things like mmap (true for lmdb, sqlite, and sled)
            * Hides processed outputs
                * Outputs are stored inside the db. Users need special tools to query the database and inspect outputs
    * Filesystem
        * Prior art: Godot, Unity? (they also have file system asset artifacts)
        * Pros:
            * Cross platform: easily support any platform (including wasm) via storage traits
            * No new dependencies: faster compiles, easier builds
            * Easy to inspect and debug processed outputs, as they are just files in the filesystem
        * Cons:
            * Harder to maintain consistency. Filesystems aren't transactional.
            * Potentially slower startup: need to scan a bunch of meta files to build up metadata.
* Progressive processing:
    * How to run a game while assets are still being processed?
        * `Assets` could gate asset loads on processed state
* Is Assets the owner of all processor / reader / writer config?
    * Separate floating thread that owns an Assets reference
    * Block load on processing?
* File system modification time to reduce cost of scans  
* Filesystems
    * Add a `.import` folder (to be ignored in .gitignore)
        * Is this _not_ the final deployed asset folder?
            * Even if it isn't, _can_ it be for dev workflows? (answer should almost certainly be yes ... actual "deployments" take time)
        * Structure
            * `.imported_assets`
                * profiles
                    * `Default`
                        * foo.png
                    * `Windows`
                        * foo.png
                    * `Xbox`
            * .deployed_assets (fully processed folders)
    * How to handle per-platform asset logic
        * "import profiles"
            * Default: the default configuration, shared across all configurations
            * NamedProfiles (layered on top of Default):
                * Specific versions of processed assets 
        * Asset Deployments
            * Ordered list of named profiles, projected on top of each other
            * "pack" information ... how to "pack" assets
            * Can have "manifests" with baked info (ex: indices) 
* Modes
    * Direct
        *  Load unprocessed assets. Do processing directly if configured
    * Processed-Dev
        * Run processor in background. Treat `.import` as source, but block loads on processor state
    * On-demand/as-constant process: run processor in background, listen for changes, keep things up to date
        * If this is fully decoupled from games (ex: running in the cli), how do we block on processing properly?
            * Assuming in-process for now is probably ok: no editor, when one exists can block game startup on processing then hand off to game during execution?
            * Not really a problem: if we later find we need cross process signaling we can definitely build that in if needed
    * Deployed (same as direct?)


### Sprites

* Sprite textures should be processed into efficient, easy to load (generally gpu friendly) formats
* The default filter for a sprite texture should be configurable in the meta file

## Dependencies

* There could be various types:
    * Soft: value not a part of the loader, load _starts_ when the asset loads, which might have a handle to it. If the dependency is changed, we don't need to reload the asset.
    * Hard: value a part of the loader, load is _finished_ when the asset loads. If the dependency is changed we do not need to reload the asset.
    * Loader: bytes are loaded as part of the loader. If the dependency changes, we must re-run the loader / reprocess the asset.

### Asset Packs

* What is the interface for this?
    * Folders of assets to be packed? A `pack: Name` field?
    * Assets::load_pack("path_to_pack")?
    * `AssetPlugin { "packs_to_autoload" }`
* Are packs just another asset?
    * Pro: users could own handles to the packs and drop them whenever
    * Con: Packs are "special" / need to store data outside of the ECS / need to inform other asset loads

### Assets as Entities ... Manually Created vs Managed by Assets

* Manually created: ... by default no dependency tracking. No AssetLoadState, DependencyLoadState, etc
* Embedded Assets in scenes?
    * Things like StandardMaterials
* This ties into events (see below)
    * We need a single "asset event" feed for things like Mesh -> GPU loading.
        * This works for Changed/Removed/Added queries
        * This does not work (by default) for dependency tracking
* Solutions:
    * Require asset creation to go through Assets. This could then ensure eventing is consistent
        * Still doesn't handle dependency management ... no way to enumerate dependencies in runtime asset
            * In theory maybe this is fine? 
    * New change tracking mode: `ComponentEvent<T>::{Changed(Entity), Removed(Entity), Added(Entity)}`

* Assets as Entities has a ton of caveats right now:
    * Cross thread entity allocation
    * Asset change tracking + events
    * Reconciling manually created assets with "asset system" assets
    * Default entity handles
* It is time to punt this!

* How will we handle "static" / built in Assets. Can't use run time ids :/
    * AssetReference { Id(AssetId), Path(AssetPath)} 
    * "dual mode" storage: fast/sparseset for server-managed (or auto-managed) ids. hashmap for static ids (uuid?)
        * `Handle::AssetId(StrongAssetHandle)` and `Handle::Uuid(Uuid)`
        * Alternatively, reserve the "last" generation for "random ids"

### Events

* Almost certainly want analog to current `AssetEvent<T>`. Maybe also a non-generic `AssetEvent`?
* Assets _are_ entities ... how do we reconcile change events with that?
    * We need one trustable and ergonomic event feed for both "loaded" assets and "manually created" assets
        * Added/Changed/Removed ECS events? ..
            * Less efficient ... need to iterate entire list to find changes
            * Does not account for dependency/recursive loads ... would need to be handled separately
            * Only way to track direct changes?
        * AssetEvent<T> resource?
            * Cannot be auto-populated for custom entities.
                * feed it using ECS change events ... still suboptimal
        * Crossbeam channel + Wrapper type?
            * Non-standard, weird. Needs to be "bootstrapped".
        * `Asset<T>` wrapper with crossbeam channel
            * Internal load state that feeds events?
            * Harder to mutate?

### Component init with Asset Handles

* Weak handles?
    * This has direct parallels with the RenderTarget::Window(WindowRef::Primary) problem. Maybe there is a generic solution?
        * Default entities for a given component
            * `world.get_default::<Mesh>()`. `Handle<Mesh>::Default` could then resolve to the default value
* Handle::Default?
    * Could no longer be used as direct entity references

### Currently On My Mind

* Combine meta + asset bytes into a single file for imported assets
    * this makes loading them transactional

## TODO

### MVP

* Hot Reloading
    * dependency aware hotreloading
        * needs to store full dep info in meta
        * this only matters for unprocessed load_deps, processor handles this for us by modifying load dep dependants
    * Processor 
        * Removed event
            * Sometimes a rename::From event (need to add "rename" start/stop events)
        * Rename event
* Do we need to add "file locking" for processed folder (sounds like yes ... this might also play into recovery? if we crash with an active lock, that means that asset was not fully written)
    * For a given processor loop run, a dependent won't try to read until processing for that item has finished, so this is safe
    * For a given asset load, a read won't happen until there is already a valid processed item
    * _However_ this might happen
        1. load starts, asset already processed, so we go through the gate
        2. processor detects hot-reload and kicks off asset process, write begins (but doesn't finish)
        3. load read begins, which reads some (but not all) of the written bytes in (2)
    * After first process, gates are always open (because they have a Processed state)
        * Multiple parallel hot-reloaded dependent processings could then result in reading bad bytes
    * Therefore, processed reader/writer should atomically lock files
    * Combine read lock and "wait for process" action?
* Crash recovery
    * Log
        * If last entry in log on startup is _not_ a Finished action, clean up entries
        * Maybe have global start/stop too to detect if there was not a clean exit. Is this necessary/useful?
    * How do we maintain integrity of processed folder in event of a crash at arbitrary times?
    * Unity uses finally blocks ... do we use catch_unwind?
    * Short term, use simple "did we crash" detection combined with a full reprocess?
* Proprely impl Reflect and FromReflect for Handle. Make sure it can be used in Bevy Scenes
* Final pass over todo! and TODO / PERF
* Asset dependency derive
    * LoadedFolder needs this
    * Wire up Asset::visit_dependencies to LoadedAsset
* Should we combine meta + asset byte loading apis?
    * Single "lock" transaction
    * Would mean we aren't streaming bytes anymore?
    * Externalize "locking" outside Reader api?
* Might want to gate dep count increments / decrements on hashset ops (or just use hashsets). Otherwise reloading a dep could affect load correctness. 
    * see next point as this probably relates
* Events
    * Send "dependency reloaded" events
        * current dependants_waiting_on_load doesn't enable this, need to retain the whole list
            * notably, freeing an asset would remove all "dependants" info, and reloading would result in an empty list  
    * Send recursive dependencies loaded event

```rust
#[derive(Resource, Loadable)]
// Loadable could also implement FromWorld
struct MyAssets {
  #[load("a.png")]
  a: Handle<Image>
  #[load("b.png")]
  b: Handle<Image>
}
// ?
fn load_resource<T: Loadable + Resource>(&self) {
  let loadable = T::load(&self);
  self.queue_loaded_resource(loadable)
}
// bad name
// also not great UX ... need to register MyAssets as an asset and look it up, despite it being
// created on the spot. Elegant design but might be worth hacking this in to be -> MyAssets
let my_assets: Handle<MyAssets> = server.load_loadable(); 
fn load_loadable<T: Loadable>(&self) -> Handle<T> {
  let my_assets = T::load(&self);
  self.load_asset(my_assets)
}

```

### Porting

* Render Assets vs App Assets
* Add type hint to loader (first check meta, then file extension, then hint)
* How will "const" handles be "handled" here? We need them for Bundles.
* Configurable meta file generation per-asset type
* Configurable global asset loader / processor default settings

### Maybe before PR

* (give this a try and see how easy it is for pr) Asset _unloading_ and _re-loading_ that allows handles to be kept alive while unloading the asset data itself
    * Being able to kick off re-loads (that re-validate existing handles) seems useful!
    * How would events be handled here?
* Consider reframing meta to be slightly less confusing:
    * loader, if present means the asset can be loaded directly (optional, provided processor exist)
    * processor, if present means the asset can be processed ... do not reuse loader.
        * Processor (enum)
            * None (copy)
            * LoadAndSave (define a loader with settings, define a saver)
            * Direct(read bytes, write bytes)
    * Maybe just a "type" enum?
        * Load
        * Process
            * LoadAndSave (maybe just a variant of Direct?)
            * Direct
* Other OS backends: Android, Web
* async-fs just wraps std::fs and offloads blocking work to a separate thread pool. This prevents us from blocking the IO thread pool (which seems useful), but it isn't "true" async io. Do we need this or should we just use std::fs directly?
* Do we try to prevent (or at least identify) circular dependencies?
* Consider storing asset path in strong handle. `handle.asset_path() -> Option<AssetPath>` would be very cool
    * Can we store _everything_ in the strong handle? Loading dependencies via atomics? Load state via rwlock?
* "Debug" Asset Server
    * Multiple asset sources (add src to AssetPath: `src://path.png#label`)
        * register a debug source? `asset_server.load_asset(LoadedAsset::from(my_shader).with_path_unchecked("debug://bevy_render/shaders/foo.wgsl"))`
* How to handle processed subassets when they have their own paths / are loaded separately? 
    * Different AssetReader? source asset path is a folder
    * Granular processed asset loading seems like a good thing (load Scene0 without Scene1)
* What is a processor if not an AssetServer with hot-reloading that writes processed outputs to disk?
    * Can it just be an alternative internal_asset_event processor with a top level load() orchestrator?
* Should retrieving Meta and Asset bytes be a single call in the interface?
    * would enable transactionality
* Maybe AssetMetaDyn could use erased serde w/ a custom deserializer to avoid double-parsing / AssetMetaMinimal
    * Or untyped reflect?
* Add type hint to loader (first check meta, then file extension, then hint)
* Support loading assets from bytes? Maybe would handle "built in" assets.
* Configurable per-file-type defaults for AssetMeta
    * Store in ErasedAssetMeta form to avoid serializing/deserializing for each asset 
* Preprocessing Options
    * Multiple Preprocessors?
        * Right now we just have Loader > Saver. Savers could, in theory, provide multiple preprocessor options. But what about arbitrary things like GltfLoader->MeshAsset->MeshOpt(MeshAsset)->NormalFix(MeshAsset)->CompactMeshSaver
    * wire in arbitrary "transforms" that produce changes in memory 
* Do we rephrase Reader apis to be `async read(path, bytes: &mut Vec<u8>)`?
* Loader/Saver Versioning
* Cleanup unused folders
    * assets are cleaned up, but folders are not
* Try to remove crossbeam channels for recycling ids
* Handles dropping before load: already implemented slow loop fix ... do better
    * Add test to ensure this works correctly

```rust

// Opt-in handles
#[derive(Asset)]

// Opt-out handles
#[derive(AssetCollection)]


// Maybe this is built into the Asset derive?
fn load<L: Loadable>(loadable: L) -> Handle<L::Type>{

}

impl<A: Asset> Loadable for TypedAssetPath<A> {
    type Type: A;

impl<A: Asset> Loadable for DirectoryPath {
    type Type: LoadedDirectory;
}
impl<A: Asset> Loadable for LoadedAsset<A> {
    type Type: A;
}

#[derive(Loadable)]
pub struct StandardMaterial {
    #[handle]
    color: Handle<Image>,
}

#[derive(Loadable)]
pub struct GameScenes {
    a: Handle<A>,
    b: Handle<B>,
}

impl<A: Asset> Loadable for LoadedAsset<A> {
    type Type: A;
}

struct LoadWithSettings<L>(AssetPath)

fn setup(mut commands: Commands, assets: ResMut<AssetServer>) {
    commands.spawn(assets.load(GameScenes {
        a: assets.load("a.scn"),
        b: assets.load("b.scn"),
    }))
}

fn on_load(query: Query<&Handle<GameScenes>>) {
    for handle in query.iter( {
        if handle.loaded() {
            do_thing()
        }
    })  
}

fn menu_loaded(handle: In<Handle<Scene>>, commands: Commands, state: NextState<GameState>) {
    commands.insert_resource(MenuData {handle})
    state.set(GameState::Menu)
}

fn enter_menu(commands: Commands, state: ResMut<MenuState>) {
    let entity = commands.spawn(SceneBundle {handle: state.handle}).id();
    state.entity = Some(entity);
}

fn exit_menu(commands: Commands, state: Res<MenuState>) {
    commands.despawn(SceneBundle {handle: state})
}


app.add_system(Update, menu_loaded.on_load::<Scene>("menu.scn")) // take an in: In<Handle<Scene>>)
    .add_system(OnEnter(GameState::Menu), enter_menu)
    .add_system(OnExit(GameState::Menu), exit_menu)
```


* Use optimized storage for RenderAssets (and other AssetId maps).

### PR Description / RFC

* Concept introduction
    * What is an Asset? (a runtime thing that can be (but doesn't have to be) loaded from an array of bytes with an "asset path" and metadata about those bytes)
    * Whis is asset preprocessing (the act of taking input "asset source" and writing it to a destination)
    * The asset server does not care _where_ an asset came from (was it manually created? pre processed?)
        * This means asset preprocessing is completely optional
* Efficient asset storage / dense handles / allocate anywhere
* Dependency tracking
* Asset Preprocessing
    * Fully optional
    * General / Recommended Flow
        * Define "unprocessed" loaders into engine-specific format (images, scenes, meshes, etc)
        * Define savers for engine specific formats
        * Add LoadAndSave process plans for assets that use the "unprocessed" loader in combination with engine format savers
* "Everything is a loader"
* Run anywhere / no platform-restricting dependencies
* Async IO
* Single Arc tree (no more "active handle" counting)
* Open Questions
    * Implied dependencies (via load calls / scopes) vs dependency enumeration (via Asset type). Call out tradeoffs in PR
        * Current plan is "dual mode"
    * Aggressive sub asset and dependency loading (which could mean adding sub assets for failed assets) vs conservative.
    * ProcessorDev right now biases toward "fast/eager startup", which mean older versions of assets might be loaded first
        * relies on hot reloading
        * Do we add another "wait for full process" mode that wont load any assets until fully processed?
            * We don't _really_ need to wait for everything ... after all deps for a given asset are checked we can send the event
* Const Typed Handles! Smaller! Faster!
* Directly load asset instances and track their dependencies
* Sub Assets are yielded right away
* Track dependencies for "runtime only" assets
* Lazy init via hot-reloading
    * You can create handles at runtime to assets that havent been created yet
* multiple asset sources
* Better non-blocking folder loading: LoadedFolder asset 
* Paths are canonical
* Asset system usability
    * handle.path()
* Call out "stable type name" as the end game for loader / processor identities

### Why not distill?

* Architectural simplicity: Compare repo size without tests, no inherent need for RPC systems or databases
    * Distill 24,237 lines of code (including generated), 11,499 (without generated) 
    * Bevy 3,983
    * Call out feature diffs for fair contextualization of line differences
        * distill
            * supports asset packs, has a DB which can be queried for metadata with faster startup times, transactional
            * supports GUID ids (bevy plans to but not yet)
            * Remote asset server (bevy plans to but not supported yet)
* Direct filesystem access to processed asset state: easier debugging (this is also how unity does it)
* Optional preprocessing
* "Run anywhere" processor
    * We want to support running the processor on the web and arbitrary platforms (consoles, mobile, etc).
    * lmdb cannot run anywhere
* reusable loaders
* Pluggable: arbitrary asset providers
* Paths are not canonical

### Next Steps

* bevy cli: `bevy asset-processor run`
    * Run bevy game in "asset processing mode"
* Asset Packing
* Migrations:
    * Loader settings migrations (based on loader version)
    * Saver settings migrations (based on loader version)
    * Asset format migrations (such as scene migrations)
        * Per-Component version migrations
            * `component_versions: { "bevy": 0.10, "rapier": 0.2}`
* Per-platform import config
* "Real cow" asset paths
* Support processing assets during load (without background processor enabled)
    * Support processing inside Assets? Or just warn/error?
* Streaming 
    * labeled-asset streaming works with load_asset() (assets pop in as the loader works)
* Should we "pre-empt" loading dependencies by encoding them in the meta file? Currently deps are handled as part of the loader
* load_direct_processed
    * load_direct currently feeds on unprocessed dependencies
    * Good chunk of load_direct cases want unprocessed:
        * shader dependencies want WGSL shader, not SPIRV
        * loading custom file formats with references to other files likely often want their own processing logic
        * anything with load_asset_bytes
* Use UntypedAssetIds where possible in preprocessor (instead of AssetPath)
* One-to-many asset saving. An asset source that produces many assets currently must be processed into a single asset source. If labled assets can be written separately they can each have their own savers and they could be loaded granularly.
* Lots of "AssetPath as identity" everywhere. Should probably exchange these at runtime for an id that is cheaper to hash. 
* watch_for_changes: default to true for dev builds?
* Delay hotreloading? https://github.com/bevyengine/bevy/pull/8503
* Handles could probably be considered "always strong" if we disallow Weak(Index). All arc-ed handles could always be indices