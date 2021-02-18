use bevy_ecs::core::World;
use bevy_utils::tracing::error;

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
        error!("this is unsafe and should be fixed");
        let mut shared_buffers =
            unsafe { world.get_resource_mut_unchecked::<SharedBuffers>().unwrap() };
        shared_buffers.apply(render_context);
    }
}
