use crate::{
    component::ComponentId,
    entity::Entity,
    event::Event,
    world::{Command, DeferredWorld, World},
};

/// A [`Command`] that emits a given trigger for a given set of targets.
pub struct TriggerEvent<E: Event, Components: Iterator<Item = ComponentId>> {
    /// The event to trigger.
    pub event: E,

    /// The components to trigger the event for.
    pub components: Components,
}

impl<E: Event<Target = ()>, Components: Iterator<Item = ComponentId> + Send + 'static> Command
    for TriggerEvent<E, Components>
{
    fn apply(mut self, world: &mut World) {
        let event_type = world.init_component::<E>();
        trigger_event(world, event_type, &mut self.event, self.components);
    }
}

/// A [`Command`] that emits a given trigger for a given set of targets.
pub struct TriggerEventWithTargets<
    E: Event<Target = Entity>,
    Components: AsIter<ComponentId>,
    Targets: AsIter<E::Target>,
> {
    /// The event to trigger.
    pub event: E,

    /// The components to trigger the event for.
    pub components: Components,

    /// The targets to trigger the event for.
    pub targets: Targets,
}

impl<E: Event<Target = Entity>, Components: AsIter<ComponentId>, Targets: AsIter<Entity>> Command
    for TriggerEventWithTargets<E, Components, Targets>
{
    fn apply(mut self, world: &mut World) {
        let event_type = world.init_component::<E>();
        trigger_event_with_entities(
            world,
            event_type,
            &mut self.event,
            self.components,
            self.targets.as_iter(),
        );
    }
}

// TODO: Dynamic
// /// Emit a trigger for a dynamic component id. This is unsafe and must be verified manually.
// pub struct EmitDynamicTrigger<T, Targets: TriggerTargets = ()> {
//     event_type: ComponentId,
//     event_data: T,
//     targets: Targets,
// }

// impl<E, Targets: TriggerTargets> EmitDynamicTrigger<E, Targets> {
//     /// Sets the event type of the resulting trigger, used for dynamic triggers
//     /// # Safety
//     /// Caller must ensure that the component associated with `event_type` is accessible as E
//     pub unsafe fn new_with_id(event_type: ComponentId, event_data: E, targets: Targets) -> Self {
//         Self {
//             event_type,
//             event_data,
//             targets,
//         }
//     }
// }

// impl<E: Event, Targets: TriggerTargets> Command for EmitDynamicTrigger<E, Targets> {
//     fn apply(mut self, world: &mut World) {
//         trigger_event(world, self.event_type, &mut self.event_data, self.targets);
//     }
// }

#[inline]
fn trigger_event<E: Event<Target = ()>>(
    world: &mut World,
    event_type: ComponentId,
    event_data: &mut E,
    components: impl Iterator<Item = ComponentId>,
) {
    let mut world = DeferredWorld::from(world);

    // SAFETY: T is accessible as the type represented by self.trigger, ensured in `Self::new`
    unsafe {
        world.trigger_observers(event_type, event_data, components);
    };
}

#[inline]
fn trigger_event_with_entities<E: Event<Target = Entity>>(
    world: &mut World,
    event_type: ComponentId,
    event_data: &mut E,
    components: impl AsIter<ComponentId>,
    targets: impl Iterator<Item = Entity>,
) {
    let mut world = DeferredWorld::from(world);

    for target in targets {
        // SAFETY: T is accessible as the type represented by self.trigger, ensured in `Self::new`
        unsafe {
            world.trigger_entity_observers(event_type, event_data, components.as_iter(), target);
        };
    }
}

pub trait AsIter<T>: Send + Sync + 'static {
    fn as_iter(&self) -> impl Iterator<Item = T>;
}

impl<T: Clone + Send + Sync + 'static> AsIter<T> for T {
    fn as_iter(&self) -> impl Iterator<Item = T> {
        std::iter::once(self.clone())
    }
}

impl<T: Clone + Send + Sync + 'static> AsIter<T> for std::iter::Empty<T> {
    fn as_iter(&self) -> impl Iterator<Item = T> {
        std::iter::empty()
    }
}

impl<T: Clone + Send + Sync + 'static> AsIter<T> for Vec<T> {
    fn as_iter(&self) -> impl Iterator<Item = T> {
        self.iter().cloned()
    }
}

impl<const N: usize, T: Clone + Send + Sync + 'static> AsIter<T> for [T; N] {
    fn as_iter(&self) -> impl Iterator<Item = T> {
        self.iter().cloned()
    }
}
