use crate::{
    core::{
        Access, ArchetypeId, Component, ComponentFlags, ComponentId, Entity, FromWorld, Or,
        QueryFilter, QueryState, World, WorldQuery,
    },
    system::{CommandQueue, Commands, Query, SystemState},
};
use bevy_ecs3_macros::{all_tuples, impl_query_set};
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

pub trait SystemParam: Sized {
    type State: SystemParamState + for<'a> SystemParamFetch<'a>;
}

/// # Safety
/// it is the implementors responsibility to ensure `system_state` is populated with the _exact_
/// [World] access used by the SystemParamState (and associated FetchSystemParam). Additionally, it is the
/// implementor's responsibility to ensure there is no conflicting access across all SystemParams.
pub unsafe trait SystemParamState: Send + Sync + 'static {
    fn init(world: &mut World, system_state: &mut SystemState) -> Self;
    #[inline]
    fn update(&mut self, _world: &World, _system_state: &mut SystemState) {}
    #[inline]
    fn apply(&mut self, _world: &mut World) {}
}

pub trait SystemParamFetch<'a>: SystemParamState {
    type Item;
    /// # Safety
    /// This call might access any of the input parameters in an unsafe way. Make sure the data access is safe in
    /// the context of the system scheduler
    unsafe fn get_param(
        state: &'a mut Self,
        system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item>;
}

pub struct QueryFetch<Q, F>(PhantomData<(Q, F)>);

impl<'a, Q: WorldQuery + 'static, F: QueryFilter + 'static> SystemParam for Query<'a, Q, F> {
    type State = QueryState<Q, F>;
}

// SAFE: Relevant query ComponentId and ArchetypeComponentId access is applied to SystemState. If this QueryState conflicts
// with any prior access, a panic will occur.
unsafe impl<Q: WorldQuery + 'static, F: QueryFilter + 'static> SystemParamState
    for QueryState<Q, F>
{
    fn init(world: &mut World, system_state: &mut SystemState) -> Self {
        let state = QueryState::new(world);
        assert_component_access_compatibility(
            &system_state.name,
            std::any::type_name::<Q>(),
            std::any::type_name::<F>(),
            &state.component_access,
            &system_state.component_access,
            world,
        );
        system_state
            .component_access
            .extend(&state.component_access);
        system_state
            .archetype_component_access
            .extend(&state.archetype_component_access);
        state
    }

    fn update(&mut self, world: &World, system_state: &mut SystemState) {
        self.update_archetypes(world);
        system_state
            .archetype_component_access
            .extend(&self.archetype_component_access);
    }
}

impl<'a, Q: WorldQuery + 'static, F: QueryFilter + 'static> SystemParamFetch<'a>
    for QueryState<Q, F>
{
    type Item = Query<'a, Q, F>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        Some(Query::new(world, state))
    }
}

fn assert_component_access_compatibility(
    system_name: &str,
    query_type: &'static str,
    filter_type: &'static str,
    current: &Access<ComponentId>,
    previous: &Access<ComponentId>,
    world: &World,
) {
    if !current.is_compatible(&previous) {
        let conflicting_components = current
            .get_conflicts(previous)
            .drain(..)
            .map(|component_id| world.components.get_info(component_id).unwrap().name())
            .collect::<Vec<&str>>();
        let accesses = conflicting_components.join(", ");
        panic!("Query<{}, {}> in system {} accesses component(s) {} in a way that conflicts with a previous system parameter. Allowing this would break Rust's mutability rules. Consider merging conflicting Queries into a QuerySet.",
            query_type, filter_type, system_name, accesses);
    }
}

pub struct QuerySet<T>(T);
pub struct QuerySetState<T>(T);

impl_query_set!();

pub struct Res<'w, T> {
    value: &'w T,
}

impl<'w, T: Component> Deref for Res<'w, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

pub struct ResState<T> {
    component_id: ComponentId,
    // NOTE: PhantomData<fn()-> X> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> T>,
}

impl<'a, T: Component> SystemParam for Res<'a, T> {
    type State = ResState<T>;
}

