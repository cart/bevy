# Component Storage

* Component storage / other info stored in a Vec<ComponentInfo>
* ComponentId(usize) is the index into that list
* TypeId->ComponentId hash lookup
* Cache where possible (ex: Queries)

## random thoughts

* Archetype/Component Graph?
* quickly hack it in to see if it regresses perf?
* how to reconcile iteration order:
    * iterating archetypes will be a different order than iterating sparse sets
* Bundle type -> ComponentId cache
* Consider using BlobVec in Archetpe
* Use ComponentIds + SparseSets in Archetype for type lookup


## TODO

* Empty archetype
* Flush
* Fixup removal

* consider new hecs entity reserver


* un-comment tests
* make reserver api safe
* consistent unchecked_mut
* simplify SAFETY text
* world.query().get(entity)
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