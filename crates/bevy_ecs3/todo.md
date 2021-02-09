# Component Storage

* Component storage / other info stored in a Vec<ComponentInfo>
* ComponentId(usize) is the index into that list
* TypeId->ComponentId hash lookup
* Cache where possible (ex: Queries)

## Summary of Done Things

* Complete World rewrite (no shared hecs code, other than the entity id allocator)
    * Multiple component storages (tables and sparse sets)
    * EntityRef / EntityMut api
    * Archetype Graph
* Stateful Queries
    * QueryState
    * Fetch and Filter state (stored in QueryStt)
* Perf improvements
    * much faster fragmented iterator perf
    * muuuuuch faster sparse fragmented perf
    * faster table component adds/removes
    * sparse component add/removes 
* Smaller Codebase (verify numbers at the end)
* Reduced monomorphization (measure compile time difference)
* More granular module organization
* Direct stateless World queries are slower
* SystemParam state (still needs "settable" params)
* Safe entity reservation api

## random thoughts

* Archetype/Component Graph?
* quickly hack it in to see if it regresses perf?
* how to reconcile iteration order:
    * iterating archetypes will be a different order than iterating sparse sets
* Bundle type -> ComponentId cache
* Consider using BlobVec in Archetpe
* Use ComponentIds + SparseSets in Archetype for type lookup
* use archetype generation to get the range of archetypes to update

## TODO
* world id safety
* core
    * un-comment all tests
    * Or Filter
    * Removal Tracking
    * ExactSizeIter for Query
    * Flags
    * try removing pre-hash in favor of non-owned get (to allow collision resolution)
    * remove one by one (remove_intersection)
    * prevent allocating in empty archetype on init (maybe use a EntityMutUninit?)
        * last attempt dropped perf
    * simplify SAFETY text
    * consistent unchecked_mut
    * batch_iter
    * Update bundle derive macro
* high level
    * port System to new api
    * port scheduler to new api
    * par_iter

## Maybe
* try trimming down Fetch api
* try removing QueryIter and see how it affects benches
* experiment with inlines
    * pub (crate) where possible (no inline)
* Optimize SparseSet::insert (code is written but it has memory issues)
* inline bundle put?
* consider removing Vec{Entity} from Archetype. tables store that data redundantly?
* query state is an unsafe api
    * maybe this is ok
    * consider abstracting out QueryState in world (hash query to state)
* commands can/should use the graph / an entity builder
* EntitySpawner
    * struct { Entity, Blobs }
* batch archetype changes


## Scratch

A: Archetype
B: Archetype
C: SparseSet

Insert (A, B, C)



impl Bundle for (A, B, C)

* 

## SparseSet

```rust
struct SparseSet<T> {
    components: Vec<T>
    entities: Vec<Entity>,
    set: Vec<MaybeUninit<Entity>>,
    flags: Vec<ComponentFlags>
}
```