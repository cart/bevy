use crate::{
    render_resource::DynamicUniformVec,
    renderer::{RenderDevice, RenderQueue},
    RenderStage,
};
use bevy_app::{App, Plugin};
use bevy_asset::{Asset, Handle};
use bevy_ecs::{component::Component, prelude::*, query::{Fetch, FilterFetch, WorldQuery}};
use crevice::std140::AsStd140;
use std::{marker::PhantomData, ops::Deref};

pub struct DynamicUniformIndex<C: RenderComponent> {
    index: u32,
    marker: PhantomData<C>,
}

impl<C: RenderComponent> DynamicUniformIndex<C> {
    #[inline]
    pub fn index(&self) -> u32 {
        self.index
    }
}

pub trait RenderComponent: Component {
    type SourceComponent: Component;
    fn extract_component(source: &Self::SourceComponent) -> Self;
}

/// Extracts assets into gpu-usable data
pub struct UniformComponentPlugin<C>(PhantomData<fn() -> C>);

impl<C> Default for UniformComponentPlugin<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: RenderComponent + AsStd140 + Clone> Plugin for UniformComponentPlugin<C> {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(0);
        render_app
            .insert_resource(ComponentUniforms::<C>::default())
            .add_system_to_stage(
                RenderStage::Prepare,
                prepare_uniform_components::<C>.system(),
            );
    }
}

pub struct ComponentUniforms<C: RenderComponent + AsStd140>
{
    uniforms: DynamicUniformVec<C>,
}

impl<C: RenderComponent + AsStd140> Deref for ComponentUniforms<C>
{
    type Target = DynamicUniformVec<C>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.uniforms
    }
}

impl<C: RenderComponent + AsStd140> ComponentUniforms<C>
{
    #[inline]
    pub fn uniforms(&self) -> &DynamicUniformVec<C> {
        &self.uniforms
    }
}

impl<C: RenderComponent + AsStd140> Default for ComponentUniforms<C>
{
    fn default() -> Self {
        Self {
            uniforms: Default::default(),
        }
    }
}

fn prepare_uniform_components<C: RenderComponent>(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut component_uniforms: ResMut<ComponentUniforms<C>>,
    components: Query<(Entity, &C)>,
) where
    C: AsStd140 + Clone,
{
    let len = components.iter().len();
    component_uniforms
        .uniforms
        .reserve_and_clear(len, &render_device);
    for (entity, component) in components.iter() {
        commands
            .get_or_spawn(entity)
            .insert(DynamicUniformIndex::<C> {
                index: component_uniforms
                    .uniforms
                    .push(component.clone()),
                marker: PhantomData,
            });
    }

    component_uniforms.uniforms.write_buffer(&render_queue);
}

pub struct RenderComponentPlugin<C, F = ()>(PhantomData<fn() -> (C, F)>);

impl<C, F> Default for RenderComponentPlugin<C, F> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: RenderComponent, F: WorldQuery + 'static> Plugin for RenderComponentPlugin<C, F> where F::Fetch: FilterFetch  {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(0);
        render_app.add_system_to_stage(
            RenderStage::Extract,
            extract_render_components::<C, F>.system(),
        );
    }
}

fn extract_render_components<C: RenderComponent, F: WorldQuery>(
    mut commands: Commands,
    mut previous_len: Local<usize>,
    components: Query<(Entity, &C::SourceComponent), F>,
) where F::Fetch: FilterFetch {
    let mut values = Vec::with_capacity(*previous_len);
    for (entity, component) in components.iter() {
        values.push((entity, (C::extract_component(component),)));
    }
    *previous_len = values.len();
    commands.insert_or_spawn_batch(values);
}

impl<T: Asset> RenderComponent for Handle<T> {
    type SourceComponent = Handle<T>;

    #[inline]
    fn extract_component(source: &Self::SourceComponent) -> Self {
        source.clone_weak()
    }
}
