use crate::{render_resource::Buffer, renderer::RenderDevice};
use bevy_core::{cast_slice, Pod};
use wgpu::BufferUsage;
use std::ops::{Deref, DerefMut};

pub struct BufferVec<T: Pod> {
    values: Vec<T>,
    staging_buffer: Option<Buffer>,
    buffer: Option<Buffer>,
    capacity: usize,
    item_size: usize,
    buffer_usage: BufferUsage,
}

impl<T: Pod> Default for BufferVec<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            staging_buffer: None,
            buffer: None,
            capacity: 0,
            item_size: std::mem::size_of::<T>(),
            buffer_usage: BufferUsage::all(),
        }
    }
}

impl<T: Pod> Deref for BufferVec<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.values.deref()
    }
}

impl<T: Pod> DerefMut for BufferVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.values.deref_mut()
    }
}

impl<'a, T: Pod> Extend<&'a T> for BufferVec<T> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.values.extend(iter);
        if self.values.len() >= self.capacity {
            self.values.truncate(self.capacity);
            panic!(
                "Cannot push values because capacity of {} has been reached",
                self.capacity
            );
        }
    }
}

impl<T: Pod> Extend<T> for BufferVec<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.values.extend(iter);
        if self.values.len() >= self.capacity {
            self.values.truncate(self.capacity);
            panic!(
                "Cannot push values because capacity of {} has been reached",
                self.capacity
            );
        }
    }
}

impl<T: Pod> BufferVec<T> {
    pub fn new(buffer_usage: BufferUsage) -> Self {
        Self {
            buffer_usage,
            ..Default::default()
        }
    }

    #[inline]
    pub fn staging_buffer(&self) -> Option<&Buffer> {
        self.staging_buffer.as_ref()
    }

    #[inline]
    pub fn buffer(&self) -> Option<&Buffer> {
        self.buffer.as_ref()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn push(&mut self, value: T) -> usize {
        if self.values.len() < self.capacity {
            let index = self.values.len();
            self.values.push(value);
            index
        } else {
            panic!(
                "Cannot push value because capacity of {} has been reached",
                self.capacity
            );
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }

    pub fn reserve(&mut self, capacity: usize, device: &RenderDevice) {
        if capacity > self.capacity {
            self.capacity = capacity;
            let size = (self.item_size * capacity) as wgpu::BufferAddress;
            self.staging_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
                mapped_at_creation: false,
            }));
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: BufferUsage::COPY_DST | self.buffer_usage,
                mapped_at_creation: false,
            }));
        }
    }

    pub fn reserve_and_clear(&mut self, capacity: usize, device: &RenderDevice) {
        self.clear();
        self.reserve(capacity, device);
    }

    pub fn swap_remove(&mut self, index: usize) {
        self.values.swap_remove(index);
    }

    pub fn truncate(&mut self, length: usize) {
        self.values.truncate(length);
    }

    pub fn write_to_staging_buffer(&self, render_device: &RenderDevice) {
        if let Some(staging_buffer) = &self.staging_buffer {
            let slice = staging_buffer.slice(..);
            render_device.map_buffer(&slice, wgpu::MapMode::Write);
            {
                let mut data = slice.get_mapped_range_mut();
                let bytes: &[u8] = cast_slice(&self.values);
                data.copy_from_slice(bytes);
            }
            staging_buffer.unmap();
        }
    }
    pub fn write_to_buffer(&self, command_encoder: &mut wgpu::CommandEncoder) {
        if let (Some(staging_buffer), Some(uniform_buffer)) = (&self.staging_buffer, &self.buffer) {
            command_encoder.copy_buffer_to_buffer(
                staging_buffer,
                0,
                uniform_buffer,
                0,
                (self.values.len() * self.item_size) as u64,
            );
        }
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}
