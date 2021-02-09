use crate::{
    core::{Access, ArchetypeComponentId, ComponentId, World},
    system::{FetchSystemParam, System, SystemId, SystemParam, SystemParamState},
};
use std::borrow::Cow;

pub struct SystemState {
    pub(crate) id: SystemId,
    pub(crate) name: Cow<'static, str>,
    pub(crate) component_access: Access<ComponentId>,
    pub(crate) archetype_component_access: Access<ArchetypeComponentId>,
    pub(crate) is_non_send: bool,
}

pub struct FuncSystem<Out, ParamState> {
    func: Box<
        dyn FnMut(&mut ParamState, &mut SystemState, &World) -> Option<Out> + Send + Sync + 'static,
    >,
    apply_buffers:
        Box<dyn FnMut(&mut ParamState, &mut SystemState, &mut World) + Send + Sync + 'static>,
    init_func: Box<dyn FnMut(&mut SystemState, &mut World) -> ParamState + Send + Sync + 'static>,
    state: SystemState,
    param_state: Option<ParamState>,
}

impl<ParamState: SystemParamState + 'static, Out: 'static> System for FuncSystem<Out, ParamState> {
    type In = ();
    type Out = Out;

    fn name(&self) -> std::borrow::Cow<'static, str> {
        self.state.name.clone()
    }

    fn id(&self) -> SystemId {
        self.state.id
    }

    fn update(&mut self, world: &World) {
        self.state.archetype_component_access.clear();
        let param_state = self.param_state.as_mut().unwrap();
        param_state.update(world, &mut self.state);
    }

    fn component_access(&self) -> &Access<ComponentId> {
        &self.state.component_access
    }

    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.state.archetype_component_access
    }

    fn is_non_send(&self) -> bool {
        self.state.is_non_send
    }

    unsafe fn run_unsafe(&mut self, _input: Self::In, world: &World) -> Option<Out> {
        (self.func)(self.param_state.as_mut().unwrap(), &mut self.state, world)
    }

    fn apply_buffers(&mut self, world: &mut World) {
        (self.apply_buffers)(self.param_state.as_mut().unwrap(), &mut self.state, world)
    }

    fn initialize(&mut self, world: &mut World) {
        self.param_state = Some((self.init_func)(&mut self.state, world));
    }
}

pub struct InputFuncSystem<In, Out, ParamState> {
    func: Box<
        dyn FnMut(In, &mut ParamState, &mut SystemState, &World) -> Option<Out>
            + Send
            + Sync
            + 'static,
    >,
    apply_buffers:
        Box<dyn FnMut(&mut ParamState, &mut SystemState, &mut World) + Send + Sync + 'static>,
    init_func: Box<dyn FnMut(&mut SystemState, &mut World) -> ParamState + Send + Sync + 'static>,
    state: SystemState,
    param_state: Option<ParamState>,
}