// SAFE: Res ComponentId and ArchetypeComponentId access is applied to SystemState. If this Res conflicts
// with any prior access, a panic will occur.
unsafe impl<T: Component> SystemParamState for ResState<T> {
    fn init(world: &mut World, system_state: &mut SystemState) -> Self {
        let component_id = world.components.get_or_insert_resource_id::<T>();
        if system_state.component_access.has_write(component_id) {
            panic!(
                "Res<{}> in system {} conflicts with a previous ResMut<{0}> access. Allowing this would break Rust's mutability rules. Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_state.name);
        }
        system_state.component_access.add_read(component_id);
        Self {
            component_id,
            marker: PhantomData,
        }
    }

    // PERF: move this into init by somehow creating the archetype component id, even if there is no resource yet
    fn update(&mut self, world: &World, system_state: &mut SystemState) {
        // SAFE: resource archetype always exists
        let archetype = unsafe {
            world
                .archetypes()
                .get_unchecked(ArchetypeId::resource_archetype())
        };

        if let Some(archetype_component) = archetype.get_archetype_component_id(self.component_id) {
            system_state
                .archetype_component_access
                .add_read(archetype_component);
        }
    }
}

impl<'a, T: Component> SystemParamFetch<'a> for ResState<T> {
    type Item = Res<'a, T>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        let column = world.get_resource_column(state.component_id)?;
        Some(Res {
            value: &*column.get_ptr().as_ptr().cast::<T>(),
        })
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

pub struct ResMutFetch<T>(PhantomData<T>);
pub struct ResMutState<T> {
    component_id: ComponentId,
    // NOTE: PhantomData<fn()-> X> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> T>,
}

impl<'a, T: Component> SystemParam for ResMut<'a, T> {
    type State = ResMutState<T>;
}

// SAFE: Res ComponentId and ArchetypeComponentId access is applied to SystemState. If this Res conflicts
// with any prior access, a panic will occur.
unsafe impl<T: Component> SystemParamState for ResMutState<T> {
    fn init(world: &mut World, system_state: &mut SystemState) -> Self {
        let component_id = world.components.get_or_insert_resource_id::<T>();
        if system_state.component_access.has_write(component_id) {
            panic!(
                "ResMut<{}> in system {} conflicts with a previous ResMut<{0}> access. Allowing this would break Rust's mutability rules. Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_state.name);
        } else if system_state.component_access.has_read(component_id) {
            panic!(
                "ResMut<{}> in system {} conflicts with a previous Res<{0}> access. Allowing this would break Rust's mutability rules. Consider removing the duplicate access.",
                std::any::type_name::<T>(), system_state.name);
        }
        system_state.component_access.add_write(component_id);
        Self {
            component_id,
            marker: PhantomData,
        }
    }

    // PERF: move this into init by somehow creating the archetype component id, even if there is no resource yet
    fn update(&mut self, world: &World, system_state: &mut SystemState) {
        // SAFE: resource archetype always exists
        let archetype = unsafe {
            world
                .archetypes()
                .get_unchecked(ArchetypeId::resource_archetype())
        };

        if let Some(archetype_component) = archetype.get_archetype_component_id(self.component_id) {
            system_state
                .archetype_component_access
                .add_write(archetype_component);
        }
    }
}

impl<'a, T: Component> SystemParamFetch<'a> for ResMutState<T> {
    type Item = ResMut<'a, T>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        let column = world.get_resource_column(state.component_id)?;
        Some(ResMut {
            value: &mut *column.get_ptr().as_ptr().cast::<T>(),
            flags: &mut *column.get_flags_mut_ptr(),
        })
    }
}

pub struct FetchCommands;

impl<'a> SystemParam for Commands<'a> {
    type State = CommandQueue;
}

// SAFE: only local state is accessed
unsafe impl SystemParamState for CommandQueue {
    fn init(_world: &mut World, _system_state: &mut SystemState) -> Self {
        Default::default()
    }

    fn apply(&mut self, world: &mut World) {
        self.apply(world);
    }
}

impl<'a> SystemParamFetch<'a> for CommandQueue {
    type Item = Commands<'a>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        Some(Commands::new(state, world))
    }
}

pub struct Local<'a, T: Component>(&'a mut T);

impl<'a, T: Component> Deref for Local<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a, T: Component> DerefMut for Local<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

pub struct LocalState<T: Component>(T);

impl<'a, T: Component + FromWorld> SystemParam for Local<'a, T> {
    type State = LocalState<T>;
}

// SAFE: only local state is accessed
unsafe impl<T: Component + FromWorld> SystemParamState for LocalState<T> {
    fn init(world: &mut World, _system_state: &mut SystemState) -> Self {
        Self(T::from_world(world))
    }
}

impl<'a, T: Component + FromWorld> SystemParamFetch<'a> for LocalState<T> {
    type Item = Local<'a, T>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self,
        _system_state: &'a SystemState,
        _world: &'a World,
    ) -> Option<Self::Item> {
        Some(Local(&mut state.0))
    }
}

