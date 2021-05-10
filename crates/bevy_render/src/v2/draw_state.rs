use crate::{
    pass::RenderPass,
    pipeline::{BindGroupDescriptorId, IndexFormat, PipelineId},
    renderer::{BindGroupId, BufferId},
};
use std::ops::Range;

/// Tracks the current pipeline state to ensure draw calls are valid.
#[derive(Debug, Default)]
pub struct DrawState {
    pipeline: Option<PipelineId>,
    bind_groups: Vec<Option<BindGroupId>>,
    vertex_buffers: Vec<Option<(BufferId, u64)>>,
    index_buffer: Option<(BufferId, u64, IndexFormat)>,
}

impl DrawState {
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

pub struct TrackedRenderPass<'a> {
    pass: &'a mut dyn RenderPass,
    state: DrawState,
}

impl<'a> TrackedRenderPass<'a> {
    pub fn new(pass: &'a mut dyn RenderPass) -> Self {
        Self {
            state: DrawState::default(),
            pass,
        }
    }
    pub fn set_pipeline(&mut self, pipeline: PipelineId) {
        if self.state.is_pipeline_set(pipeline) {
            return;
        }
        self.pass.set_pipeline_v2(pipeline);
        self.state.set_pipeline(pipeline);
    }

    pub fn set_bind_group(
        &mut self,
        index: usize,
        bind_group_descriptor: BindGroupDescriptorId,
        bind_group: BindGroupId,
        dynamic_uniform_indices: Option<Vec<u32>>,
    ) {
        if dynamic_uniform_indices.is_none()
            && self.state.is_bind_group_set(index as usize, bind_group)
        {
            return;
        }
        self.pass.set_bind_group(
            index as u32,
            bind_group_descriptor,
            bind_group,
            dynamic_uniform_indices.as_deref(),
        );
        self.state.set_bind_group(index as usize, bind_group);
    }

    pub fn set_vertex_buffer(&mut self, index: usize, buffer: BufferId, offset: u64) {
        if self.state.is_vertex_buffer_set(index, buffer, offset) {
            return;
        }
        self.pass.set_vertex_buffer(index as u32, buffer, offset);
        self.state.set_vertex_buffer(index, buffer, offset);
    }

    pub fn set_index_buffer(&mut self, buffer: BufferId, offset: u64, index_format: IndexFormat) {
        if self.state.is_index_buffer_set(buffer, offset, index_format) {
            return;
        }
        self.pass.set_index_buffer(buffer, offset, index_format);
        self.state.set_index_buffer(buffer, offset, index_format);
    }

    pub fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.pass.draw_indexed(indices, base_vertex, instances);
    }
}
