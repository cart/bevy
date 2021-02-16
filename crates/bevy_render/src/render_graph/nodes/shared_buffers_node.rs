use bevy_ecs::core::World;

use crate::{
    render_graph::{Node, ResourceSlots},
    renderer::{RenderContext, SharedBuffers},
};

#[derive(Debug, Default)]
pub struct SharedBuffersNode;

impl Node for SharedBuffersNode {
    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn RenderContext,
        _input: &ResourceSlots,
        _output: &mut ResourceSlots,
    ) {
        let mut shared_buffers = world.get_resource_mut::<SharedBuffers>().unwrap();
        shared_buffers.apply(render_context);
    }
}
