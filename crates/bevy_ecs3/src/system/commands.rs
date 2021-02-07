use super::SystemId;
use crate::core::{Bundle, Component, DynamicBundle, Entity, World};
use bevy_utils::tracing::{debug, warn};
use std::marker::PhantomData;

/// A [World] mutation
pub trait Command: Send + Sync {
    fn write(self: Box<Self>, world: &mut World);
}

#[derive(Debug)]
pub(crate) struct Spawn<T>
where
    T: DynamicBundle + Send + Sync + 'static,
{
    bundle: T,
}

impl<T> Command for Spawn<T>
where
    T: DynamicBundle + Send + Sync + 'static,
{
    fn write(self: Box<Self>, world: &mut World) {
        world.spawn().insert_bundle(self.bundle);
    }
}

pub(crate) struct SpawnBatch<I>
where
    I: IntoIterator,
    I::Item: Bundle,
{
    bundles_iter: I,
}

impl<I> Command for SpawnBatch<I>
where
    I: IntoIterator + Send + Sync,
    I::Item: Bundle,
{
    fn write(self: Box<Self>, world: &mut World) {
        world.spawn_batch(self.bundles_iter);
    }
}

#[derive(Debug)]
pub(crate) struct Despawn {
    entity: Entity,
}

impl Command for Despawn {
    fn write(self: Box<Self>, world: &mut World) {
        if !world.despawn(self.entity) {
            debug!("Failed to despawn non-existent entity {:?}", self.entity);
        }
    }
}

pub struct InsertBundle<T>
where
    T: DynamicBundle + Send + Sync + 'static,
{
    entity: Entity,
    bundle: T,
}

impl<T> Command for InsertBundle<T>
where
    T: DynamicBundle + Send + Sync + 'static,
{
    fn write(self: Box<Self>, world: &mut World) {
        world
            .entity_mut(self.entity)
            .unwrap()
            .insert_bundle(self.bundle);
    }
}

#[derive(Debug)]
pub(crate) struct Insert<T>
where
    T: Component,
{
    entity: Entity,
    component: T,
}

impl<T> Command for Insert<T>
where
    T: Component,
{
    fn write(self: Box<Self>, world: &mut World) {
        world
            .entity_mut(self.entity)
            .unwrap()
            .insert(self.component);
    }
}

#[derive(Debug)]
pub(crate) struct Remove<T>
where
    T: Component,
{
    entity: Entity,
    phantom: PhantomData<T>,
}

impl<T> Command for Remove<T>
where
    T: Component,
{
    fn write(self: Box<Self>, world: &mut World) {
        if let Some(mut entity_mut) = world.entity_mut(self.entity) {
            entity_mut.remove::<T>();
        }
    }
}

#[derive(Debug)]
pub(crate) struct RemoveBundle<T>
where
    T: Bundle + Send + Sync + 'static,
{
    entity: Entity,
    phantom: PhantomData<T>,
}

impl<T> Command for RemoveBundle<T>
where
    T: Bundle + Send + Sync + 'static,
{
    fn write(self: Box<Self>, world: &mut World) {
        todo!("use entity.remove_bundle_intersection once it is implemented");
        // if let Some(entity_mut) = world.entity_mut(self.entity) {
        //     match entity_mut.remove::<T>() {
        //         Some(_) => (),
        //         None => {
        //             warn!(
        //             "Failed to remove components {:?}. Falling back to inefficient one-by-one component removing.",
        //             std::any::type_name::<T>(),
        //         );
        //             if let Err(e) = world.remove_one_by_one::<T>(self.entity) {
        //                 debug!(
        //                     "Failed to remove components {:?} with error: {}",
        //                     std::any::type_name::<T>(),
        //                     e
        //                 );
        //             }
        //         }
        //         Err(e) => {
        //             debug!(
        //                 "Failed to remove components {:?} with error: {}",
        //                 std::any::type_name::<T>(),
        //                 e
        //             );
        //         }
        //     }
        // }
    }
}

pub trait ResourcesWriter: Send + Sync {
    fn write(self: Box<Self>);
}

