use crate::{
    render_resource::DynamicUniformVec,
    renderer::{RenderDevice, RenderQueue},
    RenderStage,
};
use bevy_app::{App, Plugin};
use bevy_asset::{Asset, Handle};
use bevy_ecs::{component::Component, prelude::*};
use bevy_math::Mat4;
use bevy_transform::components::GlobalTransform;
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
    type ExtractedComponent: Component;
    fn extract_component(&self) -> Self::ExtractedComponent;
}

/// Extracts assets into gpu-usable data
pub struct UniformComponentPlugin<C>(PhantomData<fn() -> C>);

impl<C> Default for UniformComponentPlugin<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: RenderComponent> Plugin for UniformComponentPlugin<C>
where
    C::ExtractedComponent: AsStd140 + Clone,
{
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

pub struct ComponentUniforms<C: RenderComponent>
where
    C::ExtractedComponent: AsStd140,
{
    uniforms: DynamicUniformVec<C::ExtractedComponent>,
}

impl<C: RenderComponent> Deref for ComponentUniforms<C>
where
    C::ExtractedComponent: AsStd140,
{
    type Target = DynamicUniformVec<C::ExtractedComponent>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.uniforms
    }
}

impl<C: RenderComponent> ComponentUniforms<C>
where
    C::ExtractedComponent: AsStd140,
{
    #[inline]
    pub fn uniforms(&self) -> &DynamicUniformVec<C::ExtractedComponent> {
        &self.uniforms
    }
}

impl<C: RenderComponent> Default for ComponentUniforms<C>
where
    C::ExtractedComponent: AsStd140,
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
    extracted_components: Query<(Entity, &C::ExtractedComponent)>,
) where
    C::ExtractedComponent: AsStd140 + Clone,
{
    let len = extracted_components.iter().len();
    component_uniforms
        .uniforms
        .reserve_and_clear(len, &render_device);
    for (entity, extracted_component) in extracted_components.iter() {
        commands
            .get_or_spawn(entity)
            .insert(DynamicUniformIndex::<C> {
                index: component_uniforms
                    .uniforms
                    .push(extracted_component.clone()),
                marker: PhantomData,
            });
    }

    component_uniforms.uniforms.write_buffer(&render_queue);
}

pub struct RenderComponentPlugin<C>(PhantomData<fn() -> C>);

impl<C> Default for RenderComponentPlugin<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: RenderComponent> Plugin for RenderComponentPlugin<C> {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(0);
        render_app.add_system_to_stage(
            RenderStage::Extract,
            extract_render_components::<C>.system(),
        );
    }
}

fn extract_render_components<C: RenderComponent>(
    mut commands: Commands,
    components: Query<(Entity, &C)>,
) {
    for (entity, component) in components.iter() {
        commands
            .get_or_spawn(entity)
            .insert(component.extract_component());
    }
}

impl RenderComponent for GlobalTransform {
    type ExtractedComponent = Mat4;

    #[inline]
    fn extract_component(&self) -> Self::ExtractedComponent {
        self.compute_matrix()
    }
}

impl<T: Asset> RenderComponent for Handle<T> {
    type ExtractedComponent = Handle<T>;

    #[inline]
    fn extract_component(&self) -> Self::ExtractedComponent {
        self.clone_weak()
    }
}
