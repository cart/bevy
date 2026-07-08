#![allow(missing_docs)]
use core::ops::Deref;

use crate::{
    component::Component,
    entity::{ContainsEntity, Entity},
    world::World,
};
use alloc::sync::{Arc, Weak};
use bevy_reflect::{Reflect, TypePath};
use crossbeam_channel::Sender;

/// A handle to an entity.
#[derive(Reflect)]
pub struct EntityHandle<T = ()>(pub Arc<InnerEntityHandle<T>>);

impl<T: alloc::fmt::Debug> alloc::fmt::Debug for EntityHandle<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("EntityHandle").field(&self.0.data).finish()
    }
}

impl<T> Clone for EntityHandle<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// A handle to an entity.
pub trait InnerEntityHandleTrait: Send + Sync + 'static {
    /// The entity in this handle.
    fn id(&self) -> Entity;
}

impl<T: Send + Sync + 'static> InnerEntityHandleTrait for InnerEntityHandle<T> {
    fn id(&self) -> Entity {
        self.entity
    }
}

impl<T> Drop for InnerEntityHandle<T> {
    fn drop(&mut self) {
        let _ = self.drop_sender.send(self.entity);
    }
}

impl<T> ContainsEntity for EntityHandle<T> {
    fn entity(&self) -> Entity {
        self.0.entity
    }
}

impl<T> Deref for EntityHandle<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0.data
    }
}

impl<T: Send + Sync + 'static> EntityHandle<T> {
    pub fn id(&self) -> Entity {
        self.0.entity
    }

    pub fn data(&self) -> &T {
        &self.0.data
    }

    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    pub fn weak(&self) -> WeakEntityHandle<T> {
        WeakEntityHandle(Arc::downgrade(&self.0))
    }
}

#[derive(Component, Debug)]
pub struct WeakEntityHandle<T = ()>(Weak<InnerEntityHandle<T>>);

impl<T> WeakEntityHandle<T> {
    pub fn upgrade(&self) -> Option<EntityHandle<T>> {
        self.0.upgrade().map(|value| EntityHandle(value))
    }

    pub fn strong_count(&self) -> usize {
        Weak::strong_count(&self.0)
    }
}

impl<T> Clone for WeakEntityHandle<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(TypePath)]
pub struct InnerEntityHandle<T> {
    pub entity: Entity,
    pub data: T,
    pub(super) drop_sender: Sender<Entity>,
}

pub fn despawn_dropped_entities(world: &mut World) {
    while let Ok(entity) = world.entity_allocator.handle_drop_receiver().try_recv() {
        world.despawn(entity);
    }
}

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
