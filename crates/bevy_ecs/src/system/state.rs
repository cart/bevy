use std::borrow::Cow;

use crate::{archetype::{Archetype, ArchetypeComponentId, ArchetypeGeneration, ArchetypeId}, component::ComponentId, prelude::{System, World}, query::Access, system::{ReadOnlySystemParamFetch, SystemId, SystemMeta, SystemParam, SystemParamFetch, SystemParamItem, SystemParamState}, world::WorldId};

// TODO: Actually use this in FunctionSystem. We should probably only do this once Systems are constructed using a World reference
// (to avoid the need for unwrapping to retrieve SystemMeta)
/// Holds on to persistent state required to drive [`SystemParam`] for a [`System`].  
pub struct SystemState<Param: SystemParam> {
    meta: SystemMeta,
    param_state: <Param as SystemParam>::Fetch,
    world_id: WorldId,
    archetype_generation: ArchetypeGeneration,
}

impl<Param: SystemParam> SystemState<Param> {
    pub fn new(world: &mut World) -> Self {
        let config = <Param::Fetch as SystemParamState>::default_config();
        Self::with_config(world, config)
    }

    pub fn with_config(
        world: &mut World,
        config: <Param::Fetch as SystemParamState>::Config,
    ) -> Self {
        let mut meta = SystemMeta::new::<Param>();
        let param_state = <Param::Fetch as SystemParamState>::init(world, &mut meta, config);
        Self {
            meta,
            param_state,
            world_id: world.id(),
            archetype_generation: ArchetypeGeneration::initial(),
        }
    }

    #[inline]
    pub fn meta(&self) -> &SystemMeta {
        &self.meta
    }

    /// Retrieve the [`SystemParam`] values. This can only be called when all parameters are read-only.
    #[inline]
    pub fn get<'s, 'w>(
        &'s mut self,
        world: &'w World,
    ) -> <Param::Fetch as SystemParamFetch<'s, 'w>>::Item
    where
        Param::Fetch: ReadOnlySystemParamFetch,
    {
        self.validate_world_and_update_archetypes(world);
        // SAFE: Param is read-only and doesn't allow mutable access to World. It also matches the World this SystemState was created with.
        unsafe { self.get_unchecked_manual(world) }
    }

    /// Retrieve the mutable [`SystemParam`] values.
    #[inline]
    pub fn get_mut<'s, 'w>(
        &'s mut self,
        world: &'w mut World,
    ) -> <Param::Fetch as SystemParamFetch<'s, 'w>>::Item {
        self.validate_world_and_update_archetypes(world);
        // SAFE: World is uniquely borrowed and matches the World this SystemState was created with.
        unsafe { self.get_unchecked_manual(world) }
    }

    /// Applies all state queued up for [`SystemParam`] values. For example, this will apply commands queued up
    /// by a [`Commands`](`super::Commands`) parameter to the given [`World`].
    /// This function should be called manually after the values returned by [`SystemState::get`] and [`SystemState::get_mut`]  
    /// are finished being used.
    pub fn apply(&mut self, world: &mut World) {
        self.param_state.apply(world);
    }

    #[inline]
    pub fn matches_world(&self, world: &World) -> bool {
        self.world_id == world.id()
    }

    fn validate_world_and_update_archetypes(&mut self, world: &World) {
        assert!(self.matches_world(world), "Encountered a mismatched World. A SystemState cannot be used with Worlds other than the one it was created with.");
        let archetypes = world.archetypes();
        let new_generation = archetypes.generation();
        let old_generation = std::mem::replace(&mut self.archetype_generation, new_generation);
        let archetype_index_range = old_generation.value()..new_generation.value();

        for archetype_index in archetype_index_range {
            self.param_state.new_archetype(
                &archetypes[ArchetypeId::new(archetype_index)],
                &mut self.meta,
            );
        }
    }

    pub(crate) fn new_archetype(&mut self, archetype: &Archetype) {
        self.param_state.new_archetype(archetype, &mut self.meta);
    }

    /// Retrieve the [`SystemParam`] values. This will not update archetypes automatically.
    ///
    /// # Safety
    /// This call might access any of the input parameters in a way that violates Rust's mutability rules. Make sure the data
    /// access is safe in the context of global [`World`] access. The passed-in [`World`] _must_ be the [`World`] the [`SystemState`] was
    /// created with.   
    #[inline]
    pub unsafe fn get_unchecked_manual<'s, 'w>(
        &'s mut self,
        world: &'w World,
    ) -> <Param::Fetch as SystemParamFetch<'s, 'w>>::Item {
        let change_tick = world.increment_change_tick();
        let param = <Param::Fetch as SystemParamFetch>::get_param(
            &mut self.param_state,
            &self.meta,
            world,
            change_tick,
        );
        self.meta.last_change_tick = change_tick;
        param
    }
}

pub trait StateSystem: Send + Sync + 'static {
    type Param: SystemParam;
    fn system_state(&self) -> &SystemState<Self::Param>;
    fn system_state_mut(&mut self) -> &mut SystemState<Self::Param>;
    fn run(param: SystemParamItem<Self::Param>);
}

impl<S: StateSystem> System for S {
    type In = ();

    type Out = ();

    fn name(&self) -> Cow<'static, str> {
        self.system_state().meta().name.clone()
    }

    fn id(&self) -> SystemId {
        self.system_state().meta().id
    }

    fn new_archetype(&mut self, archetype: &Archetype) {
        self.system_state_mut().new_archetype(archetype);
    }

    fn component_access(&self) -> &Access<ComponentId> {
        &self
            .system_state()
            .meta()
            .component_access_set
            .combined_access()
    }

    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.system_state().meta().archetype_component_access
    }

    fn is_send(&self) -> bool {
        self.system_state().meta().is_send()
    }

    unsafe fn run_unsafe(&mut self, _input: Self::In, world: &World) -> Self::Out {
        let param = self.system_state_mut().get_unchecked_manual(world);
        <Self as StateSystem>::run(param);
    }

    fn apply_buffers(&mut self, world: &mut World) {
        self.system_state_mut().apply(world);
    }

    fn initialize(&mut self, _world: &mut World) {
        // already initialized by nature of the SystemState being constructed
    }

    fn check_change_tick(&mut self, change_tick: u32) {
        self.system_state_mut().meta.check_change_tick(change_tick);
    }
}
