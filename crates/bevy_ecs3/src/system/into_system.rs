use crate::{
    core::{Access, ArchetypeComponentId, ComponentId, World},
    system::{System, SystemId, SystemParam, SystemParamFetch, SystemParamState},
};
use bevy_ecs3_macros::all_tuples;
use std::{borrow::Cow, marker::PhantomData};

pub struct SystemState {
    pub(crate) id: SystemId,
    pub(crate) name: Cow<'static, str>,
    pub(crate) component_access: Access<ComponentId>,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) is_non_send: bool,
}

impl SystemState {
    fn new<T>() -> Self {
        Self {
            name: std::any::type_name::<T>().into(),
            archetype_component_access: Access::default(),
            component_access: Access::default(),
            is_non_send: false,
            id: SystemId::new(),
        }
    }
}

pub trait IntoSystem<Params, SystemType: System> {
    fn system(self) -> SystemType;
}

// Systems implicitly implement IntoSystem
impl<Sys: System> IntoSystem<(), Sys> for Sys {
    fn system(self) -> Sys {
        self
    }
}

pub struct In<In>(pub In);
struct InputMarker;

pub struct FunctionSystem<In, Out, Param, Marker, F>
where
    Param: SystemParam,
{
    func: F,
    param_state: Option<Param::State>,
    system_state: SystemState,
    marker: PhantomData<(In, Out, Marker)>,
}

impl<In, Out, Param, Marker, F> IntoSystem<Param, FunctionSystem<In, Out, Param, Marker, F>> for F
where
    In: Send + Sync + 'static,
    Out: Send + Sync + 'static,
    Param: SystemParam + Send + Sync + 'static,
    Marker: Send + Sync + 'static,
    F: SystemFunction<In, Out, Param, Marker> + Send + Sync + 'static,
{
    fn system(self) -> FunctionSystem<In, Out, Param, Marker, F> {
        FunctionSystem {
            func: self,
            param_state: None,
            system_state: SystemState::new::<F>(),
            marker: PhantomData,
        }
    }
}

impl<In, Out, Param, Marker, F> System for FunctionSystem<In, Out, Param, Marker, F>
where
    In: Send + Sync + 'static,
    Out: Send + Sync + 'static,
    Param: SystemParam + Send + Sync + 'static,
    Marker: Send + Sync + 'static,
    F: SystemFunction<In, Out, Param, Marker> + Send + Sync + 'static,
{
    type In = In;
    type Out = Out;

    #[inline]
    fn name(&self) -> Cow<'static, str> {
        self.system_state.name.clone()
    }

    #[inline]
    fn id(&self) -> SystemId {
        self.system_state.id
    }

    #[inline]
    fn update(&mut self, world: &World) {
        let param_state = self.param_state.as_mut().unwrap();
        param_state.update(world, &mut self.system_state);
    }

    #[inline]
    fn component_access(&self) -> &Access<ComponentId> {
        &self.system_state.component_access
    }

    #[inline]
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.system_state.archetype_component_access
    }

    #[inline]
    fn is_non_send(&self) -> bool {
        self.system_state.is_non_send
    }

    #[inline]
    unsafe fn run_unsafe(&mut self, input: Self::In, world: &World) -> Option<Self::Out> {
        self.func.run(
            input,
            self.param_state.as_mut().unwrap(),
            &self.system_state,
            world,
        )
    }

    #[inline]
    fn apply_buffers(&mut self, world: &mut World) {
        let param_state = self.param_state.as_mut().unwrap();
        param_state.apply(world);
    }

    #[inline]
    fn initialize(&mut self, world: &mut World) {
        self.param_state = Some(<Param::State as SystemParamState>::init(
            world,
            &mut self.system_state,
        ));
    }
}

pub trait SystemFunction<In, Out, Param: SystemParam, Marker> {
    fn run(
        &mut self,
        input: In,
        state: &mut Param::State,
        system_state: &SystemState,
        world: &World,
    ) -> Option<Out>;
}

macro_rules! impl_system_function {
    ($($param: ident),*) => {
        #[allow(non_snake_case)]
        impl<Out, Func, $($param: SystemParam),*> SystemFunction<(), Out, ($($param,)*), ()> for Func
        where
            Func:
                FnMut($($param),*) -> Out +
                FnMut($(<<$param as SystemParam>::State as SystemParamFetch>::Item),*) -> Out +
                Send + Sync + 'static, Out: 'static
        {
            #[inline]
            fn run(&mut self, _input: (), state: &mut <($($param,)*) as SystemParam>::State, system_state: &SystemState, world: &World) -> Option<Out> {
                unsafe {
                    if let Some(($($param,)*)) = <<($($param,)*) as SystemParam>::State as SystemParamFetch>::get_param(state, system_state, world) {
                        Some(self($($param),*))
                    } else {
                        None
                    }
                }
            }
        }

        #[allow(non_snake_case)]
        impl<Input, Out, Func, $($param: SystemParam),*> SystemFunction<Input, Out, ($($param,)*), InputMarker> for Func
        where
            Func:
                FnMut(In<Input>, $($param),*) -> Out +
                FnMut(In<Input>, $(<<$param as SystemParam>::State as SystemParamFetch>::Item),*) -> Out +
                Send + Sync + 'static, Out: 'static
        {
            #[inline]
            fn run(&mut self, input: Input, state: &mut <($($param,)*) as SystemParam>::State, system_state: &SystemState, world: &World) -> Option<Out> {
                unsafe {
                    if let Some(($($param,)*)) = <<($($param,)*) as SystemParam>::State as SystemParamFetch>::get_param(state, system_state, world) {
                        Some(self(In(input), $($param),*))
                    } else {
                        None
                    }
                }
            }
        }
    };
}

all_tuples!(impl_system_function, 0, 16, F);