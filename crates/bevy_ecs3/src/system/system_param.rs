use crate::{
    core::{
        Component, ComponentFlags, ComponentId, Or, QueryFilter, QueryState, World, WorldQuery,
    },
    system::{CommandQueue, Commands, Query, SystemState},
};
use std::{
    any::TypeId,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

pub trait SystemParam: Sized {
    type State: SystemParamState;
    type Fetch: for<'a> FetchSystemParam<'a, State = Self::State>;
}

pub trait SystemParamState: Send + Sync + 'static {
    fn init(world: &mut World) -> Self;
    #[inline]
    fn update(&mut self, _world: &World, _system_state: &mut SystemState) {}
    #[inline]
    fn apply(&mut self, _world: &mut World) {}
}

pub trait FetchSystemParam<'a> {
    type Item;
    type State: 'a;
    /// # Safety
    /// This call might access any of the input parameters in an unsafe way. Make sure the data access is safe in
    /// the context of the system scheduler
    unsafe fn get_param(
        state: &'a mut Self::State,
        system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item>;
}

pub struct FetchQuery<Q, F>(PhantomData<(Q, F)>);

impl<'a, Q: WorldQuery + 'static, F: QueryFilter + 'static> SystemParam for Query<'a, Q, F> {
    type Fetch = FetchQuery<Q, F>;
    type State = QueryState<Q, F>;
}

impl<Q: WorldQuery + 'static, F: QueryFilter + 'static> SystemParamState for QueryState<Q, F> {
    fn init(_world: &mut World) -> Self {
        QueryState::default()
    }

    fn update(&mut self, world: &World, _system_state: &mut SystemState) {
        self.update(world);
        // TODO: check for collision with system state archetype component access
    }
}

impl<'a, Q: WorldQuery + 'static, F: QueryFilter + 'static> FetchSystemParam<'a>
    for FetchQuery<Q, F>
{
    type Item = Query<'a, Q, F>;
    type State = QueryState<Q, F>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self::State,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        Some(Query::new(world, state))
    }
}

// pub struct FetchQuerySet<T>(PhantomData<T>);

// impl<T: QueryTuple> SystemParam for QuerySet<T> {
//     type Fetch = FetchQuerySet<T>;
// }

// impl<'a, T: QueryTuple> FetchSystemParam<'a> for FetchQuerySet<T> {
//     type Item = QuerySet<T>;

//     #[inline]
//     unsafe fn get_param(
//         system_state: &'a SystemState,
//         world: &'a World,
//         _resources: &'a Resources,
//     ) -> Option<Self::Item> {
//         let query_index = *system_state.current_query_index.get();
//         *system_state.current_query_index.get() += 1;
//         Some(QuerySet::new(
//             world,
//             &system_state.query_archetype_component_accesses[query_index],
//         ))
//     }

//     fn init(system_state: &mut SystemState, _world: &World) {
//         system_state
//             .query_archetype_component_accesses
//             .push(TypeAccess::default());
//         system_state.query_accesses.push(T::get_accesses());
//         system_state
//             .query_type_names
//             .push(std::any::type_name::<T>());
//     }
// }

pub struct Res<'w, T> {
    value: &'w T,
}

impl<'w, T: Component> Deref for Res<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

pub struct FetchRes<T>(PhantomData<T>);
pub struct FetchResState<T> {
    component_id: Option<ComponentId>,
    marker: PhantomData<T>,
}

impl<'a, T: Component> SystemParam for Res<'a, T> {
    type Fetch = FetchRes<T>;
    type State = FetchResState<T>;
}

impl<T: Component> SystemParamState for FetchResState<T> {
    fn init(world: &mut World) -> Self {
        // TODO: update archetype component access here
        Self {
            component_id: world.components.get_resource_id(TypeId::of::<T>()),
            marker: PhantomData,
        }
    }
}

impl<'a, T: Component> FetchSystemParam<'a> for FetchRes<T> {
    type Item = Res<'a, T>;
    type State = FetchResState<T>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self::State,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        let component_id = state.component_id?;
        let column = world.archetypes.get_resource_column(component_id)?;
        unsafe {
            Some(Res {
                value: &*column.get_ptr().as_ptr().cast::<T>(),
            })
        }
    }
}

pub struct ResMut<'w, T> {
    value: &'w mut T,
    flags: &'w mut ComponentFlags,
}

impl<'w, T: Component> Deref for ResMut<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'w, T: Component> DerefMut for ResMut<'w, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.flags.insert(ComponentFlags::MUTATED);
        self.value
    }
}

