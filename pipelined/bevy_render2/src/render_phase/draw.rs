use crate::render_phase::TrackedRenderPass;
use bevy_app::App;
use bevy_ecs::{
    all_tuples,
    entity::Entity,
    system::{ReadOnlySystemParamFetch, SystemParam, SystemParamItem, SystemState},
    world::World,
};
use bevy_utils::HashMap;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{any::TypeId, fmt::Debug, hash::Hash};

pub trait Draw<P: PhaseItem>: Send + Sync + 'static {
    fn draw<'w>(
        &mut self,
        world: &'w World,
        pass: &mut TrackedRenderPass<'w>,
        view: Entity,
        item: &P,
    );
}

pub trait PhaseItem: Send + Sync + 'static {
    type Key;
    type SortKey: Ord;
    fn sort_key(&self) -> Self::SortKey;
    fn draw_function(&self) -> DrawFunctionId;
}

// TODO: make this generic?
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct DrawFunctionId(usize);

pub struct DrawFunctionsInternal<D: PhaseItem> {
    pub draw_functions: Vec<Box<dyn Draw<D>>>,
    pub indices: HashMap<TypeId, DrawFunctionId>,
}

impl<I: PhaseItem> DrawFunctionsInternal<I> {
    pub fn add<T: Draw<I>>(&mut self, draw_function: T) -> DrawFunctionId {
        self.add_with::<T, T>(draw_function)
    }

    pub fn add_with<T: 'static, D: Draw<I>>(&mut self, draw_function: D) -> DrawFunctionId {
        self.draw_functions.push(Box::new(draw_function));
        let id = DrawFunctionId(self.draw_functions.len() - 1);
        self.indices.insert(TypeId::of::<T>(), id);
        id
    }

    pub fn get_mut(&mut self, id: DrawFunctionId) -> Option<&mut dyn Draw<I>> {
        self.draw_functions.get_mut(id.0).map(|f| &mut **f)
    }

    pub fn get_id<T: 'static>(&self) -> Option<DrawFunctionId> {
        self.indices.get(&TypeId::of::<T>()).copied()
    }
}

pub struct DrawFunctions<I: PhaseItem> {
    internal: RwLock<DrawFunctionsInternal<I>>,
}

impl<I: PhaseItem> Default for DrawFunctions<I> {
    fn default() -> Self {
        Self {
            internal: RwLock::new(DrawFunctionsInternal {
                draw_functions: Vec::new(),
                indices: HashMap::default(),
            }),
        }
    }
}

impl<I: PhaseItem> DrawFunctions<I> {
    pub fn read(&self) -> RwLockReadGuard<'_, DrawFunctionsInternal<I>> {
        self.internal.read()
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, DrawFunctionsInternal<I>> {
        self.internal.write()
    }
}
pub trait DrawCommand<P: PhaseItem> {
    type Param: SystemParam;
    fn draw<'w>(
        view: Entity,
        item: &P,
        param: SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    );
}

macro_rules! draw_command_tuple_impl {
    ($($name: ident),*) => {
        impl<P: PhaseItem, $($name: DrawCommand<P>),*> DrawCommand<P> for ($($name,)*) {
            type Param = ($($name::Param,)*);

            #[allow(non_snake_case)]
            fn draw<'w>(
                _view: Entity,
                _item: &P,
                ($($name,)*): SystemParamItem<'_, 'w, Self::Param>,
                _pass: &mut TrackedRenderPass<'w>,
            ) {
                $($name::draw(_view, _item, $name, _pass);)*
            }
        }
    };
}

all_tuples!(draw_command_tuple_impl, 0, 15, C);

pub struct DrawCommandState<P: PhaseItem, D: DrawCommand<P>> {
    state: SystemState<D::Param>,
}

impl<P: PhaseItem, D: DrawCommand<P>> DrawCommandState<P, D> {
    pub fn new(world: &mut World) -> Self {
        Self {
            state: SystemState::new(world),
        }
    }
}

impl<P: PhaseItem, D: DrawCommand<P> + Send + Sync + 'static> Draw<P> for DrawCommandState<P, D>
where
    <D::Param as SystemParam>::Fetch: ReadOnlySystemParamFetch,
{
    fn draw<'w>(
        &mut self,
        world: &'w World,
        pass: &mut TrackedRenderPass<'w>,
        view: Entity,
        item: &P,
    ) {
        let param = self.state.get(world);
        D::draw(view, item, param, pass);
    }
}

pub trait AddDrawCommand {
    fn add_draw_command<I: PhaseItem, T: 'static, D: DrawCommand<I> + Send + Sync + 'static>(
        &mut self,
    ) -> &mut Self
    where
        <D::Param as SystemParam>::Fetch: ReadOnlySystemParamFetch;
}

impl AddDrawCommand for App {
    fn add_draw_command<I: PhaseItem, T: 'static, D: DrawCommand<I> + Send + Sync + 'static>(
        &mut self,
    ) -> &mut Self
    where
        <D::Param as SystemParam>::Fetch: ReadOnlySystemParamFetch,
    {
        let draw_function = DrawCommandState::<I, D>::new(&mut self.world);
        let draw_functions = self.world.get_resource::<DrawFunctions<I>>().unwrap();
        draw_functions.write().add_with::<T, _>(draw_function);
        self
    }
}
