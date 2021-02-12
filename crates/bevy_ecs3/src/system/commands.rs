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
        // TODO: use entity.remove_bundle_intersection once it is implemented"
        if let Some(mut entity_mut) = world.entity_mut(self.entity) {
            match entity_mut.remove::<T>() {
                Some(_) => (),
                None => {
                    warn!(
                    "Failed to remove components {:?}. Falling back to inefficient one-by-one component removing.",
                    std::any::type_name::<T>(),
                    )
                }
            }
        }
    }
}

pub trait ResourcesWriter: Send + Sync {
    fn write(self: Box<Self>);
}

#[derive(Default)]
pub struct CommandQueue {
    commands: Vec<Box<dyn Command>>,
}

impl CommandQueue {
    pub fn apply(&mut self, world: &mut World) {
        world.flush();
        for command in self.commands.drain(..) {
            command.write(world);
        }
    }

    #[inline]
    pub fn push(&mut self, command: Box<dyn Command>) {
        self.commands.push(command);
    }
}

/// A list of commands that will be run to populate a `World` and `Resources`.
pub struct Commands<'a> {
    queue: &'a mut CommandQueue,
    world: &'a World,
    current_entity: Option<Entity>,
}

impl<'a> Commands<'a> {
    pub fn new(queue: &'a mut CommandQueue, world: &'a World) -> Self {
        Self {
            queue,
            world,
            current_entity: None,
        }
    }

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
        let entity = self.world.entities().reserve_entity();
        self.set_current_entity(entity);
        self.insert_bundle(entity, bundle);
        self
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
    pub fn insert_bundle(
        &mut self,
        entity: Entity,
        bundle: impl DynamicBundle + Send + Sync + 'static,
    ) -> &mut Self {
        self.add_command(InsertBundle { entity, bundle })
    }

    /// Inserts a single component into `entity`.
    ///
    /// See [`World::insert_one`].
    pub fn insert(&mut self, entity: Entity, component: impl Component) -> &mut Self {
        self.add_command(Insert { entity, component })
    }

    /// See [`World::remove_one`].
    pub fn remove<T>(&mut self, entity: Entity) -> &mut Self
    where
        T: Component,
    {
        self.add_command(Remove::<T> {
            entity,
            phantom: PhantomData,
        })
    }

    /// See [`World::remove`].
    pub fn remove_bundle<T>(&mut self, entity: Entity) -> &mut Self
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
        self.queue.push(Box::new(InsertBundle {
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
        self.queue.push(Box::new(Insert {
            entity: current_entity,
            component,
        }));
        self
    }

    /// Adds a command directly to the command list. Prefer this to [`Self::add_command_boxed`] if the type of `command` is statically known.
    pub fn add_command<C: Command + 'static>(&mut self, command: C) -> &mut Self {
        self.queue.push(Box::new(command));
        self
    }

    /// See [`Self::add_command`].
    pub fn add_command_boxed(&mut self, command: Box<dyn Command>) -> &mut Self {
        self.queue.push(command);
        self
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
        core::World,
        system::{CommandQueue, Commands},
    };

    #[test]
    fn commands() {
        let mut world = World::default();
        let mut command_queue = CommandQueue::default();
        let entity = Commands::new(&mut command_queue, &world)
            .spawn((1u32, 2u64))
            .current_entity()
            .unwrap();
        // commands.insert_resource(3.14f32);
        command_queue.apply(&mut world);
        assert!(world.entities().len() == 1);
        let results = world
            .query::<(&u32, &u64)>()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results, vec![(1u32, 2u64)]);
        // assert_eq!(*resources.get::<f32>().unwrap(), 3.14f32);
        // test entity despawn
        Commands::new(&mut command_queue, &world)
            .despawn(entity)
            .despawn(entity); // double despawn shouldn't panic
        command_queue.apply(&mut world);
        let results2 = world
            .query::<(&u32, &u64)>()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results2, vec![]);
    }

    #[test]
    fn remove_components() {
        let mut world = World::default();
        let mut command_queue = CommandQueue::default();
        let entity = Commands::new(&mut command_queue, &world)
            .spawn((1u32, 2u64))
            .current_entity()
            .unwrap();
        command_queue.apply(&mut world);
        let results_before = world
            .query::<(&u32, &u64)>()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results_before, vec![(1u32, 2u64)]);

        // test component removal
        Commands::new(&mut command_queue, &world)
            .remove::<u32>(entity)
            .remove_bundle::<(u32, u64)>(entity);
        command_queue.apply(&mut world);
        let results_after = world
            .query::<(&u32, &u64)>()
            .iter(&world)
            .map(|(a, b)| (*a, *b))
            .collect::<Vec<_>>();
        assert_eq!(results_after, vec![]);
        let results_after_u64 = world
            .query::<&u64>()
            .iter(&world)
            .map(|a| *a)
            .collect::<Vec<_>>();
        assert_eq!(results_after_u64, vec![]);
    }
}