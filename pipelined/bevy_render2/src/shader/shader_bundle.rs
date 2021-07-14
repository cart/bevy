use crate::{render_asset::RenderAssets, renderer::RenderDevice, shader::Shader};
use bevy_asset::{Assets, Handle};
use bevy_utils::HashMap;
use std::{borrow::Cow, sync::Arc};
use wgpu::{
    ColorTargetState, DepthStencilState, FragmentState, MultisampleState, PipelineLayout,
    PrimitiveState, RenderPipelineDescriptor, ShaderModule, VertexBufferLayout, VertexState,
};

#[derive(Hash, PartialEq, Eq)]
struct CompiledShaderKey {
    shader_defs: Cow<'static, [String]>,
    handle: Handle<Shader>,
}

#[derive(Default)]
pub struct CompiledShaders {
    shaders: HashMap<CompiledShaderKey, Arc<ShaderModule>>,
}

impl CompiledShaders {
    pub fn compile_shader(
        &mut self,
        render_device: &RenderDevice,
        handle: &Handle<Shader>,
        shaders: &RenderAssets<Shader>,
        shader_defs: impl Into<Cow<'static, [String]>>,
    ) -> Option<Arc<ShaderModule>> {
        let shader_defs = shader_defs.into();
        let key = CompiledShaderKey {
            handle: handle.clone_weak(),
            shader_defs: shader_defs.clone(),
        };
        match self.shaders.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => Some(entry.get().clone()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let shader = shaders.get(handle)?;
                let processed_shader = shader.process(&shader_defs)?;
                let module = Arc::new(render_device.create_shader_module(&processed_shader));
                Some(entry.insert(module).clone())
            }
        }
    }
}

pub struct RenderPipelineBundle {
    pub vertex: VertexDescriptor,
    pub fragment: Option<FragmentDescriptor>,

    // TODO: needs to remove entry point and module?
    // fragment: Option<FragmentState>,
    pub depth_stencil: Option<DepthStencilState>,
    // TODO: from mesh?
    // primitive: PrimitiveState,
    // TODO: from global state?
    // multisample: MultisampleState,
}

pub struct VertexDescriptor {
    pub entry_point: String,
    pub handle: Handle<Shader>,
}

pub struct FragmentDescriptor {
    pub entry_point: String,
    pub handle: Handle<Shader>,
    pub targets: Vec<ColorTargetState>,
}

impl RenderPipelineBundle {
    pub fn compile(
        &self,
        compiled_shaders: &mut CompiledShaders,
        shaders: &RenderAssets<Shader>,
        render_device: &RenderDevice,
        shader_defs: impl Into<Cow<'static, [String]>>,
    ) -> Option<CompiledRenderPipelineBundle> {
        let shader_defs = shader_defs.into();
        let vertex_module = compiled_shaders.compile_shader(
            render_device,
            &self.vertex.handle,
            shaders,
            shader_defs.clone(),
        )?;
        Some(CompiledRenderPipelineBundle {
            vertex: CompiledVertexDescriptor {
                module: vertex_module,
                entry_point: self.vertex.entry_point.clone(),
            },
            fragment: match &self.fragment {
                Some(fragment) => Some(CompiledFragmentDescriptor {
                    entry_point: fragment.entry_point.clone(),
                    module: compiled_shaders.compile_shader(
                        render_device,
                        &fragment.handle,
                        shaders,
                        shader_defs.clone(),
                    )?,
                    targets: fragment.targets.clone(),
                }),
                None => None,
            },
            depth_stencil: self.depth_stencil.clone(),
        })
    }
}


#[derive(Debug, Clone)]
pub struct CompiledRenderPipelineBundle {
    vertex: CompiledVertexDescriptor,
    fragment: Option<CompiledFragmentDescriptor>,

    // TODO: needs to remove entry point and module?
    // fragment: Option<FragmentState>,
    depth_stencil: Option<DepthStencilState>,
    // TODO: from mesh?
    // primitive: PrimitiveState,
    // TODO: from global state?
    // multisample: MultisampleState,
}

impl CompiledRenderPipelineBundle {
    pub fn get_descriptor<'a>(
        &'a self,
        vertex_buffers: &'a [VertexBufferLayout],
        primitive: PrimitiveState,
        pipeline_layout: Option<&'a PipelineLayout>,
        multisample: MultisampleState,
    ) -> RenderPipelineDescriptor<'a> {
        RenderPipelineDescriptor {
            label: None,
            vertex: VertexState {
                buffers: vertex_buffers,
                module: &self.vertex.module,
                entry_point: &self.vertex.entry_point,
            },
            fragment: self.fragment.as_ref().map(|fragment| FragmentState {
                entry_point: &fragment.entry_point,
                module: &fragment.module,
                targets: &fragment.targets,
            }),
            depth_stencil: self.depth_stencil.clone(),
            layout: pipeline_layout,
            multisample,
            primitive,
        }
    }
}

#[derive(Debug, Clone)]
struct CompiledVertexDescriptor {
    entry_point: String,
    module: Arc<ShaderModule>,
}

#[derive(Debug, Clone)]
struct CompiledFragmentDescriptor {
    entry_point: String,
    module: Arc<ShaderModule>,
    targets: Vec<ColorTargetState>,
}
