use bevy_ecs::core::{
    Archetype, Component, Entity, EntityMap, FromWorld, MapEntities, MapEntitiesError, World,
};

use crate::{FromType, Reflect};
use std::marker::PhantomData;

#[derive(Clone)]
pub struct ReflectComponent {
    add_component: fn(&mut World, Entity, &dyn Reflect),
    apply_component: fn(&mut World, Entity, &dyn Reflect),
    reflect_component: unsafe fn(&Archetype, usize) -> &dyn Reflect,
    reflect_component_mut: unsafe fn(&Archetype, usize) -> &mut dyn Reflect,
    copy_component: fn(&World, &mut World, Entity, Entity),
}

impl ReflectComponent {
    pub fn add_component(&self, world: &mut World, entity: Entity, component: &dyn Reflect) {
        (self.add_component)(world, entity, component);
    }

    pub fn apply_component(&self, world: &mut World, entity: Entity, component: &dyn Reflect) {
        (self.apply_component)(world, entity, component);
    }

    /// # Safety
    /// This does not do bound checks on entity_index. You must make sure entity_index is within bounds before calling.
    pub unsafe fn reflect_component<'a>(
        &self,
        archetype: &'a Archetype,
        entity_index: usize,
    ) -> &'a dyn Reflect {
        (self.reflect_component)(archetype, entity_index)
    }

    /// # Safety
    /// This does not do bound checks on entity_index. You must make sure entity_index is within bounds before calling.
    /// This method does not prevent you from having two mutable pointers to the same data, violating Rust's aliasing rules. To avoid this:
    /// * Only call this method in a thread-local system to avoid sharing across threads.
    /// * Don't call this method more than once in the same scope for a given component.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn reflect_component_mut<'a>(
        &self,
        archetype: &'a Archetype,
        entity_index: usize,
    ) -> &'a mut dyn Reflect {
        (self.reflect_component_mut)(archetype, entity_index)
    }

    pub fn copy_component(
        &self,
        source_world: &World,
        destination_world: &mut World,
        source_entity: Entity,
        destination_entity: Entity,
    ) {
        (self.copy_component)(
            source_world,
            destination_world,
            source_entity,
            destination_entity,
        );
    }
}

impl<C: Component + Reflect + FromWorld> FromType<C> for ReflectComponent {
    fn from_type() -> Self {
        ReflectComponent {
            add_component: |world, entity, reflected_component| {
                let mut component = C::from_world(world);
                component.apply(reflected_component);
                world.entity_mut(entity).insert(component);
            },
            apply_component: |world, entity, reflected_component| {
                let mut component = world.get_mut::<C>(entity).unwrap();
                component.apply(reflected_component);
            },
            copy_component: |source_world, destination_world, source_entity, destination_entity| {
                let source_component = source_world.get::<C>(source_entity).unwrap();
                let mut destination_component = C::from_world(destination_world);
                destination_component.apply(source_component);
                destination_world
                    .entity_mut(destination_entity)
                    .insert(destination_component);
            },
            reflect_component: |archetype, index| {
                // TODO: fix these impls
                todo!("adapt this to new bevy ecs")
                // unsafe {
                //     // the type has been looked up by the caller, so this is safe
                //     let ptr = archetype.get::<C>().unwrap().as_ptr().add(index);
                //     ptr.as_ref().unwrap()
                // }
            },
            reflect_component_mut: |archetype, index| {
                todo!("adapt this to new bevy ecs")
                // unsafe {
                //     // the type has been looked up by the caller, so this is safe
                //     let ptr = archetype.get::<C>().unwrap().as_ptr().add(index);
                //     &mut *ptr
                // }
            },
        }
    }
}

#[derive(Clone)]
pub struct SceneComponent<Scene: Component, Runtime: Component> {
    copy_scene_to_runtime: fn(&World, &mut World, Entity, Entity),
    marker: PhantomData<(Scene, Runtime)>,
}

impl<Scene: Component + IntoComponent<Runtime>, Runtime: Component> SceneComponent<Scene, Runtime> {
    pub fn copy_scene_to_runtime(
        &self,
        scene_world: &World,
        runtime_world: &mut World,
        scene_entity: Entity,
        runtime_entity: Entity,
    ) {
        (self.copy_scene_to_runtime)(scene_world, runtime_world, scene_entity, runtime_entity);
    }
}

impl<Scene: Component + IntoComponent<Runtime>, Runtime: Component> FromType<Scene>
    for SceneComponent<Scene, Runtime>
{
    fn from_type() -> Self {
        SceneComponent {
            copy_scene_to_runtime: |scene_world, runtime_world, scene_entity, runtime_entity| {
                let scene_component = scene_world.get::<Scene>(scene_entity).unwrap();
                let destination_component = scene_component.into_component(runtime_world);
                runtime_world
                    .entity_mut(runtime_entity)
                    .insert(destination_component);
            },
            marker: Default::default(),
        }
    }
}

#[derive(Clone)]
pub struct RuntimeComponent<Runtime: Component, Scene: Component> {
    copy_runtime_to_scene: fn(&World, &mut World, Entity, Entity),
    marker: PhantomData<(Runtime, Scene)>,
}

impl<Runtime: Component + IntoComponent<Scene>, Scene: Component> RuntimeComponent<Runtime, Scene> {
    pub fn copy_runtime_to_scene(
        &self,
        runtime_world: &World,
        scene_world: &mut World,
        runtime_entity: Entity,
        scene_entity: Entity,
    ) {
        (self.copy_runtime_to_scene)(runtime_world, scene_world, runtime_entity, scene_entity);
    }
}

impl<Runtime: Component + IntoComponent<Scene>, Scene: Component> FromType<Runtime>
    for RuntimeComponent<Runtime, Scene>
{
    fn from_type() -> Self {
        RuntimeComponent {
            copy_runtime_to_scene: |runtime_world, scene_world, runtime_entity, scene_entity| {
                let runtime_component = runtime_world.get::<Runtime>(runtime_entity).unwrap();
                let scene_component = runtime_component.into_component(runtime_world);
                scene_world.entity_mut(scene_entity).insert(scene_component);
            },
            marker: Default::default(),
        }
    }
}

#[derive(Clone)]
pub struct ReflectMapEntities {
    map_entities: fn(&mut World, &EntityMap) -> Result<(), MapEntitiesError>,
}

impl ReflectMapEntities {
    pub fn map_entities(
        &self,
        world: &mut World,
        entity_map: &EntityMap,
    ) -> Result<(), MapEntitiesError> {
        (self.map_entities)(world, entity_map)
    }
}

impl<C: Component + MapEntities> FromType<C> for ReflectMapEntities {
    fn from_type() -> Self {
        ReflectMapEntities {
            map_entities: |world, entity_map| {
                for entity in entity_map.values() {
                    if let Some(mut component) = world.get_mut::<C>(entity) {
                        component.map_entities(entity_map)?;
                    }
                }

                Ok(())
            },
        }
    }
}

pub trait IntoComponent<ToComponent: Component> {
    fn into_component(&self, world: &World) -> ToComponent;
}
