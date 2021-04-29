use std::{ops::Range, sync::Arc};

use crate::{
    pass::RenderPass,
    pipeline::{BindGroupDescriptorId, IndexFormat, PipelineId},
    renderer::{BindGroupId, BufferId},
};
/// A queued command for the renderer
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RenderCommand {
    SetPipeline {
        pipeline: PipelineId,
    },
    SetVertexBuffer {
        slot: u32,
        buffer: BufferId,
        offset: u64,
    },
    SetIndexBuffer {
        buffer: BufferId,
        offset: u64,
        index_format: IndexFormat,
    },
    SetBindGroup {
        index: u32,
        bind_group_descriptor: BindGroupDescriptorId,
        bind_group: BindGroupId,
        dynamic_uniform_indices: Option<Vec<u32>>,
    },
    DrawIndexed {
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    },
    Draw {
        vertices: Range<u32>,
        instances: Range<u32>,
    },
}

/// A component that indicates how to draw an entity.
#[derive(Debug, Clone)]
pub struct Draw {
    pub render_commands: Vec<RenderCommand>,
}

impl Default for Draw {
    fn default() -> Self {
        Self {
            render_commands: Default::default(),
        }
    }
}

impl Draw {
    pub fn clear_render_commands(&mut self) {
        self.render_commands.clear();
    }

    pub fn set_pipeline(&mut self, pipeline: PipelineId) {
        self.render_command(RenderCommand::SetPipeline {
            pipeline,
        });
    }

    pub fn set_vertex_buffer(&mut self, slot: u32, buffer: BufferId, offset: u64) {
        self.render_command(RenderCommand::SetVertexBuffer {
            slot,
            buffer,
            offset,
        });
    }

    pub fn set_index_buffer(&mut self, buffer: BufferId, offset: u64, index_format: IndexFormat) {
        self.render_command(RenderCommand::SetIndexBuffer {
            buffer,
            offset,
            index_format,
        });
    }

    pub fn set_bind_group(&mut self, index: u32, bind_group_descriptor: BindGroupDescriptorId, bind_group: BindGroupId, dynamic_uniform_indices: Option<Vec<u32>>) {
        self.render_command(RenderCommand::SetBindGroup {
            index,
            bind_group_descriptor,
            bind_group,
            dynamic_uniform_indices,
        });
    }

    pub fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.render_command(RenderCommand::DrawIndexed {
            base_vertex,
            indices,
            instances,
        });
    }

    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.render_command(RenderCommand::Draw {
            vertices,
            instances,
        });
    }

    #[inline]
    pub fn render_command(&mut self, render_command: RenderCommand) {
        self.render_commands.push(render_command);
    }
}

/// Tracks the current pipeline state to ensure draw calls are valid.
#[derive(Debug, Default)]
pub struct DrawState {
    pipeline: Option<PipelineId>,
    bind_groups: Vec<Option<BindGroupId>>,
    vertex_buffers: Vec<Option<(BufferId, u64)>>,
    index_buffer: Option<(BufferId, u64, IndexFormat)>,
}