impl<In: 'static, Out: 'static, ParamState: SystemParamState + Send + Sync + 'static> System
    for InputFuncSystem<In, Out, ParamState>
{
    type In = In;
    type Out = Out;

    fn name(&self) -> std::borrow::Cow<'static, str> {
        self.state.name.clone()
    }

    fn id(&self) -> SystemId {
        self.state.id
    }

    fn update(&mut self, world: &World) {
        self.state.archetype_component_access.clear();
        let param_state = self.param_state.as_mut().unwrap();
        param_state.update(world, &mut self.state);
    }

    fn component_access(&self) -> &Access<ComponentId> {
        &self.state.component_access
    }

    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.state.archetype_component_access
    }

    fn is_non_send(&self) -> bool {
        self.state.is_non_send
    }

    unsafe fn run_unsafe(&mut self, input: In, world: &World) -> Option<Out> {
        (self.func)(
            input,
            &mut self.param_state.as_mut().unwrap(),
            &mut self.state,
            world,
        )
    }

    fn apply_buffers(&mut self, world: &mut World) {
        (self.apply_buffers)(
            &mut self.param_state.as_mut().unwrap(),
            &mut self.state,
            world,
        )
    }

    fn initialize(&mut self, world: &mut World) {
        self.param_state = Some((self.init_func)(&mut self.state, world));
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

macro_rules! impl_into_system {
    ($($param: ident),*) => {
        impl<Func, Out, $($param: SystemParam),*> IntoSystem<($($param,)*), FuncSystem<Out, ($($param::State,)*)>> for Func
        where
            Func:
                FnMut($($param),*) -> Out +
                FnMut($(<<$param as SystemParam>::Fetch as FetchSystemParam>::Item),*) -> Out +
                Send + Sync + 'static, Out: 'static
        {
            #[allow(unused_variables)]
            #[allow(unused_unsafe)]
            #[allow(non_snake_case)]
            fn system(mut self) -> FuncSystem<Out, ($($param::State,)*)> {
                FuncSystem {
                    state: SystemState {
                        name: std::any::type_name::<Self>().into(),
                        archetype_component_access: Access::default(),
                        component_access: Access::default(),
                        is_non_send: false,
                        id: SystemId::new(),
                    },
                    func: Box::new(move |param_state, state, world| {
                        unsafe {
                            if let Some(($($param,)*)) = <<($($param,)*) as SystemParam>::Fetch as FetchSystemParam>::get_param(param_state, state, world) {
                                Some(self($($param),*))
                            } else {
                                None
                            }
                        }
                    }),
                    apply_buffers: Box::new(|param_state, state, world| {
                        param_state.apply(world);
                    }),
                    param_state: None,
                    init_func: Box::new(|state, world| {
                        ($(<$param::State as SystemParamState>::init(world),)*)
                    }),
                }
            }
        }

        impl<Func, Input, Out, $($param: SystemParam),*> IntoSystem<(Input, $($param,)*), InputFuncSystem<Input, Out, ($($param::State,)*)>> for Func
        where
            Func:
                FnMut(In<Input>, $($param),*) -> Out +
                FnMut(In<Input>, $(<<$param as SystemParam>::Fetch as FetchSystemParam>::Item),*) -> Out +
                Send + Sync + 'static, Input: 'static, Out: 'static
        {
            #[allow(unused_variables)]
            #[allow(unused_unsafe)]
            #[allow(non_snake_case)]
            fn system(mut self) -> InputFuncSystem<Input, Out, ($($param::State,)*)> {
                InputFuncSystem {
                    state: SystemState {
                        name: std::any::type_name::<Self>().into(),
                        archetype_component_access: Access::default(),
                        component_access: Access::default(),
                        is_non_send: false,
                        id: SystemId::new(),
                    },
                    func: Box::new(move |input, param_state, state, world| {
                        unsafe {
                            if let Some(($($param,)*)) = <<($($param,)*) as SystemParam>::Fetch as FetchSystemParam>::get_param(param_state, state, world) {
                                Some(self(In(input), $($param),*))
                            } else {
                                None
                            }
                        }
                    }),
                    param_state: None,
                    apply_buffers: Box::new(|param_state, state, world| {
                        param_state.apply(world);
                    }),
                    init_func: Box::new(|state, world| {
                        ($(<$param::State as SystemParamState>::init(world),)*)
                    }),
                }
            }
        }
    };
}

impl_into_system!();
impl_into_system!(A);
impl_into_system!(A, B);
impl_into_system!(A, B, C);
impl_into_system!(A, B, C, D);
impl_into_system!(A, B, C, D, E);
impl_into_system!(A, B, C, D, E, F);
impl_into_system!(A, B, C, D, E, F, G);
impl_into_system!(A, B, C, D, E, F, G, H);
impl_into_system!(A, B, C, D, E, F, G, H, I);
impl_into_system!(A, B, C, D, E, F, G, H, I, J);
impl_into_system!(A, B, C, D, E, F, G, H, I, J, K);
impl_into_system!(A, B, C, D, E, F, G, H, I, J, K, L);
impl_into_system!(A, B, C, D, E, F, G, H, I, J, K, L, M);
impl_into_system!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
impl_into_system!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
impl_into_system!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);

#[cfg(test)]
mod tests {
    use crate::{
        core::World,
        system::{Query, System},
    };

    use super::IntoSystem;
    #[derive(Debug, Eq, PartialEq, Default)]
    struct A;
    struct B;
    struct C;
    struct D;