/// A list of commands that will be run to populate a `World` and `Resources`.
#[derive(Default)]
pub struct Commands {
    commands: Vec<Box<dyn Command>>,
    current_entity: Option<Entity>,
}

impl Commands {
    /// Creates a new entity with the components contained in `bundle`.
    ///
    /// Note that `bundle` is a [DynamicBundle], which is a collection of components. [DynamicBundle] is automatically implemented for tuples of components. You can also create your own bundle types by deriving [`derive@Bundle`]. If you would like to spawn an entity with a single component, consider wrapping the component in a tuple (which [DynamicBundle] is implemented for).
    ///
    /// See [`Self::set_current_entity`], [`Self::insert`].
    ///
    /// # Example
    ///
    /// ```
    /// use bevy_ecs::prelude::*;
    ///
    /// struct Component1;
    /// struct Component2;
    ///
    /// #[derive(Bundle)]
    /// struct ExampleBundle {
    ///     a: Component1,
    ///     b: Component2,
    /// }
    ///
    /// fn example_system(mut commands: Commands) {
    ///     // Create a new entity with a component bundle.
    ///     commands.spawn(ExampleBundle {
    ///         a: Component1,
    ///         b: Component2,
    ///     });
    ///
    ///     // Create a new entity with a single component.
    ///     commands.spawn((Component1,));
    ///     // Create a new entity with two components.
    ///     commands.spawn((Component1, Component2));
    /// }
    /// ```
    pub fn spawn(&mut self, bundle: impl DynamicBundle + Send + Sync + 'static) -> &mut Self {
        todo!("use direct world ref to reserve entity");
        // let entity = self
        //     .entity_reserver
        //     .as_ref()
        //     .expect("Entity reserver has not been set.")
        //     .reserve_entity();
        // self.set_current_entity(entity);
        // self.insert(entity, bundle);
        // self
    }

    /// Equivalent to iterating `bundles_iter` and calling [`Self::spawn`] on each bundle, but slightly more performant.
    pub fn spawn_batch<I>(&mut self, bundles_iter: I) -> &mut Self
    where
        I: IntoIterator + Send + Sync + 'static,
        I::Item: Bundle,
    {
        self.add_command(SpawnBatch { bundles_iter })
    }

    /// Despawns only the specified entity, not including its children.
    pub fn despawn(&mut self, entity: Entity) -> &mut Self {
        self.add_command(Despawn { entity })
    }

    /// Inserts a bundle of components into `entity`.
    ///
    /// See [`World::insert`].
    pub fn insert(
        &mut self,
        entity: Entity,
        bundle: impl DynamicBundle + Send + Sync + 'static,
    ) -> &mut Self {
        self.add_command(InsertBundle { entity, bundle })
    }

    /// Inserts a single component into `entity`.
    ///
    /// See [`World::insert_one`].
    pub fn insert_one(&mut self, entity: Entity, component: impl Component) -> &mut Self {
        self.add_command(Insert { entity, component })
    }

    /// See [`World::remove_one`].
    pub fn remove_one<T>(&mut self, entity: Entity) -> &mut Self
    where
        T: Component,
    {
        self.add_command(Remove::<T> {
            entity,
            phantom: PhantomData,
        })
    }

    /// See [`World::remove`].
    pub fn remove<T>(&mut self, entity: Entity) -> &mut Self
    where
        T: Bundle + Send + Sync + 'static,
    {
        self.add_command(RemoveBundle::<T> {
            entity,
            phantom: PhantomData,
        })
    }

    /// Adds a bundle of components to the current entity.
    ///
    /// See [`Self::with`], [`Self::current_entity`].
    pub fn with_bundle(&mut self, bundle: impl DynamicBundle + Send + Sync + 'static) -> &mut Self {
        let current_entity =  self.current_entity.expect("Cannot add bundle because the 'current entity' is not set. You should spawn an entity first.");
        self.commands.push(Box::new(InsertBundle {
            entity: current_entity,
            bundle,
        }));
        self
    }

