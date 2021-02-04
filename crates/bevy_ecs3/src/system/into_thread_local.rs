pub use super::Query;
use crate::{
    core::{Access, ArchetypeComponentId, World},
    system::{IntoSystem, System, SystemId, ThreadLocalExecution},
};
use std::borrow::Cow;

pub struct ThreadLocalSystemFn {
    pub func: Box<dyn FnMut(&mut World) + Send + Sync + 'static>,
    pub archetype_component_access: Access<ArchetypeComponentId>,
    pub name: Cow<'static, str>,
    pub id: SystemId,
}

impl System for ThreadLocalSystemFn {
    type In = ();
    type Out = ();

    fn name(&self) -> Cow<'static, str> {
        self.name.clone()
    }

    fn update(&mut self, _world: &World) {}

    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.archetype_component_access
    }

    fn thread_local_execution(&self) -> ThreadLocalExecution {
        ThreadLocalExecution::Immediate
    }

    unsafe fn run_unsafe(&mut self, _input: (), _world: &World) -> Option<()> {
        Some(())
    }

    fn run_thread_local(&mut self, world: &mut World) {
        (self.func)(world);
    }

    fn initialize(&mut self, _world: &mut World) {}

    fn id(&self) -> SystemId {
        self.id
    }
}

impl<F> IntoSystem<&mut World, ThreadLocalSystemFn> for F
where
    F: FnMut(&mut World) + Send + Sync + 'static,
{
    fn system(mut self) -> ThreadLocalSystemFn {
        ThreadLocalSystemFn {
            func: Box::new(move |world| (self)(world)),
            name: core::any::type_name::<F>().into(),
            id: SystemId::new(),
            archetype_component_access: Access::default(),
        }
    }
}