    #[test]
    fn simple_system() {
        fn sys(query: Query<&A>) {
            for a in query.iter() {
                println!("{:?}", a);
            }
        }

        let mut system = sys.system();
        let mut world = World::new();
        world.spawn().insert(A);

        system.initialize(&mut world);
        system.update(&mut world);
        system.run((), &mut world);
    }

    // fn run_system<S: System<In = (), Out = ()>>(
    //     world: &mut World,
    //     system: S,
    // ) {
    //     let mut schedule = Schedule::default();
    //     let mut update = SystemStage::parallel();
    //     update.add_system(system);
    //     schedule.add_stage("update", update);
    //     schedule.initialize_and_run(world);
    // }

    // #[test]
    // fn query_system_gets() {
    //     fn query_system(
    //         mut ran: ResMut<bool>,
    //         entity_query: Query<Entity, With<A>>,
    //         b_query: Query<&B>,
    //         a_c_query: Query<(&A, &C)>,
    //         d_query: Query<&D>,
    //     ) {
    //         let entities = entity_query.iter().collect::<Vec<Entity>>();
    //         assert!(
    //             b_query.get_component::<B>(entities[0]).is_err(),
    //             "entity 0 should not have B"
    //         );
    //         assert!(
    //             b_query.get_component::<B>(entities[1]).is_ok(),
    //             "entity 1 should have B"
    //         );
    //         assert!(
    //             b_query.get_component::<A>(entities[1]).is_err(),
    //             "entity 1 should have A, but b_query shouldn't have access to it"
    //         );
    //         assert!(
    //             b_query.get_component::<D>(entities[3]).is_err(),
    //             "entity 3 should have D, but it shouldn't be accessible from b_query"
    //         );
    //         assert!(
    //             b_query.get_component::<C>(entities[2]).is_err(),
    //             "entity 2 has C, but it shouldn't be accessible from b_query"
    //         );
    //         assert!(
    //             a_c_query.get_component::<C>(entities[2]).is_ok(),
    //             "entity 2 has C, and it should be accessible from a_c_query"
    //         );
    //         assert!(
    //             a_c_query.get_component::<D>(entities[3]).is_err(),
    //             "entity 3 should have D, but it shouldn't be accessible from b_query"
    //         );
    //         assert!(
    //             d_query.get_component::<D>(entities[3]).is_ok(),
    //             "entity 3 should have D"
    //         );

    //         *ran = true;
    //     }

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(false);
    //     world.spawn((A,));
    //     world.spawn((A, B));
    //     world.spawn((A, C));
    //     world.spawn((A, D));

    //     run_system(&mut world, &mut resources, query_system.system());

    //     assert!(*resources.get::<bool>().unwrap(), "system ran");
    // }

    // #[test]
    // fn or_query_set_system() {
    //     // Regression test for issue #762
    //     use crate::{Added, Changed, Mutated, Or};
    //     fn query_system(
    //         mut ran: ResMut<bool>,
    //         set: QuerySet<(
    //             Query<(), Or<(Changed<A>, Changed<B>)>>,
    //             Query<(), Or<(Added<A>, Added<B>)>>,
    //             Query<(), Or<(Mutated<A>, Mutated<B>)>>,
    //         )>,
    //     ) {
    //         let changed = set.q0().iter().count();
    //         let added = set.q1().iter().count();
    //         let mutated = set.q2().iter().count();

    //         assert_eq!(changed, 1);
    //         assert_eq!(added, 1);
    //         assert_eq!(mutated, 0);

    //         *ran = true;
    //     }

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(false);
    //     world.spawn((A, B));

    //     run_system(&mut world, &mut resources, query_system.system());

    //     assert!(*resources.get::<bool>().unwrap(), "system ran");
    // }

    // #[test]
    // fn changed_resource_system() {
    //     fn incr_e_on_flip(_run_on_flip: ChangedRes<bool>, mut query: Query<&mut i32>) {
    //         for mut i in query.iter_mut() {
    //             *i += 1;
    //         }
    //     }

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(false);
    //     let ent = world.spawn((0,));

