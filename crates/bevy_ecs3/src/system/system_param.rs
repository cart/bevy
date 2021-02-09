use crate::{
    core::{Or, QueryFilter, QueryState, World, WorldQuery},
    system::{Commands, Query, SystemState},
};
use parking_lot::Mutex;
use std::{marker::PhantomData, sync::Arc};

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

pub struct FetchCommands;

impl<'a> SystemParam for &'a mut Commands {
    type Fetch = FetchCommands;
    type State = Commands;
}

impl SystemParamState for Commands {
    fn init(_world: &mut World) -> Self {
        Default::default()
    }

    fn apply(&mut self, world: &mut World) {
        self.apply(world);
    }
}

impl<'a> FetchSystemParam<'a> for FetchCommands {
    type Item = &'a mut Commands;
    type State = Commands;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self::State,
        _system_state: &'a SystemState,
        _world: &'a World,
    ) -> Option<Self::Item> {
        Some(state)
    }
}

pub struct FetchArcCommands;
impl SystemParam for Arc<Mutex<Commands>> {
    type Fetch = FetchArcCommands;
    type State = Arc<Mutex<Commands>>;
}

impl SystemParamState for Arc<Mutex<Commands>> {
    fn init(_world: &mut World) -> Self {
        Default::default()
    }

    fn apply(&mut self, world: &mut World) {
        let mut commands = self.lock();
        commands.apply(world);
    }
}

impl<'a> FetchSystemParam<'a> for FetchArcCommands {
    type Item = Arc<Mutex<Commands>>;
    type State = Arc<Mutex<Commands>>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self::State,
        _system_state: &SystemState,
        _world: &World,
    ) -> Option<Self::Item> {
        Some(state.clone())
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
