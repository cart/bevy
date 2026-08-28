#![allow(missing_docs)]

use crate::{
    component::Component,
    entity::{ContainsEntity, Entity},
    world::World,
};
use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use bevy_reflect::{Reflect, TypePath};
use crossbeam_channel::Sender;
use downcast_rs::{impl_downcast, Downcast};

/// A handle to an entity.
#[derive(Reflect, Debug)]
pub struct EntityHandle(pub Arc<InnerEntityHandle>);

impl Clone for EntityHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl Drop for InnerEntityHandle {
    fn drop(&mut self) {
        let _ = self.drop_sender.send(self.entity);
    }
}

impl ContainsEntity for EntityHandle {
    fn entity(&self) -> Entity {
        self.0.entity
    }
}

impl EntityHandle {
    pub fn id(&self) -> Entity {
        self.0.entity
    }

    pub fn data<T: EntityHandleData>(&self) -> Option<&T> {
        self.0.data.downcast_ref::<T>()
    }

    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    pub fn weak(&self) -> WeakEntityHandle {
        WeakEntityHandle(Arc::downgrade(&self.0))
    }
}

#[derive(Component, Debug)]
pub struct WeakEntityHandle(Weak<InnerEntityHandle>);

impl WeakEntityHandle {
    pub fn upgrade(&self) -> Option<EntityHandle> {
        self.0.upgrade().map(|value| EntityHandle(value))
    }

    pub fn strong_count(&self) -> usize {
        Weak::strong_count(&self.0)
    }
}

impl Clone for WeakEntityHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(TypePath)]
pub struct InnerEntityHandle {
    pub entity: Entity,
    pub data: Box<dyn EntityHandleData>,
    pub(super) drop_sender: Sender<Entity>,
}

impl core::fmt::Debug for InnerEntityHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InnerEntityHandle")
            .field("entity", &self.entity)
            .field("drop_sender", &self.drop_sender)
            .finish()
    }
}

pub fn despawn_dropped_entities(world: &mut World) {
    while let Ok(entity) = world.entity_allocator.handle_drop_receiver().try_recv() {
        world.despawn(entity);
    }
}

/// Erased data stored on an [`EntityHandle`].
pub trait EntityHandleData: Send + Sync + Downcast {}

impl EntityHandleData for () {}

impl_downcast!(EntityHandleData);

#[cfg(test)]
mod tests {
    use crate::{entity::despawn_dropped_entities, world::World};

    #[test]
    fn despawn_entity_handle() {
        let mut world = World::new();
        let handle = world.spawn_empty().handle();
        despawn_dropped_entities(&mut world);
        assert!(world.get_entity(handle.id()).is_ok());
        let id = handle.id();
        let handle2 = handle.clone();
        drop(handle);
        assert!(world.get_entity(id).is_ok());
        despawn_dropped_entities(&mut world);
        assert!(world.get_entity(id).is_ok());
        drop(handle2);
        despawn_dropped_entities(&mut world);
        assert!(world.get_entity(id).is_err());
    }
}