    //     let mut schedule = Schedule::default();
    //     let mut update = SystemStage::parallel();
    //     update.add_system(incr_e_on_flip.system());
    //     schedule.add_stage("update", update);
    //     schedule.add_stage(
    //         "clear_trackers",
    //         SystemStage::single(clear_trackers_system.system()),
    //     );

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     *resources.get_mut::<bool>().unwrap() = true;
    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 2);
    // }

    // #[test]
    // fn changed_resource_or_system() {
    //     fn incr_e_on_flip(
    //         _or: Or<(Option<ChangedRes<bool>>, Option<ChangedRes<i32>>)>,
    //         mut query: Query<&mut i32>,
    //     ) {
    //         for mut i in query.iter_mut() {
    //             *i += 1;
    //         }
    //     }

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(false);
    //     resources.insert::<i32>(10);
    //     let ent = world.spawn((0,));

    //     let mut schedule = Schedule::default();
    //     let mut update = SystemStage::parallel();
    //     update.add_system(incr_e_on_flip.system());
    //     schedule.add_stage("update", update);
    //     schedule.add_stage(
    //         "clear_trackers",
    //         SystemStage::single(clear_trackers_system.system()),
    //     );

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 1);

    //     *resources.get_mut::<bool>().unwrap() = true;
    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 2);

    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 2);

    //     *resources.get_mut::<i32>().unwrap() = 20;
    //     schedule.initialize_and_run(&mut world, &mut resources);
    //     assert_eq!(*(world.get::<i32>(ent).unwrap()), 3);
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_query_mut_system() {
    //     fn sys(_q1: Query<&mut A>, _q2: Query<&mut A>) {}

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     world.spawn((A,));

    //     run_system(&mut world, &mut resources, sys.system());
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_query_immut_system() {
    //     fn sys(_q1: Query<&A>, _q2: Query<&mut A>) {}

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     world.spawn((A,));

    //     run_system(&mut world, &mut resources, sys.system());
    // }

    // #[test]
    // fn query_set_system() {
    //     fn sys(_set: QuerySet<(Query<&mut A>, Query<&B>)>) {}

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     world.spawn((A,));

    //     run_system(&mut world, &mut resources, sys.system());
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_query_with_query_set_system() {
    //     fn sys(_query: Query<&mut A>, _set: QuerySet<(Query<&mut A>, Query<&B>)>) {}

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     world.spawn((A,));

    //     run_system(&mut world, &mut resources, sys.system());
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_query_sets_system() {
    //     fn sys(_set_1: QuerySet<(Query<&mut A>,)>, _set_2: QuerySet<(Query<&mut A>, Query<&B>)>) {}

    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     world.spawn((A,));
    //     run_system(&mut world, &mut resources, sys.system());
    // }

    // #[derive(Default)]
    // struct BufferRes {
    //     _buffer: Vec<u8>,
    // }

    // fn test_for_conflicting_resources<S: System<In = (), Out = ()>>(sys: S) {
    //     let mut world = World::default();
    //     let mut resources = Resources::default();
    //     resources.insert(BufferRes::default());
    //     resources.insert(A);
    //     resources.insert(B);
    //     run_system(&mut world, &mut resources, sys.system());
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_system_resources() {
    //     fn sys(_: ResMut<BufferRes>, _: Res<BufferRes>) {}
    //     test_for_conflicting_resources(sys.system())
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_system_resources_reverse_order() {
    //     fn sys(_: Res<BufferRes>, _: ResMut<BufferRes>) {}
    //     test_for_conflicting_resources(sys.system())
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_system_resources_multiple_mutable() {
    //     fn sys(_: ResMut<BufferRes>, _: ResMut<BufferRes>) {}
    //     test_for_conflicting_resources(sys.system())
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_changed_and_mutable_resource() {
    //     // A tempting pattern, but unsound if allowed.
    //     fn sys(_: ResMut<BufferRes>, _: ChangedRes<BufferRes>) {}
    //     test_for_conflicting_resources(sys.system())
    // }

    // #[test]
    // #[should_panic]
    // fn conflicting_system_local_resources() {
    //     fn sys(_: Local<BufferRes>, _: Local<BufferRes>) {}
    //     test_for_conflicting_resources(sys.system())
    // }

    // #[test]
    // fn nonconflicting_system_resources() {
    //     fn sys(_: Local<BufferRes>, _: ResMut<BufferRes>, _: Local<A>, _: ResMut<A>) {}
    //     test_for_conflicting_resources(sys.system())
    // }
}
