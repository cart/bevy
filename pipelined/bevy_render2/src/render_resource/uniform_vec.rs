use crate::{render_resource::Buffer, renderer::RenderDevice};
use crevice::std140::{self, AsStd140, DynamicUniform, Std140};
use std::{num::NonZeroU64, ops::{Deref, DerefMut}};
use wgpu::{BindingResource, BufferBinding, BufferDescriptor, BufferUsage, CommandEncoder};

pub struct UniformVec<T: AsStd140> {
    values: Vec<T>,
    staging_buffer: Option<Buffer>,
    uniform_buffer: Option<Buffer>,
    capacity: usize,
    item_size: usize,
}

impl<T: AsStd140> Default for UniformVec<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            staging_buffer: None,
            uniform_buffer: None,
            capacity: 0,
            item_size: (T::std140_size_static() + <T as AsStd140>::Std140Type::ALIGNMENT - 1)
                & !(<T as AsStd140>::Std140Type::ALIGNMENT - 1),
        }
    }
}

impl<T: AsStd140> Deref for UniformVec<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.values.deref()
    }
}

impl<T: AsStd140> DerefMut for UniformVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.values.deref_mut()
    }
}

impl<'a, T: AsStd140 + Clone> Extend<&'a T> for UniformVec<T> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.values
            .extend(iter.into_iter().map(|item| item.clone()));
        if self.values.len() >= self.capacity {
            self.values.truncate(self.capacity);
            panic!(
                "Cannot push values because capacity of {} has been reached",
                self.capacity
            );
        }
    }
}

impl<T: AsStd140> Extend<T> for UniformVec<T> {
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

impl<T: AsStd140> UniformVec<T> {
    #[inline]
    pub fn staging_buffer(&self) -> Option<&Buffer> {
        self.staging_buffer.as_ref()
    }

    #[inline]
    pub fn uniform_buffer(&self) -> Option<&Buffer> {
        self.uniform_buffer.as_ref()
    }

    #[inline]
    pub fn binding(&self) -> BindingResource {
        BindingResource::Buffer(BufferBinding {
            buffer: self.uniform_buffer().expect("uniform buffer should exist"),
            offset: 0,
            size: Some(NonZeroU64::new(self.item_size as u64).unwrap()),
        })
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn push(&mut self, value: T) -> usize {
        let len = self.values.len();
        if len < self.capacity {
            self.values.push(value);
            len
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
            self.staging_buffer = Some(device.create_buffer(&BufferDescriptor {
                label: None,
                size,
                usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
                mapped_at_creation: false,
            }));
            self.uniform_buffer = Some(device.create_buffer(&BufferDescriptor {
                label: None,
                size,
                usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
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

    pub fn write_to_staging_buffer(&self, device: &RenderDevice) {
        if let Some(staging_buffer) = &self.staging_buffer {
            let slice = staging_buffer.slice(..);
            device.map_buffer(&slice, wgpu::MapMode::Write);
            {
                let mut data = slice.get_mapped_range_mut();
                let mut writer = std140::Writer::new(data.deref_mut());
                writer.write(self.values.as_slice()).unwrap();
            }
            staging_buffer.unmap()
        }
    }
    pub fn write_to_uniform_buffer(&self, command_encoder: &mut CommandEncoder) {
        if let (Some(staging_buffer), Some(uniform_buffer)) =
            (&self.staging_buffer, &self.uniform_buffer)
        {
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

pub struct DynamicUniformVec<T: AsStd140> {
    uniform_vec: UniformVec<DynamicUniform<T>>,
}

impl<T: AsStd140> Default for DynamicUniformVec<T> {
    fn default() -> Self {
        Self {
            uniform_vec: Default::default(),
        }
    }
}

impl<T: AsStd140> Deref for DynamicUniformVec<T> {
    type Target = [DynamicUniform<T>];
    fn deref(&self) -> &Self::Target {
        self.uniform_vec.deref()
    }
}

impl<T: AsStd140> DerefMut for DynamicUniformVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.uniform_vec.deref_mut()
    }
}

impl<'a, T: AsStd140 + Clone> Extend<&'a T> for DynamicUniformVec<T> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.uniform_vec
            .extend(iter.into_iter().map(|item| DynamicUniform(item.clone())));
    }
}

impl<T: AsStd140> Extend<T> for DynamicUniformVec<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.uniform_vec
            .extend(iter.into_iter().map(|item| DynamicUniform(item)));
    }
}

impl<T: AsStd140> DynamicUniformVec<T> {
    #[inline]
    pub fn staging_buffer(&self) -> Option<&Buffer> {
        self.uniform_vec.staging_buffer()
    }

    #[inline]
    pub fn uniform_buffer(&self) -> Option<&Buffer> {
        self.uniform_vec.uniform_buffer()
    }

    #[inline]
    pub fn binding(&self) -> BindingResource {
        self.uniform_vec.binding()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.uniform_vec.capacity()
    }

    #[inline]
    pub fn push(&mut self, value: T) -> u32 {
        (self.uniform_vec.push(DynamicUniform(value)) * self.uniform_vec.item_size) as u32
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.uniform_vec.pop().map(|item| item.0)
    }

    #[inline]
    pub fn reserve(&mut self, capacity: usize, device: &RenderDevice) {
        self.uniform_vec.reserve(capacity, device);
    }

    #[inline]
    pub fn reserve_and_clear(&mut self, capacity: usize, device: &RenderDevice) {
        self.uniform_vec.reserve_and_clear(capacity, device);
    }

    #[inline]
    pub fn swap_remove(&mut self, index: usize) {
        self.uniform_vec.swap_remove(index);
    }

    #[inline]
    pub fn truncate(&mut self, length: usize) {
        self.uniform_vec.truncate(length);
    }

    #[inline]
    pub fn write_to_staging_buffer(&self, device: &RenderDevice) {
        self.uniform_vec.write_to_staging_buffer(device);
    }

    #[inline]
    pub fn write_to_uniform_buffer(&self, command_encoder: &mut CommandEncoder) {
        self.uniform_vec.write_to_uniform_buffer(command_encoder);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.uniform_vec.clear();
    }
}
