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
* Smaller Codebase (verify numbers at the end, at the very least core is smaller?)
* Reduced monomorphization (ty measuring compile time difference)
* More granular module organization
* Direct stateless World queries are slower
* SystemParam state (still needs "settable" params)
* Safer
    * Entity reservation uses a normal world reference instead of unsafe transmute
    * QuerySets no longer transmute lifetimes
    * SystemParamState is an unsafe trait

## TODO
* world id safety
* core
    * un-comment all tests
    * Or Filter
    * Removal Tracking
    * Flags
    * fail on duplicate components in bundle
    * try removing pre-hash in favor of non-owned get (to allow collision resolution)
    * remove one by one (remove_intersection)
    * prevent allocating in empty archetype on init (maybe use a EntityMutUninit?)
        * last attempt dropped perf
    * simplify SAFETY text
    * consistent unchecked_mut
    * batch_iter
    * Update bundle derive macro
    * Consider making archetype id a usize (but still u32 in entity id)
    * try removing "unchecked" methods to cut down on unsafe and see if it cuts perf 
    * Optimize SparseSet::insert (code is written but it has memory issues)
    * Local
    * Foreach tests
    * Test stateful query adapting to archetype changes
    * Give Option fetch access updating some scrutiny
* high level
    * par_iter
    * Rename System::Update() to System::UpdateAccess() (only pass in required data)
    * investigate slower becs3 schedule perf (54 vs 69 us) ... afaik ive only subtracted ops so wtf
* resources
    * NonSend (system param too)

## LATER

* world.reserve

## Maybe
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
