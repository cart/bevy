use crevice::std140::{self, AsStd140};

use crate::renderer::{
    BufferId, BufferInfo, BufferUsage, RenderContext, RenderResourceBinding, RenderResourceContext,
};
pub struct UniformVecBuffer<T: AsStd140> {
    values: Vec<T>,
    staging_buffer: BufferId,
    uniform_buffer: BufferId,
    capacity: usize,
}

impl<T: AsStd140> UniformVecBuffer<T> {
    pub fn new(render_resources: &dyn RenderResourceContext, capacity: usize) -> Self {
        let size = T::std140_size_static() * capacity;
        let staging_buffer = render_resources.create_buffer(BufferInfo {
            size,
            buffer_usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
            mapped_at_creation: true,
        });
        let uniform_buffer = render_resources.create_buffer(BufferInfo {
            size: T::std140_size_static(),
            buffer_usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
            mapped_at_creation: false,
        });

        Self {
            capacity,
            staging_buffer,
            uniform_buffer,
            values: Vec::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn staging_buffer(&self) -> BufferId {
        self.staging_buffer
    }

    #[inline]
    pub fn uniform_buffer(&self) -> BufferId {
        self.uniform_buffer
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn push(&mut self, value: T) -> Result<RenderResourceBinding, T> {
        if self.values.len() < self.capacity {
            let binding = RenderResourceBinding::Buffer {
                buffer: self.uniform_buffer,
                dynamic_index: Some(self.values.len() as u32),
                range: 0..T::std140_size_static() as u64,
            };
            self.values.push(value);
            Ok(binding)
        } else {
            Err(value)
        }
    }

    pub fn reserve(&mut self, capacity: usize, render_resources: &dyn RenderResourceContext) {
        if capacity > self.capacity {
            self.capacity = capacity;
            render_resources.remove_buffer(self.staging_buffer);
            render_resources.remove_buffer(self.uniform_buffer);

            let size = T::std140_size_static() * capacity;
            self.staging_buffer = render_resources.create_buffer(BufferInfo {
                size,
                buffer_usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
                mapped_at_creation: true,
            });
            self.uniform_buffer = render_resources.create_buffer(BufferInfo {
                size: T::std140_size_static(),
                buffer_usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
                mapped_at_creation: false,
            });
        }
    }

    pub fn write_to_staging_buffer(&mut self, render_resources: &dyn RenderResourceContext) {
        let size = self.capacity * T::std140_size_static();
        render_resources.write_mapped_buffer(
            self.staging_buffer,
            0..size as u64,
            &mut |data, _renderer| {
                let mut writer = std140::Writer::new(data);
                let len = self.values.len() as u32;
                writer.write(&len).unwrap();
            },
        );
    }
    pub fn write_to_uniform_buffer(&mut self, render_context: &mut dyn RenderContext) {
        render_context.copy_buffer_to_buffer(
            self.staging_buffer,
            0,
            self.uniform_buffer,
            0,
            (self.capacity * T::std140_size_static()) as u64,
        );
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}
