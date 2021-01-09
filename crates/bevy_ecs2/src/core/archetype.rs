use crate::{AtomicBorrow, Component, ComponentFlags, Entity, Location};
use bevy_utils::AHasher;
use std::{
    alloc::{alloc, dealloc, Layout},
    any::TypeId,
    cell::UnsafeCell,
    collections::HashMap,
    hash::Hasher,
    mem,
    ptr::{self, NonNull},
};

use super::entities::Entities;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ArchetypeId(pub(crate) u32);

impl ArchetypeId {
    #[inline]
    pub fn empty_archetype() -> ArchetypeId {
        ArchetypeId(0)
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub fn is_empty_archetype(&self) -> bool {
        self.0 == 0
    }
}

/// Determines freshness of information derived from `World::archetypes`
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ArchetypesGeneration(pub u32);

/// A collection of entities having the same component types
///
/// Accessing `Archetype`s is only required for complex dynamic scheduling. To manipulate entities,
/// go through the `World`.
#[derive(Debug)]
pub struct Archetype {
    id: ArchetypeId,
    types: Vec<TypeInfo>,
    // A hasher optimized for hashing a single TypeId.
    // We don't use RandomState from std or Random state from Ahash
    // because fxhash is [proved to be faster](https://github.com/bevyengine/bevy/pull/1119#issuecomment-751361215)
    // and we don't need Hash Dos attack protection here
    // since TypeIds generated during compilation and there is no reason to user attack himself.
    state: HashMap<TypeId, TypeState, fxhash::FxBuildHasher>,
    len: usize,
    entities: Vec<Entity>,
    // UnsafeCell allows unique references into `data` to be constructed while shared references
    // containing the `Archetype` exist
    data: UnsafeCell<NonNull<u8>>,
    data_size: usize,
    grow_size: usize,
}

impl Archetype {
    fn assert_type_info(types: &[TypeInfo]) {
        types.windows(2).for_each(|x| match x[0].cmp(&x[1]) {
            core::cmp::Ordering::Less => (),
            #[cfg(debug_assertions)]
            core::cmp::Ordering::Equal => panic!(
                "attempted to allocate entity with duplicate {} components; \
                 each type must occur at most once!",
                x[0].type_name()
            ),
            #[cfg(not(debug_assertions))]
            core::cmp::Ordering::Equal => panic!(
                "attempted to allocate entity with duplicate components; \
                 each type must occur at most once!"
            ),
            core::cmp::Ordering::Greater => panic!("Type info is unsorted."),
        });
    }

    #[allow(missing_docs)]
    pub fn new(id: ArchetypeId, types: Vec<TypeInfo>) -> Self {
        Self::with_grow(id, types, 64)
    }

    #[allow(missing_docs)]
    pub fn with_grow(id: ArchetypeId, mut types: Vec<TypeInfo>, grow_size: usize) -> Self {
        types.sort_unstable();
        Self::assert_type_info(&types);
        let mut state = HashMap::with_capacity_and_hasher(types.len(), Default::default());
        for ty in &types {
            state.insert(ty.id(), TypeState::new());
        }
        Self {
            id,
            state,
            types,
            entities: Vec::new(),
            len: 0,
            data: UnsafeCell::new(NonNull::dangling()),
            data_size: 0,
            grow_size,
        }
    }

    pub(crate) fn clear(&mut self) {
        for ty in &self.types {
            for index in 0..self.len {
                unsafe {
                    let removed = self
                        .get_dynamic(ty.id(), ty.layout().size(), index)
                        .unwrap()
                        .as_ptr();
                    (ty.drop)(removed);
                }
            }
        }
        self.len = 0;
    }