pub struct RemovedComponents<'a, T> {
    world: &'a World,
    component_id: ComponentId,
    marker: PhantomData<T>,
}

impl<'a, T> RemovedComponents<'a, T> {
    pub fn iter(&self) -> std::iter::Cloned<std::slice::Iter<'_, Entity>> {
        self.world.removed_with_id(self.component_id)
    }
}

pub struct RemovedComponentsState<T> {
    component_id: ComponentId,
    // NOTE: PhantomData<fn()-> T> gives this safe Send/Sync impls
    marker: PhantomData<fn() -> T>,
}

impl<'a, T: Component> SystemParam for RemovedComponents<'a, T> {
    type State = RemovedComponentsState<T>;
}

// SAFE: no component access. removed component entity collections can be read in parallel and are never mutably borrowed
// during system execution
unsafe impl<T: Component> SystemParamState for RemovedComponentsState<T> {
    fn init(world: &mut World, _system_state: &mut SystemState) -> Self {
        Self {
            component_id: world.components.get_or_insert_id::<T>(),
            marker: PhantomData,
        }
    }
}

impl<'a, T: Component> SystemParamFetch<'a> for RemovedComponentsState<T> {
    type Item = RemovedComponents<'a, T>;

    #[inline]
    unsafe fn get_param(
        state: &'a mut Self,
        _system_state: &'a SystemState,
        world: &'a World,
    ) -> Option<Self::Item> {
        Some(RemovedComponents {
            world,
            component_id: state.component_id,
            marker: PhantomData,
        })
    }
}

pub struct OrState<T>(T);

macro_rules! impl_system_param_tuple {
    ($($param: ident),*) => {
        impl<$($param: SystemParam),*> SystemParam for ($($param,)*) {
            type State = ($($param::State,)*);
        }
        #[allow(unused_variables)]
        #[allow(non_snake_case)]
        impl<'a, $($param: SystemParamFetch<'a>),*> SystemParamFetch<'a> for ($($param,)*) {
            type Item = ($($param::Item,)*);

            #[inline]
            unsafe fn get_param(
                state: &'a mut Self,
                system_state: &'a SystemState,
                world: &'a World,
            ) -> Option<Self::Item> {

                let ($($param,)*) = state;
                Some(($($param::get_param($param, system_state, world)?,)*))
            }
        }

        /// SAFE: implementors of each SystemParamState in the tuple have validated their impls
        #[allow(non_snake_case)]
        unsafe impl<$($param: SystemParamState),*> SystemParamState for ($($param,)*) {
            #[inline]
            fn init(_world: &mut World, _system_state: &mut SystemState) -> Self {
                (($($param::init(_world, _system_state),)*))
            }

            #[inline]
            fn update(&mut self, _world: &World, _system_state: &mut SystemState) {
                let ($($param,)*) = self;
                $($param.update(_world, _system_state);)*
            }

            #[inline]
            fn apply(&mut self, _world: &mut World) {
                let ($($param,)*) = self;
                $($param.apply(_world);)*
            }
        }

        impl<$($param: SystemParam),*> SystemParam for Or<($(Option<$param>,)*)> {
            type State = OrState<($($param::State,)*)>;
        }

        #[allow(non_snake_case)]
        unsafe impl<$($param: SystemParamState),*> SystemParamState for OrState<($($param,)*)> {
            #[inline]
            fn init(_world: &mut World, _system_state: &mut SystemState) -> Self {
                OrState((($($param::init(_world, _system_state),)*)))
            }

            #[inline]
            fn update(&mut self, _world: &World, _system_state: &mut SystemState) {
                let ($($param,)*) = &mut self.0;
                $($param.update(_world, _system_state);)*
            }

            #[inline]
            fn apply(&mut self, _world: &mut World) {
                let ($($param,)*) = &mut self.0;
                $($param.apply(_world);)*
            }
        }

        #[allow(unused_variables)]
        #[allow(unused_mut)]
        #[allow(non_snake_case)]
        impl<'a, $($param: SystemParamFetch<'a>),*> SystemParamFetch<'a> for OrState<($($param,)*)> {
            type Item = Or<($(Option<$param::Item>,)*)>;

            #[inline]
            unsafe fn get_param(
                state: &'a mut Self,
                system_state: &'a SystemState,
                world: &'a World,
            ) -> Option<Self::Item> {
                let mut has_some = false;
                let ($($param,)*) = &mut state.0;
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

all_tuples!(impl_system_param_tuple, 0, 16, P);
