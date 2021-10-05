use crate::{
    render_asset::RenderAssets,
    render_resource::{BindGroupLayout, RenderPipeline},
    renderer::RenderDevice,
    shader::Shader,
};
use bevy_asset::Handle;
use bevy_utils::{AHasher, HashMap, HashSet};
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use wgpu::{
    ColorTargetState, DepthStencilState, FragmentState, IndexFormat, MultisampleState,
    PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology, RenderPipelineDescriptor,
    ShaderModule, VertexBufferLayout, VertexState,
};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct PipelineLayoutKey(u64);

impl From<&[&BindGroupLayout]> for PipelineLayoutKey {
    fn from(bind_group_layouts: &[&BindGroupLayout]) -> Self {
        let mut hasher = AHasher::new_with_keys(42, 23);
        for bind_group_layout in bind_group_layouts {
            bind_group_layout.id().hash(&mut hasher);
        }
        Self(hasher.finish())
    }
}

// TODO: rename to MeshDescriptor?
/// Describes how the vertex buffer is interpreted.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct VertexBufferLayoutDescriptor {
    /// The stride, in bytes, between elements of this buffer.
    pub array_stride: wgpu::BufferAddress,
    /// How often this vertex buffer is "stepped" forward.
    pub step_mode: wgpu::InputStepMode,
    /// The list of attributes which comprise a single vertex.
    pub attributes: Vec<wgpu::VertexAttribute>,
    pub topology: PrimitiveTopology,
    pub strip_index_format: Option<IndexFormat>,
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct VertexBufferLayoutKey(u64);

