# Component Storage

* Component storage / other info stored in a Vec<ComponentInfo>
* ComponentId(usize) is the index into that list
* TypeId->ComponentId hash lookup
* Cache where possible (ex: Queries)

## Done

* Multiple component storages (tables and sparse sets)
* Complete World rewrite (no shared hecs code, other than the entity id allocator)
* EntityRef / EntityMut api
* Stateful Queries
* Smaller Codebase (verify numbers at the end)
* Reduced monomorphization (measure compile time difference)

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

* Optimize SparseSet::insert (code is written but it has memory issues)
* Tracking
    * fix memory access issue in add_remove_many_tables
    * finish updating ComponentSparseSet with flags

* inline bundle put?
* try removing pre-hash in favor of non-owned get (to allow collision resolution)
* consider removing Vec{Entity} from Archetype. tables store that data redundantly?
* consider specializing spawn_bundle
* remove one by one
* prevent allocating in empty archetype on init (maybe use a EntityMutUninit?)
    * last attempt dropped perf
* query state is an unsafe api
    * maybe this is ok
* change tracking
    * store adds/removes/unchanged in ArchetypeEdges. use these for change tracking
    * ensure tracking components are correctly added to archetypes / tables
    * consider removing branching / assuming things are tracked?
* un-comment tests
* make reserver api safe
    * pass world into Commands
* consistent unchecked_mut
* simplify SAFETY text
* world.query().get(entity)
* commands can/should use the graph / an entity builder
* EntitySpawner
    * struct { Entity, Blobs }


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