pub struct FetchResMut<T>(PhantomData<T>);
pub struct FetchResMutState<T> {
    component_id: Option<ComponentId>,
    marker: PhantomData<T>,
}

impl<'a, T: Component> SystemParam for ResMut<'a, T> {
    type Fetch = FetchResMut<T>;
    type State = FetchResMutState<T>;
}

impl<T: Component> SystemParamState for FetchResMutState<T> {
    fn init(world: &mut World) -> Self {
        // TODO: update archetype component access here
        Self {
            component_id: world.components.get_resource_id(TypeId::of::<T>()),
            marker: PhantomData,
        }
    }
}

impl<'a, T: Component> FetchSystemParam<'a> for FetchResMut<T> {
    type Item = ResMut<'a, T>;
    type State = FetchResMutState<T>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self::State,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        let component_id = state.component_id?;
        let column = world.archetypes.get_resource_column(component_id)?;
        unsafe {
            Some(ResMut {
                value: &mut *column.get_ptr().as_ptr().cast::<T>(),
                flags: &mut *column.get_flags_mut_ptr(),
            })
        }
    }
}

pub struct FetchCommands;

impl<'a> SystemParam for Commands<'a> {
    type Fetch = FetchCommands;
    type State = CommandQueue;
}

impl SystemParamState for CommandQueue {
    fn init(_world: &mut World) -> Self {
        Default::default()
    }

    fn apply(&mut self, world: &mut World) {
        self.apply(world);
    }
}

impl<'a> FetchSystemParam<'a> for FetchCommands {
    type Item = Commands<'a>;
    type State = CommandQueue;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self::State,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        Some(Commands::new(state, world))
    }
}

pub struct FetchParamTuple<T>(PhantomData<T>);
pub struct FetchOr<T>(PhantomData<T>);

macro_rules! impl_system_param_tuple {
    ($($param: ident),*) => {
        impl<$($param: SystemParam),*> SystemParam for ($($param,)*) {
            type Fetch = FetchParamTuple<($($param::Fetch,)*)>;
            type State = ($($param::State,)*);
        }
        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        impl<'a, $($param: FetchSystemParam<'a>),*> FetchSystemParam<'a> for FetchParamTuple<($($param,)*)> {
            type Item = ($($param::Item,)*);
            type State = ($($param::State,)*);

            #[inline]
            unsafe fn get_param(
                state: &'a mut Self::State,
                system_state: &'a SystemState,
                world: &'a World,
            ) -> Option<Self::Item> {

                let ($($param,)*) = state;
                Some(($($param::get_param($param, system_state, world)?,)*))
            }
        }

        #[allow(non_snake_case)]
        impl<$($param: SystemParamState),*> SystemParamState for ($($param,)*) {
            #[inline]
            fn init(_world: &mut World) -> Self {
                (($($param::init(_world),)*))
            }

            #[inline]
            fn apply(&mut self, _world: &mut World) {
                let ($($param,)*) = self;
                $($param.apply(_world);)*
            }
        }

        impl<$($param: SystemParam),*> SystemParam for Or<($(Option<$param>,)*)> {
            type Fetch = FetchOr<($($param::Fetch,)*)>;
            type State = ($($param::State,)*);
        }

        #[allow(unused_variables)]
        #[allow(unused_mut)]
        #[allow(non_snake_case)]
        impl<'a, $($param: FetchSystemParam<'a>),*> FetchSystemParam<'a> for FetchOr<($($param,)*)> {
            type Item = Or<($(Option<$param::Item>,)*)>;
            type State = ($($param::State,)*);

            #[inline]
            unsafe fn get_param(
                state: &'a mut Self::State,
                system_state: &'a SystemState,
                world: &'a World,
            ) -> Option<Self::Item> {
                let mut has_some = false;
                let ($($param,)*) = state;
                $(
                    let $param = $param::get_param($param, system_state, world);
                    if $param.is_some() {
                        has_some = true;
                    }
                )*

                if has_some {
                    Some(Or(($($param,)*)))
                } else {
                    None
                }
            }
        }
    };
}

impl_system_param_tuple!();
impl_system_param_tuple!(A);
impl_system_param_tuple!(A, B);
impl_system_param_tuple!(A, B, C);
impl_system_param_tuple!(A, B, C, D);
impl_system_param_tuple!(A, B, C, D, E);
impl_system_param_tuple!(A, B, C, D, E, F);
impl_system_param_tuple!(A, B, C, D, E, F, G);
impl_system_param_tuple!(A, B, C, D, E, F, G, H);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
impl_system_param_tuple!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);