    /// Adds a single component to the current entity.
    ///
    /// See [`Self::with_bundle`], [`Self::current_entity`].
    ///
    /// # Warning
    ///
    /// It's possible to call this with a bundle, but this is likely not intended and [`Self::with_bundle`] should be used instead. If `with` is called with a bundle, the bundle itself will be added as a component instead of the bundles' inner components each being added.
    ///
    /// # Example
    ///
    /// `with` can be chained with [`Self::spawn`].
    ///
    /// ```
    /// use bevy_ecs::prelude::*;
    ///
    /// struct Component1;
    /// struct Component2;
    ///
    /// fn example_system(mut commands: Commands) {
    ///     // Create a new entity with a `Component1` and `Component2`.
    ///     commands.spawn((Component1,)).with(Component2);
    ///
    ///     // Psst! These are also equivalent to the line above!
    ///     commands.spawn((Component1, Component2));
    ///     commands.spawn(()).with(Component1).with(Component2);
    ///     #[derive(Bundle)]
    ///     struct ExampleBundle {
    ///         a: Component1,
    ///         b: Component2,
    ///     }
    ///     commands.spawn(()).with_bundle(ExampleBundle {
    ///         a: Component1,
    ///         b: Component2,
    ///     });
    /// }
    /// ```
    pub fn with(&mut self, component: impl Component) -> &mut Self {
        let current_entity =  self.current_entity.expect("Cannot add component because the 'current entity' is not set. You should spawn an entity first.");
        self.commands.push(Box::new(Insert {
            entity: current_entity,
            component,
        }));
        self
    }

    /// Adds a command directly to the command list. Prefer this to [`Self::add_command_boxed`] if the type of `command` is statically known.
    pub fn add_command<C: Command + 'static>(&mut self, command: C) -> &mut Self {
        self.commands.push(Box::new(command));
        self
    }

    /// See [`Self::add_command`].
    pub fn add_command_boxed(&mut self, command: Box<dyn Command>) -> &mut Self {
        self.commands.push(command);
        self
    }

    /// Runs all the stored commands on `world` and `resources`. The command buffer is emptied as a part of this call.
    pub fn apply(&mut self, world: &mut World) {
        for command in self.commands.drain(..) {
            command.write(world);
        }
    }

    /// Returns the current entity, set by [`Self::spawn`] or with [`Self::set_current_entity`].
    pub fn current_entity(&self) -> Option<Entity> {
        self.current_entity
    }

    pub fn set_current_entity(&mut self, entity: Entity) {
        self.current_entity = Some(entity);
    }

    pub fn clear_current_entity(&mut self) {
        self.current_entity = None;
    }

    pub fn for_current_entity(&mut self, f: impl FnOnce(Entity)) -> &mut Self {
        let current_entity = self
            .current_entity
            .expect("The 'current entity' is not set. You should spawn an entity first.");
        f(current_entity);
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core::{IntoQueryState, World},
        system::Commands,
    };

    #[test]
    fn commands() {
        let mut world = World::default();
        let mut commands = Commands::default();
        commands.spawn((1u32, 2u64));
        let entity = commands.current_entity().unwrap();
        // commands.insert_resource(3.14f32);
        commands.apply(&mut world);
        let results = <(&u32, &u64)>::query()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results, vec![(1u32, 2u64)]);
        // assert_eq!(*resources.get::<f32>().unwrap(), 3.14f32);
        // test entity despawn
        commands.despawn(entity);
        commands.despawn(entity); // double despawn shouldn't panic
        commands.apply(&mut world);
        let results2 = <(&u32, &u64)>::query()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results2, vec![]);
    }

    #[test]
    fn remove_components() {
        let mut world = World::default();
        let mut command_buffer = Commands::default();
        command_buffer.spawn((1u32, 2u64));
        let entity = command_buffer.current_entity().unwrap();
        command_buffer.apply(&mut world);
        let results_before = <(&u32, &u64)>::query()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results_before, vec![(1u32, 2u64)]);

        // test component removal
        command_buffer.remove_one::<u32>(entity);
        command_buffer.remove::<(u32, u64)>(entity);
        command_buffer.apply(&mut world);
        let results_after = <(&u32, &u64)>::query()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results_after, vec![]);
        let results_after_u64 = <&u64>::query().iter(&world).map(|a| *a).collect::<Vec<_>>();
        assert_eq!(results_after_u64, vec![]);
    }
}
