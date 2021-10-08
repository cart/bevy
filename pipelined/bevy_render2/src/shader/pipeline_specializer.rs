use std::hash::Hash;

use bevy_ecs::{entity::Entity, prelude::World};
use bevy_utils::HashMap;
use wgpu::{RenderPipeline, RenderPipelineDescriptor};

use crate::{
    render_asset::RenderAssets,
    render_phase::{Draw, PhaseItem, TrackedRenderPass},
    render_resource::RenderPipelineId,
    renderer::RenderDevice,
    shader::{RenderPipelineCache, Shader},
};

pub struct EntityRendererCache<E: EntityRenderer> {
    cache: HashMap<E::Key, RenderPipelineId>,
    pipelines: Vec<RenderPipeline>,
}

impl<E: EntityRenderer> EntityRendererCache<E> {
    pub fn get_pipeline(
        &mut self,
        device: &RenderDevice,
        cache: &mut RenderPipelineCache,
        shaders: &mut RenderAssets<Shader>,
        key: E::Key,
    ) -> RenderPipelineId {
        self.cache.entry(key).or_insert_with(|| {
            let descriptor = E::get_pipeline(cache, shaders, key);
            device.create_render_pipeline(&descriptor)
            
        })
    }
}

trait EntityRenderer {
    type Key: Hash + PartialEq + Eq;
    // type Query: Fetch;
    // get_key(item: QueryItem<Self::Query>) -> Self::Key {}
    fn get_pipeline<'a>(
        cache: &mut RenderPipelineCache,
        shaders: &'a mut RenderAssets<Shader>,
        key: Self::Key,
    ) -> RenderPipelineDescriptor<'a>;
    fn get_draw(key: Self::Key) -> Box<dyn DrawEntity>;
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct PbrPipelineKey: u32 {
        const THING = 1;
    }
}

pub trait DrawEntity {
    fn draw_entity<'w>(
        &mut self,
        world: &'w World,
        pass: &mut TrackedRenderPass<'w>,
        view: Entity,
        entity: Entity,
    );
}

pub trait EntityPhaseItem: PhaseItem {
    fn phase_item_entity(&self) -> Entity;
}

impl<P: EntityPhaseItem, D: DrawEntity + Send + Sync + 'static> Draw<P> for D {
    fn draw<'w>(
        &mut self,
        world: &'w World,
        pass: &mut TrackedRenderPass<'w>,
        view: Entity,
        item: &P,
    ) {
        self.draw_entity(world, pass, view, item.phase_item_entity())
    }
}
