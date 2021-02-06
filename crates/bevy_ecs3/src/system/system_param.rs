use crate::{core::{Fetch, Or, QueryFilter, QueryState, World, WorldQuery}, system::{Commands, Query, SystemQueryState, SystemState}};
use parking_lot::Mutex;
use std::{any::TypeId, marker::PhantomData, sync::Arc};
pub trait SystemParam: Sized {
    type Fetch: for<'a> FetchSystemParam<'a>;
}

pub trait FetchSystemParam<'a> {
    type Item;
    fn init(system_state: &mut SystemState, world: &World);
    /// # Safety
    /// This call might access any of the input parameters in an unsafe way. Make sure the data access is safe in
    /// the context of the system scheduler
    unsafe fn get_param(system_state: &'a SystemState, world: &'a World) -> Option<Self::Item>;
}

pub struct FetchQuery<Q, F>(PhantomData<(Q, F)>);

impl<'a, Q: WorldQuery, F: QueryFilter> SystemParam for Query<'a, Q, F> {
    type Fetch = FetchQuery<Q, F>;
}

impl<'a, Q: WorldQuery, F: QueryFilter> FetchSystemParam<'a> for FetchQuery<Q, F> {
    type Item = Query<'a, Q, F>;

    #[inline]
    unsafe fn get_param(system_state: &'a SystemState, world: &'a World) -> Option<Self::Item> {
        let query_index = *system_state.current_query_index.get();
        let system_query_state: &'a SystemQueryState = &system_state.param_query_states[query_index][0];
        *system_state.current_query_index.get() += 1;
        // TODO: try caching fetch state in SystemParam
        // TODO: remove these "expects"
        Some(Query::new(world, system_query_state))
    }

    fn init(system_state: &mut SystemState, _world: &World) {
        system_state
            .param_query_states
            .push(vec![SystemQueryState::default()]);
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
}
impl<'a> FetchSystemParam<'a> for FetchCommands {
    type Item = &'a mut Commands;

    fn init(system_state: &mut SystemState, world: &World) {}

    #[inline]
    unsafe fn get_param(system_state: &'a SystemState, _world: &'a World) -> Option<Self::Item> {
        Some(&mut *system_state.commands.get())
    }
}

pub struct FetchArcCommands;
impl SystemParam for Arc<Mutex<Commands>> {
    type Fetch = FetchArcCommands;
}

impl<'a> FetchSystemParam<'a> for FetchArcCommands {
    type Item = Arc<Mutex<Commands>>;

    fn init(system_state: &mut SystemState, world: &World) {
        system_state.arc_commands.get_or_insert_with(|| {
            let mut commands = Commands::default();
            Arc::new(Mutex::new(commands))
        });
    }

    #[inline]
    unsafe fn get_param(system_state: &SystemState, _world: &World) -> Option<Self::Item> {
        Some(system_state.arc_commands.as_ref().unwrap().clone())
    }
}

pub struct FetchParamTuple<T>(PhantomData<T>);
pub struct FetchOr<T>(PhantomData<T>);

macro_rules! impl_system_param_tuple {
    ($($param: ident),*) => {
        impl<$($param: SystemParam),*> SystemParam for ($($param,)*) {
            type Fetch = FetchParamTuple<($($param::Fetch,)*)>;
        }
        #[allow(unused_variables)]
        impl<'a, $($param: FetchSystemParam<'a>),*> FetchSystemParam<'a> for FetchParamTuple<($($param,)*)> {
            type Item = ($($param::Item,)*);
            fn init(system_state: &mut SystemState, world: &World) {
                $($param::init(system_state, world);)*
            }

            #[inline]
            unsafe fn get_param(
                system_state: &'a SystemState,
                world: &'a World,
            ) -> Option<Self::Item> {
                Some(($($param::get_param(system_state, world)?,)*))
            }
        }

        impl<$($param: SystemParam),*> SystemParam for Or<($(Option<$param>,)*)> {
            type Fetch = FetchOr<($($param::Fetch,)*)>;
        }

        #[allow(unused_variables)]
        #[allow(unused_mut)]
        #[allow(non_snake_case)]
        impl<'a, $($param: FetchSystemParam<'a>),*> FetchSystemParam<'a> for FetchOr<($($param,)*)> {
            type Item = Or<($(Option<$param::Item>,)*)>;
            fn init(system_state: &mut SystemState, world: &World) {
                $($param::init(system_state, world);)*
            }

            #[inline]
            unsafe fn get_param(
                system_state: &'a SystemState,
                world: &'a World,
            ) -> Option<Self::Item> {
                let mut has_some = false;
                $(
                    let $param = $param::get_param(system_state, world);
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
