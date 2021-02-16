use std::borrow::Cow;

use crate::{
    core::{Access, ArchetypeComponentId, ComponentId, World},
    system::{System, SystemId},
};

pub struct ChainSystem<SystemA, SystemB> {
    system_a: SystemA,
    system_b: SystemB,
    name: Cow<'static, str>,
    id: SystemId,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
}

impl<SystemA: System, SystemB: System<In = SystemA::Out>> System for ChainSystem<SystemA, SystemB> {
    type In = SystemA::In;
    type Out = SystemB::Out;

    fn name(&self) -> Cow<'static, str> {
        self.name.clone()
    }

    fn id(&self) -> SystemId {
        self.id
    }

    fn update(&mut self, world: &World) {
        self.archetype_component_access.clear();
        self.component_access.clear();

        self.system_a.update(world);
        self.system_b.update(world);

        self.archetype_component_access
            .extend(self.system_a.archetype_component_access());
        self.archetype_component_access
            .extend(self.system_b.archetype_component_access());
        self.component_access
            .extend(self.system_a.component_access());
        self.component_access
            .extend(self.system_b.component_access());
    }

    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.archetype_component_access
    }

    fn component_access(&self) -> &Access<ComponentId> {
        &self.component_access
    }

    fn is_send(&self) -> bool {
        self.system_a.is_send() && self.system_b.is_send()
    }

    unsafe fn run_unsafe(&mut self, input: Self::In, world: &World) -> Option<Self::Out> {
        let out = self.system_a.run_unsafe(input, world).unwrap();
        self.system_b.run_unsafe(out, world)
    }

    fn apply_buffers(&mut self, world: &mut World) {
        self.system_a.apply_buffers(world);
        self.system_b.apply_buffers(world);
    }

    fn initialize(&mut self, world: &mut World) {
        self.system_a.initialize(world);
        self.system_b.initialize(world);
    }
}

pub trait IntoChainSystem<SystemB>: System + Sized
where
    SystemB: System<In = Self::Out>,
{
    fn chain(self, system: SystemB) -> ChainSystem<Self, SystemB>;
}

impl<SystemA, SystemB> IntoChainSystem<SystemB> for SystemA
where
    SystemA: System,
    SystemB: System<In = SystemA::Out>,
{
    fn chain(self, system: SystemB) -> ChainSystem<SystemA, SystemB> {
        ChainSystem {
            name: Cow::Owned(format!("Chain({}, {})", self.name(), system.name())),
            system_a: self,
            system_b: system,
            archetype_component_access: Default::default(),
            component_access: Default::default(),
            id: SystemId::new(),
        }
    }
}