    #[inline]
    pub fn id(&self) -> ArchetypeId {
        self.id
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn has<T: Component>(&self) -> bool {
        self.has_dynamic(TypeId::of::<T>())
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn has_type(&self, ty: TypeId) -> bool {
        self.has_dynamic(ty)
    }

    pub(crate) fn has_dynamic(&self, id: TypeId) -> bool {
        self.state.contains_key(&id)
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn get<T: Component>(&self) -> Option<NonNull<T>> {
        let state = self.state.get(&TypeId::of::<T>())?;
        Some(unsafe {
            NonNull::new_unchecked(
                (*self.data.get()).as_ptr().add(state.offset).cast::<T>() as *mut T
            )
        })
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn get_with_type_state<T: Component>(&self) -> Option<(NonNull<T>, &TypeState)> {
        let state = self.state.get(&TypeId::of::<T>())?;
        Some(unsafe {
            (
                NonNull::new_unchecked(
                    (*self.data.get()).as_ptr().add(state.offset).cast::<T>() as *mut T
                ),
                state,
            )
        })
    }

    #[allow(missing_docs)]
    pub fn get_type_state(&self, ty: TypeId) -> Option<&TypeState> {
        self.state.get(&ty)
    }

    #[allow(missing_docs)]
    pub fn get_type_state_mut(&mut self, ty: TypeId) -> Option<&mut TypeState> {
        self.state.get_mut(&ty)
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[allow(missing_docs)]
    pub fn iter_entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter().take(self.len)
    }

    #[inline]
    pub(crate) fn entities(&self) -> NonNull<Entity> {
        unsafe { NonNull::new_unchecked(self.entities.as_ptr() as *mut _) }
    }

    pub(crate) fn get_entity(&self, index: usize) -> Entity {
        self.entities[index]
    }

    #[inline]
    pub unsafe fn get_entity_unchecked(&self, index: usize) -> Entity {
        *self.entities.get_unchecked(index)
    }

    #[allow(missing_docs)]
    pub fn types(&self) -> &[TypeInfo] {
        &self.types
    }

    /// # Safety
    /// `index` must be in-bounds
    pub(crate) unsafe fn get_dynamic(
        &self,
        ty: TypeId,
        size: usize,
        index: usize,
    ) -> Option<NonNull<u8>> {
        debug_assert!(index < self.len);
        Some(NonNull::new_unchecked(
            (*self.data.get())
                .as_ptr()
                .add(self.state.get(&ty)?.offset + size * index)
                .cast::<u8>(),
        ))
    }

    /// # Safety
    /// Every type must be written immediately after this call
    pub unsafe fn allocate(&mut self, id: Entity) -> usize {
        if self.len == self.entities.len() {
            self.grow(self.len.max(self.grow_size));
        }

        self.entities[self.len] = id;
        self.len += 1;
        self.len - 1
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        if additional > (self.capacity() - self.len()) {
            self.grow(additional - (self.capacity() - self.len()));
        }
    }

    fn capacity(&self) -> usize {
        self.entities.len()
    }

    #[allow(missing_docs)]
    pub fn clear_trackers(&mut self) {
        for type_state in self.state.values_mut() {
            type_state.clear_trackers();
        }
    }

    fn grow(&mut self, increment: usize) {
        unsafe {
            let old_count = self.len;
            let new_capacity = self.capacity() + increment;
            self.entities.resize(
                new_capacity,
                Entity {
                    id: u32::MAX,
                    generation: u32::MAX,
                },
            );

            for type_state in self.state.values_mut() {
                type_state
                    .component_flags
                    .resize_with(new_capacity, ComponentFlags::empty);
            }

            let old_data_size = mem::replace(&mut self.data_size, 0);
            let mut old_offsets = Vec::with_capacity(self.types.len());
            for ty in &self.types {
                self.data_size = align(self.data_size, ty.layout().align());
                let ty_state = self.state.get_mut(&ty.id()).unwrap();
                old_offsets.push(ty_state.offset);
                ty_state.offset = self.data_size;
                self.data_size += ty.layout().size() * new_capacity;
            }
            let new_data = if self.data_size == 0 {
                NonNull::dangling()
            } else {
                NonNull::new(alloc(
                    Layout::from_size_align(
                        self.data_size,
                        self.types.first().map_or(1, |x| x.layout().align()),
                    )
                    .unwrap(),
                ))
                .unwrap()
            };
            if old_data_size != 0 {
                for (i, ty) in self.types.iter().enumerate() {
                    let old_off = old_offsets[i];
                    let new_off = self.state.get(&ty.id()).unwrap().offset;
                    ptr::copy_nonoverlapping(
                        (*self.data.get()).as_ptr().add(old_off),
                        new_data.as_ptr().add(new_off),
                        ty.layout().size() * old_count,
                    );
                }
                dealloc(
                    (*self.data.get()).as_ptr().cast(),
                    Layout::from_size_align_unchecked(
                        old_data_size,
                        self.types.first().map_or(1, |x| x.layout().align()),
                    ),
                );
            }

            self.data = UnsafeCell::new(new_data);
        }
    }

    /// Returns the ID of the entity moved into `index`, if any
    pub(crate) unsafe fn remove(&mut self, index: usize) -> Option<Entity> {
        let last = self.len - 1;
        for ty in &self.types {
            let removed = self
                .get_dynamic(ty.id(), ty.layout().size(), index)
                .unwrap()
                .as_ptr();
            (ty.drop)(removed);
            if index != last {
                // TODO: copy component tracker state here
                ptr::copy_nonoverlapping(
                    self.get_dynamic(ty.id(), ty.layout().size(), last)
                        .unwrap()
                        .as_ptr(),
                    removed,
                    ty.layout().size(),
                );

                let type_state = self.state.get_mut(&ty.id()).unwrap();
                type_state.component_flags[index] = type_state.component_flags[last];
            }
        }
        self.len = last;
        if index != last {
            self.entities[index] = self.entities[last];
            Some(self.entities[last])
        } else {
            None
        }
    }

    /// Returns the ID of the entity moved into `index`, if any
    pub(crate) unsafe fn move_to(
        &mut self,
        index: usize,
        mut f: impl FnMut(*mut u8, TypeId, usize, ComponentFlags),
    ) -> Option<Entity> {
        let last = self.len - 1;
        for ty in &self.types {
            let moved = self
                .get_dynamic(ty.id(), ty.layout().size(), index)
                .unwrap()
                .as_ptr();
            let type_state = self.state.get(&ty.id()).unwrap();
            let flags = type_state.component_flags[index];
            f(moved, ty.id(), ty.layout().size(), flags);
            if index != last {
                ptr::copy_nonoverlapping(
                    self.get_dynamic(ty.id(), ty.layout().size(), last)
                        .unwrap()
                        .as_ptr(),
                    moved,
                    ty.layout().size(),
                );
                let type_state = self.state.get_mut(&ty.id()).unwrap();
                type_state.component_flags[index] = type_state.component_flags[last];
            }
        }
        self.len -= 1;
        if index != last {
            self.entities[index] = self.entities[last];
            Some(self.entities[last])
        } else {
            None
        }
    }

    /// # Safety
    ///
    ///  - `component` must point to valid memory
    ///  - the component `ty`pe must be registered
    ///  - `index` must be in-bound
    ///  - `size` must be the size of the component
    ///  - the storage array must be big enough
    pub unsafe fn put_component(
        &mut self,
        component: *mut u8,
        ty: TypeId,
        size: usize,
        index: usize,
        flags: ComponentFlags,
    ) {
        let state = self.state.get_mut(&ty).unwrap();
        state.component_flags[index] = flags;
        let ptr = (*self.data.get())
            .as_ptr()
            .add(state.offset + size * index)
            .cast::<u8>();
        ptr::copy_nonoverlapping(component, ptr, size);
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        self.clear();
        if self.data_size != 0 {
            unsafe {
                dealloc(
                    (*self.data.get()).as_ptr().cast(),
                    Layout::from_size_align_unchecked(
                        self.data_size,
                        self.types.first().map_or(1, |x| x.layout().align()),
                    ),
                );
            }
        }
    }
}

/// Metadata about a type stored in an archetype
#[derive(Debug)]
pub struct TypeState {
    offset: usize,
    borrow: AtomicBorrow,
    component_flags: Vec<ComponentFlags>,
}

impl TypeState {
    fn new() -> Self {
        Self {
            offset: 0,
            borrow: AtomicBorrow::new(),
            component_flags: Vec::new(),
        }
    }

    fn clear_trackers(&mut self) {
        for flags in self.component_flags.iter_mut() {
            *flags = ComponentFlags::empty();
        }
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn component_flags(&self) -> NonNull<ComponentFlags> {
        unsafe { NonNull::new_unchecked(self.component_flags.as_ptr() as *mut ComponentFlags) }
    }
}

fn align(x: usize, alignment: usize) -> usize {
    debug_assert!(alignment.is_power_of_two());
    (x + alignment - 1) & (!alignment + 1)
}

/// A hasher optimized for hashing a single TypeId.
///
/// TypeId is already thoroughly hashed, so there's no reason to hash it again.
/// Just leave the bits unchanged.
#[derive(Default)]
pub(crate) struct TypeIdHasher {
    hash: u64,
}

impl Hasher for TypeIdHasher {
    fn write_u64(&mut self, n: u64) {
        // Only a single value can be hashed, so the old hash should be zero.
        debug_assert_eq!(self.hash, 0);
        self.hash = n;
    }

    // Tolerate TypeId being either u64 or u128.
    fn write_u128(&mut self, n: u128) {
        debug_assert_eq!(self.hash, 0);
        self.hash = n as u64;
    }

    fn write(&mut self, bytes: &[u8]) {
        debug_assert_eq!(self.hash, 0);

        // This will only be called if TypeId is neither u64 nor u128, which is not anticipated.
        // In that case we'll just fall back to using a different hash implementation.
        let mut hasher = AHasher::default();
        hasher.write(bytes);
        self.hash = hasher.finish();
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

pub struct Archetypes {
    pub archetypes: Vec<Archetype>,
    archetype_ids: HashMap<Vec<TypeId>, ArchetypeId>,
    removed_components: HashMap<TypeId, Vec<Entity>>,
}

impl Default for Archetypes {
    fn default() -> Self {
        Self {
            archetypes: vec![Archetype::new(ArchetypeId::empty_archetype(), Vec::new())],
            archetype_ids: Default::default(),
            removed_components: Default::default(),
        }
    }
}

impl Archetypes {
    #[inline]
    pub fn empty_archetype(&self) -> &Archetype {
        &self.archetypes[0]
    }

    #[inline]
    pub(crate) fn empty_archetype_mut(&mut self) -> &mut Archetype {
        &mut self.archetypes[0]
    }

    #[inline]
    pub fn generation(&self) -> ArchetypesGeneration {
        ArchetypesGeneration(self.archetypes.len() as u32)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.archetypes.len()
    }

    #[inline]
    pub fn get(&self, id: ArchetypeId) -> Option<&Archetype> {
        self.archetypes.get(id.0 as usize)
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, id: ArchetypeId) -> &Archetype {
        self.archetypes.get_unchecked(id.0 as usize)
    }

    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        self.archetypes.get_unchecked_mut(id.0 as usize)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: ArchetypeId) -> Option<&mut Archetype> {
        self.archetypes.get_mut(id.0 as usize)
    }

    pub(crate) fn get_or_insert(&mut self, mut type_info: Vec<TypeInfo>) -> ArchetypeId {
        type_info.sort_unstable();
        let type_ids = type_info
            .iter()
            .map(|info| info.id)
            .collect::<Vec<TypeId>>();

        let archetypes = &mut self.archetypes;
        *self.archetype_ids.entry(type_ids).or_insert_with(|| {
            let id = ArchetypeId(archetypes.len() as u32);
            archetypes.push(Archetype::new(id, type_info));
            id
        })
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Archetype> {
        self.archetypes.iter()
    }

    pub(crate) fn clear_trackers(&mut self) {
        for archetype in self.archetypes.iter_mut() {
            archetype.clear_trackers();
        }

        self.removed_components.clear();
    }

    /// Removes the `entity` at the given `location` and returns an Entity that moved into the new location (if an entity was moved)
    #[inline]
    pub unsafe fn remove_entity_unchecked(
        &mut self,
        entity: Entity,
        location: Location,
    ) -> Option<Entity> {
        let archetype = self
            .archetypes
            .get_unchecked_mut(location.archetype.index());
        let moved_entity = archetype.remove(location.index);
        for ty in archetype.types() {
            let removed_entities = self
                .removed_components
                .entry(ty.id())
                .or_insert_with(Vec::new);
            removed_entities.push(entity);
        }

        moved_entity
    }

    pub fn removed<C: Component>(&self) -> &[Entity] {
        self.removed_components
            .get(&TypeId::of::<C>())
            .map_or(&[], |entities| entities.as_slice())
    }

    #[inline]
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Archetype> {
        self.archetypes.iter_mut()
    }

    pub(crate) fn flush_entities(&mut self, entities: &mut Entities) {
        let empty_archetype = self.empty_archetype_mut();
        for entity_id in entities.flush() {
            entities.meta[entity_id as usize].location.index = unsafe {
                empty_archetype.allocate(Entity {
                    id: entity_id,
                    generation: entities.meta[entity_id as usize].generation,
                })
            };
        }
        for i in 0..entities.reserved_len() {
            let id = entities.reserved(i);
            entities.meta[id as usize].location.index = unsafe {
                empty_archetype.allocate(Entity {
                    id,
                    generation: entities.meta[id as usize].generation,
                })
            };
        }
        entities.clear_reserved();
    }

    pub(crate) fn clear(&mut self) {
        for archetype in &mut self.archetypes {
            for ty in archetype.types() {
                let removed_entities = self
                    .removed_components
                    .entry(ty.id())
                    .or_insert_with(Vec::new);
                removed_entities.extend(archetype.iter_entities().copied());
            }
            archetype.clear();
        }
    }
}

/// Metadata required to store a component
#[derive(Debug, Copy, Clone)]
pub struct TypeInfo {
    id: TypeId,
    layout: Layout,
    drop: unsafe fn(*mut u8),
    type_name: &'static str,
}

impl TypeInfo {
    /// Metadata for `T`
    pub fn of<T: 'static>() -> Self {
        unsafe fn drop_ptr<T>(x: *mut u8) {
            x.cast::<T>().drop_in_place()
        }

        Self {
            id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop: drop_ptr::<T>,
            type_name: core::any::type_name::<T>(),
        }
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn id(&self) -> TypeId {
        self.id
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn drop(&self) -> unsafe fn(*mut u8) {
        self.drop
    }

    #[allow(missing_docs)]
    #[inline]
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }
}

impl PartialOrd for TypeInfo {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TypeInfo {
    /// Order by alignment, descending. Ties broken with TypeId.
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.layout
            .align()
            .cmp(&other.layout.align())
            .reverse()
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialEq for TypeInfo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for TypeInfo {}
