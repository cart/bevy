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

* consider specializing spawn_bundle
* remove one by one
* if we roll with componentid graph, set initial table capacity to 0 to cut down on (probably) unused tables
* prevent allocating in empty archetype on init (maybe use a EntityMutUninit?)
* query state is an unsafe api
* change tracking
* un-comment tests
* make reserver api safe
* consistent unchecked_mut
* simplify SAFETY text
* world.query().get(entity)
* commands can/should use the graph / an entity builder
* EntitySpawner
    * struct { Entity, Blobs }

* consider if we really need sparse sets:
    * each archetype could store the index into a single column table instead
    * rather than needing a sparse lookup, it could instead be "dense".


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