impl<'a> From<&VertexBufferLayoutDescriptor> for VertexBufferLayoutKey {
    fn from(vertex_buffer_layout: &VertexBufferLayoutDescriptor) -> Self {
        let mut hasher = AHasher::new_with_keys(42, 23);
        vertex_buffer_layout.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct ShaderDefsKey(u64);

// TODO: this _must_ be sorted to work!
impl ShaderDefsKey {
    pub fn new(shader_defs: &[&str]) -> Self {
        let mut hasher = AHasher::new_with_keys(42, 23);
        shader_defs.hash(&mut hasher);
        Self(hasher.finish())
    }
}

pub struct VertexDescriptor {
    pub shader: Handle<Shader>,
    pub entry_point: String,
}

pub struct FragmentDescriptor {
    pub shader: Handle<Shader>,
    pub entry_point: String,
    pub targets: Vec<ColorTargetState>,
}

pub struct PipelineBundle {
    pub vertex: VertexDescriptor,
    pub fragment: Option<FragmentDescriptor>,
    pub depth_stencil: Option<DepthStencilState>,
    pub primitive: PrimitiveDescriptor,
}

pub struct PrimitiveDescriptor {
    pub front_face: wgpu::FrontFace,
    /// The face culling mode.
    pub cull_mode: Option<wgpu::Face>,
    /// If set to true, the polygon depth is clamped to 0-1 range instead of being clipped.
    ///
    /// Enabling this requires `Features::DEPTH_CLAMPING` to be enabled.
    pub clamp_depth: bool,
    /// Controls the way each polygon is rasterized. Can be either `Fill` (default), `Line` or `Point`
    ///
    /// Setting this to something other than `Fill` requires `Features::NON_FILL_POLYGON_MODE` to be enabled.
    pub polygon_mode: wgpu::PolygonMode,
    /// If set to true, the primitives are rendered with conservative overestimation. I.e. any rastered pixel touched by it is filled.
    /// Only valid for PolygonMode::Fill!
    ///
    /// Enabling this requires `Features::CONSERVATIVE_RASTERIZATION` to be enabled.
    pub conservative: bool,
}

#[derive(Default)]
pub struct ShaderData {
    pipelines: HashSet<PipelineBundleId>,
    processed_shaders: HashMap<ShaderDefsKey, Arc<ShaderModule>>,
}

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct SpecializedPipelineKey {
    pub vertex_buffer_layout: VertexBufferLayoutKey,
    pub pipeline_layout: PipelineLayoutKey,
    pub shader_defs: ShaderDefsKey,
    pub multisample_count: u32, // TODO: this should probably be a full hash of the MultiSampleState
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct PipelineBundleId(usize);

pub struct PipelineBundleData {
    specialized_pipelines: HashMap<SpecializedPipelineKey, SpecializedPipelineId>,
    bundle: PipelineBundle,
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct SpecializedPipelineId(usize);

impl SpecializedPipelineId {
    pub const INVALID: Self = SpecializedPipelineId(usize::MAX); 
}

pub struct RenderPipelineCache {
    pipeline_layouts: HashMap<PipelineLayoutKey, wgpu::PipelineLayout>,
    vertex_buffer_layouts: HashMap<VertexBufferLayoutKey, VertexBufferLayoutDescriptor>,
    pipeline_bundles: HashMap<PipelineBundleId, PipelineBundleData>,
    shader_pipelines: HashMap<Handle<Shader>, ShaderData>,
    next_pipeline_bundle_id: usize,
    render_device: RenderDevice,
    pipelines: Vec<RenderPipeline>,
}

pub enum SpecializationError {
    MissingShader,
    MissingPipelineBundle,
    MissingVertexBufferLayout,
    MissingPipelineLayout,
}

impl RenderPipelineCache {
    pub fn new(render_device: RenderDevice) -> Self {
        Self {
            render_device,
            next_pipeline_bundle_id: 0,
            pipeline_layouts: Default::default(),
            vertex_buffer_layouts: Default::default(),
            pipeline_bundles: Default::default(),
            shader_pipelines: Default::default(),
            pipelines: Default::default(),
        }
    }

    pub fn add_bundle(&mut self, bundle: PipelineBundle) -> PipelineBundleId {
        let id = PipelineBundleId(self.next_pipeline_bundle_id);
        self.next_pipeline_bundle_id += 1;
        {
            let shader_data = self
                .shader_pipelines
                .entry(bundle.vertex.shader.clone_weak())
                .or_default();
            shader_data.pipelines.insert(id);
        }
        if let Some(fragment) = &bundle.fragment {
            let shader_data = self
                .shader_pipelines
                .entry(fragment.shader.clone_weak())
                .or_default();
            shader_data.pipelines.insert(id);
        }
        self.pipeline_bundles.insert(
            id,
            PipelineBundleData {
                bundle,
                specialized_pipelines: Default::default(),
            },
        );
        id
    }

    #[inline]
    pub fn get_pipeline(&self, id: SpecializedPipelineId) -> Option<&RenderPipeline> {
        self.pipelines.get(id.0)
    }

    pub fn specialize(
        &mut self,
        pipelined_bundle: PipelineBundleId,
        shaders: &RenderAssets<Shader>,
        key: SpecializedPipelineKey,
    ) -> Result<SpecializedPipelineId, SpecializationError> {
        let bundle_data = self
            .pipeline_bundles
            .get_mut(&pipelined_bundle)
            .ok_or(SpecializationError::MissingPipelineBundle)?;
        match bundle_data.specialized_pipelines.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => Ok(*entry.get()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let pipeline_layout = self
                    .pipeline_layouts
                    .get(&key.pipeline_layout)
                    .ok_or(SpecializationError::MissingPipelineLayout)?;
                let vertex_buffer_layout = self
                    .vertex_buffer_layouts
                    .get(&key.vertex_buffer_layout)
                    .ok_or(SpecializationError::MissingVertexBufferLayout)?;
                let primitive = &bundle_data.bundle.primitive;
                let render_device = &self.render_device;
                let vertex_module = {
                    let vertex_shader_handle = &bundle_data.bundle.vertex.shader;
                    let vertex_shader = shaders
                        .get(vertex_shader_handle)
                        .ok_or(SpecializationError::MissingShader)?;
                    let vertex_data = self
                        .shader_pipelines
                        .entry(vertex_shader_handle.clone_weak())
                        .or_default();
                    vertex_data
                        .processed_shaders
                        .entry(key.shader_defs)
                        .or_insert_with(|| {
                            // TODO: pass in shader defs here and handle process errors properly
                            let processed = vertex_shader.process(&[]).unwrap();
                            Arc::new(render_device.create_shader_module(&processed))
                        })
                        .clone()
                };
                let shader_pipelines = &mut self.shader_pipelines;
                let fragment = bundle_data
                    .bundle
                    .fragment
                    .as_ref()
                    .map(|fragment| {
                        let fragment_shader = shaders.get(&fragment.shader)?;
                        let fragment_data = shader_pipelines
                            .entry(fragment.shader.clone_weak())
                            .or_default();
                        let fragment_module = fragment_data
                            .processed_shaders
                            .entry(key.shader_defs)
                            .or_insert_with(|| {
                                // TODO: pass in shader defs here and handle process errors properly
                                let processed = fragment_shader.process(&[]).unwrap();
                                Arc::new(render_device.create_shader_module(&processed))
                            })
                            .clone();
                        Some((fragment, fragment_module))
                    })
                    .ok_or(SpecializationError::MissingShader)?;
                let pipeline =
                    self.render_device
                        .create_render_pipeline(&RenderPipelineDescriptor {
                            depth_stencil: bundle_data.bundle.depth_stencil.clone(),
                            label: None,
                            layout: Some(pipeline_layout),
                            primitive: PrimitiveState {
                                topology: vertex_buffer_layout.topology,
                                strip_index_format: vertex_buffer_layout.strip_index_format,
                                front_face: primitive.front_face,
                                cull_mode: primitive.cull_mode,
                                clamp_depth: primitive.clamp_depth,
                                polygon_mode: primitive.polygon_mode,
                                conservative: primitive.conservative,
                            },
                            vertex: VertexState {
                                module: &vertex_module,
                                entry_point: &bundle_data.bundle.vertex.entry_point,
                                buffers: &[VertexBufferLayout {
                                    array_stride: vertex_buffer_layout.array_stride,
                                    step_mode: vertex_buffer_layout.step_mode,
                                    attributes: &vertex_buffer_layout.attributes,
                                }],
                            },
                            fragment: fragment.as_ref().map(|(fragment, module)| FragmentState {
                                module,
                                entry_point: &fragment.entry_point,
                                targets: &fragment.targets,
                            }),
                            multisample: MultisampleState {
                                count: key.multisample_count,
                                ..Default::default()
                            },
                        });
                let id = SpecializedPipelineId(self.pipelines.len());
                entry.insert(id);
                self.pipelines.push(pipeline);
                Ok(id)
            }
        }
    }

    pub fn get_specialized(&self, id: SpecializedPipelineId) -> Option<&RenderPipeline> {
        self.pipelines.get(id.0)
    }

    pub fn get_or_insert_pipeline_layout<'a>(
        &'a mut self,
        bind_group_layouts: &[&BindGroupLayout],
    ) -> PipelineLayoutKey {
        let key = PipelineLayoutKey::from(bind_group_layouts);
        let render_device = &self.render_device;
        self.pipeline_layouts.entry(key).or_insert_with(|| {
            let bind_group_layouts = bind_group_layouts
                .iter()
                .map(|l| l.value())
                .collect::<Vec<_>>();
            render_device.create_pipeline_layout(&PipelineLayoutDescriptor {
                bind_group_layouts: &bind_group_layouts,
                ..Default::default()
            })
        });
        key
    }

    pub fn get_or_insert_vertex_buffer_layout(
        &mut self,
        vertex_buffer_layout: VertexBufferLayoutDescriptor,
    ) -> VertexBufferLayoutKey {
        let key = VertexBufferLayoutKey::from(&vertex_buffer_layout);
        self.vertex_buffer_layouts
            .entry(key)
            .or_insert(vertex_buffer_layout);
        key
    }
}
