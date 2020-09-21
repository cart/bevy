use alloc::vec::Drain;

use crate::{Archetype, Bundle, Component, TypeInfo, World};
use core::any::TypeId;
use std::{mem, vec, vec::Vec};

pub struct SoaBatch<T> {
    component_arrays: T,
}

impl<T> SoaBatch<T> {
    pub fn new(components: T) -> Self {
        Self {
            component_arrays: components,
        }
    }
}

pub trait SpawnBatch {
    fn spawn(self, world: &mut World);
}

macro_rules! count {
    () => { 0 };
    ($x: ident $(, $rest: ident)*) => { 1 + count!($($rest),*) };
}

macro_rules! soa_impl {
    ($($item: ident),+) => {
        impl<$($item: Component),+> SpawnBatch for SoaBatch<($(Vec<$item>),+)>
        {
            fn spawn(self, world: &mut World) {
                world.flush();
                let ($(mut $item),+) = self.component_arrays;
                let mut entity_count = usize::MAX;
                // TODO: check if vec lengths dont match
                $(
                   entity_count =  $item.len();
                )+

                if entity_count == 0 {
                    panic!("there should be at least 1 entity");
                }

                const N: usize = count!($($item),*);
                let mut xs: [(usize, TypeId); N] = [$((mem::align_of::<$item>(), TypeId::of::<$item>())),*];
                xs.sort_unstable_by(|x, y| x.0.cmp(&y.0).reverse().then(x.1.cmp(&y.1)));
                let mut type_ids = vec![TypeId::of::<()>(); N];
                for (slot, &(_, id)) in type_ids.iter_mut().zip(xs.iter()) {
                    *slot = id;
                }

                let archetype_id = world.index.get(&type_ids).copied().unwrap_or_else(|| {
                    let archetype_id = world.archetypes.len() as u32;
                    let mut type_info = vec![
                        $(TypeInfo::of::<$item>(),)+
                    ];
                    type_info.sort_by_key(|i| i.id());
                    world.archetypes.push(Archetype::new(type_info));
                    world.index.insert(type_ids.clone(), archetype_id);
                    world.archetype_generation += 1;
                    archetype_id
                });

                let archetype = &mut world.archetypes[archetype_id as usize];
                world.entities.reserve(entity_count as u32);
                archetype.reserve(entity_count);

                let start_index = archetype.len();
                for entity in world.entities.claim(entity_count as u32) {
                    archetype.allocate(entity);
                }
                assert_eq!(archetype.len(), start_index + entity_count);
                let mut type_index = 0;
                $(
                    let storage = archetype.get_storage_dynamic(type_ids[type_index]).unwrap();
                    // for i in start_index..start_index + entity_count {
                    //     state.added_entities[i] = true;
                    // }
                    unsafe {
                        let item_size = storage.item_size();
                        let type_ptr = storage.get_pointer().add(start_index * item_size) as *mut u8;
                        let components_ptr = $item.as_ptr().cast::<u8>();
                        std::ptr::copy_nonoverlapping(components_ptr, type_ptr, item_size * entity_count);
                    }

                    for i in $item.drain(..) {
                        mem::forget(i);
                    }

                    // TODO: mem forget components
                    type_index += 1;
                )+

                // std::println!("{}", type_index);
            }
        }
    };
}

soa_impl!(A, B);
soa_impl!(A, B, C);
soa_impl!(A, B, C, D);