impl DrawState {
    pub fn draw(&mut self, draw: &Draw, render_pass: &mut dyn RenderPass) {
        for command in draw.render_commands.iter() {
            match command.clone() {
                RenderCommand::SetPipeline { pipeline } => {
                    if self.is_pipeline_set(pipeline) {
                        continue;
                    }
                    render_pass.set_pipeline_v2(pipeline);
                    self.set_pipeline(pipeline);
                }
                RenderCommand::DrawIndexed {
                    base_vertex,
                    indices,
                    instances,
                } => {
                    // if self.can_draw_indexed() {
                    render_pass.draw_indexed(indices, base_vertex, instances);
                    // } else {
                    //     debug!("Could not draw indexed because the pipeline layout wasn't fully set for pipeline: {:?}", draw_state.pipeline);
                    // }
                }
                RenderCommand::Draw {
                    vertices,
                    instances,
                } => {
                    // if draw_state.can_draw() {
                    render_pass.draw(vertices, instances);
                    // } else {
                    //     debug!("Could not draw because the pipeline layout wasn't fully set for pipeline: {:?}", draw_state.pipeline);
                    // }
                }
                RenderCommand::SetVertexBuffer {
                    buffer,
                    offset,
                    slot,
                } => {
                    if self.is_vertex_buffer_set(slot as usize, buffer, offset) {
                        continue;
                    }
                    render_pass.set_vertex_buffer(slot, buffer, offset);
                    self.set_vertex_buffer(slot as usize, buffer, offset);
                }
                RenderCommand::SetIndexBuffer {
                    buffer,
                    offset,
                    index_format,
                } => {
                    if self.is_index_buffer_set(buffer, offset, index_format) {
                        continue;
                    }
                    render_pass.set_index_buffer(buffer, offset, index_format);
                    self.set_index_buffer(buffer, offset, index_format);
                }
                RenderCommand::SetBindGroup {
                    index,
                    bind_group_descriptor,
                    bind_group,
                    dynamic_uniform_indices,
                } => {
                    if dynamic_uniform_indices.is_none()
                        && self.is_bind_group_set(index as usize, bind_group)
                    {
                        continue;
                    }
                    // let pipeline = pipelines
                    //     .get(draw_state.pipeline.as_ref().unwrap())
                    //     .unwrap();
                    // let layout = pipeline.get_layout().unwrap();
                    // let bind_group_descriptor = layout.get_bind_group(index).unwrap();
                    render_pass.set_bind_group(
                        index,
                        bind_group_descriptor,
                        bind_group,
                        dynamic_uniform_indices.as_deref(),
                    );
                    self.set_bind_group(index as usize, bind_group);
                }
            }
        }
    }
    pub fn set_bind_group(&mut self, index: usize, bind_group: BindGroupId) {
        if index >= self.bind_groups.len() {
            self.bind_groups.resize(index + 1, None);
        }
        self.bind_groups[index] = Some(bind_group);
    }

    pub fn is_bind_group_set(&self, index: usize, bind_group: BindGroupId) -> bool {
        if let Some(current_bind_group) = self.bind_groups.get(index) {
            *current_bind_group == Some(bind_group)
        } else {
            false
        }
    }

    pub fn set_vertex_buffer(&mut self, index: usize, buffer: BufferId, offset: u64) {
        if index >= self.vertex_buffers.len() {
            self.vertex_buffers.resize(index + 1, None);
        }
        self.vertex_buffers[index] = Some((buffer, offset));
    }

    pub fn is_vertex_buffer_set(&self, index: usize, buffer: BufferId, offset: u64) -> bool {
        if let Some(current) = self.vertex_buffers.get(index) {
            *current == Some((buffer, offset))
        } else {
            false
        }
    }

    pub fn set_index_buffer(&mut self, buffer: BufferId, offset: u64, index_format: IndexFormat) {
        self.index_buffer = Some((buffer, offset, index_format));
    }

    pub fn is_index_buffer_set(
        &self,
        buffer: BufferId,
        offset: u64,
        index_format: IndexFormat,
    ) -> bool {
        self.index_buffer == Some((buffer, offset, index_format))
    }

    pub fn can_draw(&self) -> bool {
        self.bind_groups.iter().all(|b| b.is_some())
            && self.vertex_buffers.iter().all(|v| v.is_some())
    }

    pub fn can_draw_indexed(&self) -> bool {
        self.can_draw() && self.index_buffer.is_some()
    }

    pub fn is_pipeline_set(&self, pipeline: PipelineId) -> bool {
        self.pipeline == Some(pipeline)
    }

    pub fn set_pipeline(&mut self, pipeline: PipelineId) {
        // TODO: do these need to be cleared?
        // self.bind_groups.clear();
        // self.vertex_buffers.clear();
        // self.index_buffer = None;
        self.pipeline = Some(pipeline);
    }
}
