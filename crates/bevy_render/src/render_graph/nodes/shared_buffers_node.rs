use crate::{
    render_graph::{CommandQueue, Node, ResourceSlots},
    renderer::{RenderContext, SharedBuffers},
};
use bevy_ecs::core::World;

#[derive(Default)]
pub struct SharedBuffersNode {
    command_queue: CommandQueue,
}

impl Node for SharedBuffersNode {
    fn prepare(&mut self, world: &mut World) {
        let mut shared_buffers = world.get_resource_mut::<SharedBuffers>().unwrap();
        self.command_queue.take_commands(shared_buffers.command_queue_mut());
    }

    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn RenderContext,
        _input: &ResourceSlots,
        _output: &mut ResourceSlots,
    ) {
        let shared_buffers = world.get_resource::<SharedBuffers>().unwrap();
        shared_buffers.unmap_buffer(render_context);
        self.command_queue.execute(render_context);
    }
}
