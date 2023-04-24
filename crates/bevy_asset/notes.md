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

* How to hand out entity/asset handles during load?
* Using Entities as Handles lazily creates a "duplicate" problem. Would require RWLocks
* Callout that "this impl is aspirational"
    * Hinges on RwLocking once for handles being ok
    * Hinges on non-world ref parallel entity allocation being possible
* Maybe just use a crossbeam channel for pending ents. Would make reuse across threads possible?
* Should we "pre-empt" loading dependencies by encoding them in the meta file? Currently deps are handled as part of the loader
* Overriding default loaders would require loading assetmeta _first_, but this impl relies on knowing the loader type ahead of time (from the path)
* Can erased loaders store some state?
    * Ex: default_meta could return a ref, could store a "passthrough" flag
* Add option to reuse loader settings in processor
* Should retrieving Meta and Asset bytes be a single call in the interface?
    * would enable transactionality
* Maybe AssetMetaDyn could use erased serde w/ a custom deserializer to avoid double-parsing / AssetMetaMinimal
* AssetPath could avoid a lot of allocations by impling "real cows". Could generally be smarter about these. Do a pass to avoid unnecessary clones
* Add tests / mock out loader storage
* How to handle "built in" assets?
* Add type hint to loader (first check meta, then file extension, then hint)
* Support loading assets from bytes? Maybe would handle "built in" assets.
* How will "const" handles be "handled" here? We need them for Bundles.
* "Default" handles?
* Idea: store asset_path in handle, but still only use entity for identity  
* Make load state a bitset: smaller, easier to access, cross-state info tests
    * Cons: ECS change detection ceases to be useful? Ex: An asset is loaded, then its deps load in a later frame, how do you know if the asset load happened that frame. Need to prevent Loaded | Failed states at runtime.
    * Maybe just combine into a single type? Still breaks change detection
* Unified Assets
    * AssetsMode::Unprocessed
        * load() reads from source
        * hitting a processor config will either (1) run the processor inline or (2) fail
    * AssetMode::Processed
        * load() reads from destination 
        * hitting a processor config will either (1) run the processor inline or (2) fail
    * AssetMode::ProcessedDev
        * load() reads from destination, blocks on processed status from host (for now in-process, but should support out-of-process for editor scenarios). Should not treat "non-existent because not yet processed" as an "asset does not exist" failure
        * kicks off processor in new thread ... this should not fail
        * processor _should not_ share entity identity space with app. Therefore it should have its own copy of Assets?
* Multiple Preprocessors?
    * Right now we just have Loader > Saver. Savers could, in theory, provide multiple preprocessor options. But what about arbitrary things like GltfLoader->MeshAsset->MeshOpt(MeshAsset)->NormalFix(MeshAsset)->CompactMeshSaver
* Give me a handle for a given loading asset path
    1. Map path to load state in asset server
        1.5 If load state doesn't exist, kick of load and create state
    2. allocate asset id (and handle) and increment root count associated with handle

## TODO

### MVP

* Adopt old-style id + storage system
    * Update load interfaces to account for TypeId being needed for handle creation
        * TypeId sources (least to most important)
            * From extension (hint that could be wrong ... overridden by handle type and meta)
            * From handle load (hint that could be wrong ... won't exist for untyped loads, overridden by meta, must be compatible with extension) 
            * From meta-defined loader (source of truth if it exists)
        * Untyped loading
            * Check 
        * Interfaces
        * make tests to ensure id allocation fails at the appropriate time for "mismatched types" like `let h: Handle<Mesh> = assets.load("x.png");`
    * Who tracks handle roots? This should probably be the Assets collection
    * Try to remove crossbeam channels for recycling ids
* Impl Reflect and FromReflect for Handle
* Final pass over todo! and TODO / PERF
* Validate dependency types in preprocessor? 
* Asset _unloading_ and _re-loading_ that allows handles to be kept alive while unloading the asset data itself
    * Being able to kick off re-loads (that re-validate existing handles) seems useful!
    * How would events be handled here?
* Hot Reloading
* Might want to gate dep count increments / decrements on hashset ops (or just use hashsets). Otherwise reloading a dep could affect load correctness. 
* Consider de-duping LoadState / only storing in ECS
    * The problem with this is entities are only created when loading event is processed. If an asset loads before the loading event is processed for a dep, the entity won't exist yet.
    * Do we just remove the components, treat "server state" as the "current source of truth" and then make events the way to listen for changes? 
* Handles dropping before load breaks?
    * Implemented slow loop fix ... do better
    * Add test to ensure this works correctly

### Porting

* Render Assets vs App Assets
* Add type hint to loader (first check meta, then file extension, then hint)
* How will "const" handles be "handled" here? We need them for Bundles.
* Configurable meta file generation per-asset type
* Configurable global asset loader / processor default settings

### Maybe before PR

* Other OS backends: Android, Web
* Should we combine meta + asset loading apis?
* More granular preprocessor locking
    * Global gate on scan (to build view of the world)
    * Granular gate on individual outputs (maybe done in Assets directly?)
* Using load_direct_async for "embedded" processor dependencies means we "re-load" such dependencies multiple times. On the other hand, maybe we want this ownership model.
* async-fs just wraps std::fs and offloads blocking work to a separate thread pool. This prevents us from blocking the IO thread pool (which seems useful), but it isn't "true" async io. Do we need this or should we just use std::fs directly?
* Do we try to prevent (or at least identify) circular dependencies?
* Investigate non-crossbeam-channel approaches to id recycling 
* Handles could probably be considered "always strong" if we disallow Weak(Index). All arc-ed handles could always be indices
    * Handle::Uuid(Uuid), Handle::Index(Arc<IndexHandle>)
* Consider storing asset path in strong handle. `handle.asset_path() -> Option<AssetPath>` would be very cool
    * Can we store _everything_ in the strong handle? Loading dependencies via atomics? Load state via rwlock?
* Loading UX
    * "dependency subscriber system"
        * "AssetDependencies" derive?
        * Maybe just a part of the Asset derive?
        * Should be able to support two cases:
            * Dependency tracking for non-asset-server assets
            * Dependency tracking for groups of assets (ex: a list of assets you want to wait to load)
    * Review bevy_asset_loader for UX ideas
* Implied dependencies (via load calls / scopes) vs dependency enumeration (via Asset type). Call out tradeoffs in PR
* RenderAssets .. remove need for id extraction
* Move Reader into LoadContext and add LoadContext::read_bytes()?
* LabeledLoadContext pattern doesn't support parallel context access
    * Consider moving to a `LoadedAsset<A>` approach (with loaded.with_dependency(&load_context, path))
* load_folder ... this is a long-running operation ... it should not be sync
    * consider something AssetCollection-like (ex: Return Handle<AssetCollection>)?
    * Multicast pub-sub?

### PR Description / RFC

* Efficient asset storage / dense handles / allocate anywhere
* Dependency tracking
* Asset Preprocessing
* Run anywhere / no platform-restricting dependencies
* Async IO
* Single Arc tree (no more "active handle" counting)
* Open Questions
    * Implied dependencies (via load calls / scopes) vs dependency enumeration (via Asset type). Call out tradeoffs in PR
* Const Typed Handles!

### Next Steps

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