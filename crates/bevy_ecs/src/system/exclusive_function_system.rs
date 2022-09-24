use crate::{
    archetype::{ArchetypeComponentId, ArchetypeGeneration, ArchetypeId},
    change_detection::MAX_CHANGE_AGE,
    component::ComponentId,
    query::Access,
    schedule::{SystemLabel, SystemLabelId},
    system::{
        check_system_change_tick, AsSystemLabel, ExclusiveSystemParam, ExclusiveSystemParamFetch,
        ExclusiveSystemParamItem, ExclusiveSystemParamState, IntoSystem, System, SystemMeta,
        SystemTypeIdLabel,
    },
    world::{World, WorldId},
};
use bevy_ecs_macros::all_tuples;
use std::{borrow::Cow, marker::PhantomData};

/// A function system that runs with exclusive [`World`] access.
///
/// You get this by calling [`IntoSystem::into_system`]  on a function that only accepts
/// [`ExclusiveSystemParam`]s. The output of the system becomes the functions return type, while the input
/// becomes the functions [`In`] tagged parameter or `()` if no such parameter exists.
///
/// [`ExclusiveFunctionSystem`] must be `.initialized` before they can be run.
pub struct ExclusiveFunctionSystem<In, Out, Param, Marker, F>
where
    Param: ExclusiveSystemParam,
{
    func: F,
    param_state: Option<Param::Fetch>,
    system_meta: SystemMeta,
    world_id: Option<WorldId>,
    archetype_generation: ArchetypeGeneration,
    // NOTE: PhantomData<fn()-> T> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> (In, Out, Marker)>,
}

pub struct IsExclusiveFunctionSystem;

impl<In, Out, Param, Marker, F> IntoSystem<In, Out, (IsExclusiveFunctionSystem, Param, Marker)>
    for F
where
    In: 'static,
    Out: 'static,
    Param: ExclusiveSystemParam + 'static,
    Marker: 'static,
    F: ExclusiveSystemParamFunction<In, Out, Param, Marker> + Send + Sync + 'static,
{
    type System = ExclusiveFunctionSystem<In, Out, Param, Marker, F>;
    fn into_system(func: Self) -> Self::System {
        ExclusiveFunctionSystem {
            func,
            param_state: None,
            system_meta: SystemMeta::new::<F>(),
            world_id: None,
            archetype_generation: ArchetypeGeneration::initial(),
            marker: PhantomData,
        }
    }
}

const PARAM_MESSAGE: &str = "System's param_state was not found. Did you forget to initialize this system before running it?";

