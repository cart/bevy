## DONE

* Complete World rewrite (no shared hecs code, other than the entity id allocator)
    * Multiple component storages (tables and sparse sets)
    * EntityRef / EntityMut api
    * Archetype Graph
    * ComponentId + ComponentInfo
        * Densely packed
            * access comparisons are now bitsets everywhere (reduces hashing)
            * Cheaper to look up component info: sparse set instead of hashmap
* Stateful Queries
    * QueryState
    * Fetch and Filter state (stored in QueryState)
* Perf improvements
    * much faster fragmented iterator perf
    * muuuuuch faster sparse fragmented perf
    * faster table component adds/removes
    * sparse component add/removes 
* Query conflicts are determined by Component access instead of Archetype Component access
    * stricter, but more predictable. lets see how it plays out
    * With/Without are still taken into account for conflicts, so this should still be pretty comfy to use
* Smaller Codebase (verify numbers at the end, at the very least core is smaller?)
* Reduced monomorphization (ty measuring compile time difference)
* More granular module organization
* Direct stateless World queries are slower
* SystemParam state (still needs "settable" params)
* Safety Improvements
    * Entity reservation uses a normal world reference instead of unsafe transmute
    * QuerySets no longer transmute lifetimes
    * SystemParamState is an unsafe trait
    * More thorough safety docs
* Slightly nicer IntoSystem / FuncSystem impl (inspired by DJMcnab's work)
* New removal system param API
    * old version existed on queries: had no relation to the query
    * caches component id
* removed `Mut<T>` query impl. better to only support one way `&mut T` 
* Removed with() from `Flags<T>` in favor of `Option<Flags<T>>`, which allows querying for flags to be "filtered" by default 
* Replaced slow "remove_bundle_one_by_one" used as fallback for Commands::remove_bundle with fast "remove_bundle_intersection"
* Components now have is_send property (currently only resources support non-send)
* WorldCell

## TODO
* world id safety
* documentation / symbol review
* todo review
* readme
* core
    * resolve remaining miri errors
    * Fix or filter test (un comment code)
    * Or filters break when some archetypes match and others don't
        * consider making or filters non-dense. then each item's matches_archetype could be used in Or::set_archetype to determine if it should be accessed
        * alternatively make all filters capable of handling mismatched calls to set_table/set_archetype (and check for item existence on each set_table)
    * panic on conflicting fetches (&A, &mut A)
    * consider reverting all_tuples proc macro. it makes RA sad
    * drop tests
    * Nested Bundles 
    * batch_iter
    * par_iter
    * make type_id totally optional
    * test same archetype / component id when resource removed
    * Optimize SparseSet::insert (code is written but it has memory issues)
    * un-comment all tests
    * try removing pre-hash in favor of non-owned get (to allow collision resolution)
    * prevent allocating in empty archetype on init (maybe use a EntityMutUninit?)
        * last attempt dropped perf
    * simplify SAFETY text
    * consistent unchecked_mut (unsafe_mut == potential Mut aliasing, unchecked_mut == no bounds checks)
    * try removing "unchecked" methods to cut down on unsafe and see if it cuts perf 
    * Foreach tests
    * Test stateful query adapting to archetype changes
    * Give Option fetch access updating some scrutiny
    * Rename System::Update() to System::UpdateAccess() (only pass in required data)
    * investigate slower becs3 schedule perf (54 vs 69 us) ... afaik ive only subtracted ops so wtf
        * busy and contrived benches are 3x slower!!!! wtf!!!
    * make StorageType a builder on descriptor?

## LATER

* it is currently impossible to use resources during a "write query" or entity insertion with safe code directly on &mut World. This could be solved by adding "mixed entity / resource queries" or expanding WorldCell to include spawn and query ops. (see ecs_guide.rs for example)
* world.clear
* world.reserve
* ChangedRes -> Res::is_changed

## Maybe
* fixedbitset could be allocation free (store blocks in a SmallVec)
* fixedbitset could have a const constructor 
* World Error Handling (EntityRef)
* consider adding Unique to StorageType
* TrackedWorld
    * runtime borrow checked wrapper around world
* try trimming down Fetch api
* experiment with inlines
    * pub (crate) where possible (no inline)
* inline bundle put?
* commands can/should use the graph / an entity builder
* EntitySpawner
    * struct { Entity, Blobs }
* batch archetype changes

## New Limitations

* Resources added at runtime will be ignored