impl<In, Out, Param, Marker, F> System for ExclusiveFunctionSystem<In, Out, Param, Marker, F>
where
    In: 'static,
    Out: 'static,
    Param: ExclusiveSystemParam + 'static,
    Marker: 'static,
    F: ExclusiveSystemParamFunction<In, Out, Param, Marker> + Send + Sync + 'static,
{
    type In = In;
    type Out = Out;

    #[inline]
    fn name(&self) -> Cow<'static, str> {
        self.system_meta.name.clone()
    }

    #[inline]
    fn component_access(&self) -> &Access<ComponentId> {
        self.system_meta.component_access_set.combined_access()
    }

    #[inline]
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.system_meta.archetype_component_access
    }

    #[inline]
    fn is_send(&self) -> bool {
        self.system_meta.is_send()
    }

    #[inline]
    unsafe fn run_unsafe(&mut self, _input: Self::In, _world: &World) -> Self::Out {
        panic!("Cannot run exclusive systems with a shared World reference");
    }

    fn run(&mut self, input: Self::In, world: &mut World) -> Self::Out {
        let change_tick = world.increment_change_tick();

        let params = <Param as ExclusiveSystemParam>::Fetch::get_param(
            self.param_state.as_mut().expect(PARAM_MESSAGE),
            &self.system_meta,
            change_tick,
        );
        let out = self.func.run(input, world, params);
        self.system_meta.last_change_tick = change_tick;
        out
    }

    #[inline]
    fn is_exclusive(&self) -> bool {
        true
    }

    fn get_last_change_tick(&self) -> u32 {
        self.system_meta.last_change_tick
    }

    fn set_last_change_tick(&mut self, last_change_tick: u32) {
        self.system_meta.last_change_tick = last_change_tick;
    }

    #[inline]
    fn apply_buffers(&mut self, world: &mut World) {
        let param_state = self.param_state.as_mut().expect(PARAM_MESSAGE);
        param_state.apply(world);
    }

    #[inline]
    fn initialize(&mut self, world: &mut World) {
        self.world_id = Some(world.id());
        self.system_meta.last_change_tick = world.change_tick().wrapping_sub(MAX_CHANGE_AGE);
        self.param_state = Some(<Param::Fetch as ExclusiveSystemParamState>::init(
            world,
            &mut self.system_meta,
        ));
    }

    fn update_archetype_component_access(&mut self, world: &World) {
        assert!(self.world_id == Some(world.id()), "Encountered a mismatched World. A System cannot be used with Worlds other than the one it was initialized with.");
        let archetypes = world.archetypes();
        let new_generation = archetypes.generation();
        let old_generation = std::mem::replace(&mut self.archetype_generation, new_generation);
        let archetype_index_range = old_generation.value()..new_generation.value();

        for archetype_index in archetype_index_range {
            self.param_state.as_mut().unwrap().new_archetype(
                &archetypes[ArchetypeId::new(archetype_index)],
                &mut self.system_meta,
            );
        }
    }

    #[inline]
    fn check_change_tick(&mut self, change_tick: u32) {
        check_system_change_tick(
            &mut self.system_meta.last_change_tick,
            change_tick,
            self.system_meta.name.as_ref(),
        );
    }
    fn default_labels(&self) -> Vec<SystemLabelId> {
        vec![self.func.as_system_label().as_label()]
    }
}

impl<
        In,
        Out,
        Param: ExclusiveSystemParam,
        Marker,
        T: ExclusiveSystemParamFunction<In, Out, Param, Marker>,
    > AsSystemLabel<(In, Out, Param, Marker, IsExclusiveFunctionSystem)> for T
{
    #[inline]
    fn as_system_label(&self) -> SystemLabelId {
        SystemTypeIdLabel::<T>(PhantomData).as_label()
    }
}

/// A trait implemented for all exclusive system functions that can be used as [`System`]s.
///
/// This trait can be useful for making your own systems which accept other systems,
/// sometimes called higher order systems.
pub trait ExclusiveSystemParamFunction<In, Out, Param: ExclusiveSystemParam, Marker>:
    Send + Sync + 'static
{
    fn run(
        &mut self,
        input: In,
        world: &mut World,
        param_value: ExclusiveSystemParamItem<Param>,
    ) -> Out;
}

macro_rules! impl_exclusive_system_function {
    ($($param: ident),*) => {
        #[allow(non_snake_case)]
        impl<Out, Func: Send + Sync + 'static, $($param: ExclusiveSystemParam),*> ExclusiveSystemParamFunction<(), Out, ($($param,)*), ()> for Func
        where
        for <'a> &'a mut Func:
                FnMut(&mut World, $($param),*) -> Out +
                FnMut(&mut World, $(ExclusiveSystemParamItem<$param>),*) -> Out, Out: 'static
        {
            #[inline]
            fn run(&mut self, _input: (), world: &mut World, param_value: ExclusiveSystemParamItem< ($($param,)*)>) -> Out {
                // Yes, this is strange, but `rustc` fails to compile this impl
                // without using this function. It fails to recognise that `func`
                // is a function, potentially because of the multiple impls of `FnMut`
                #[allow(clippy::too_many_arguments)]
                fn call_inner<Out, $($param,)*>(
                    mut f: impl FnMut(&mut World, $($param,)*)->Out,
                    world: &mut World,
                    $($param: $param,)*
                )->Out{
                    f(world, $($param,)*)
                }
                let ($($param,)*) = param_value;
                call_inner(self, world, $($param),*)
            }
        }
    };
}
// Note that we rely on the highest impl to be <= the highest order of the tuple impls
// of `SystemParam` created.
all_tuples!(impl_exclusive_system_function, 0, 16, F);
