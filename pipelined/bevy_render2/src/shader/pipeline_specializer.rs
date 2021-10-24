use crate::{
    render_asset::RenderAssets,
    render_phase::{Draw, PhaseItem, TrackedRenderPass},
    render_resource::RenderPipeline,
    renderer::RenderDevice,
    shader::{RenderPipelineCache, Shader},
};
use bevy_asset::Handle;
use bevy_ecs::{
    entity::Entity,
    prelude::{FromWorld, World},
};
use bevy_utils::HashMap;
use std::hash::Hash;
use wgpu::{
    BufferAddress, ColorTargetState, DepthStencilState, FragmentState, InputStepMode, Label,
    MultisampleState, PipelineLayout, PrimitiveState, RenderPipelineDescriptor, VertexAttribute,
    VertexBufferLayout, VertexState,
};

use super::ProcessedShaderCache;

#[derive(Clone, Copy, Hash, Debug)]
pub struct TmpPipelineId(usize);

pub struct RenderPipelines<S: SpecializePipeline> {
    cache: HashMap<S::Key, TmpPipelineId>,
    pipelines: Vec<RenderPipeline>,
    render_device: RenderDevice,
}

impl<S: SpecializePipeline> FromWorld for RenderPipelines<S> {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.get_resource::<RenderDevice>().unwrap().clone();
        Self {
            render_device,
            cache: Default::default(),
            pipelines: Default::default(),
        }
    }
}

impl<S: SpecializePipeline> RenderPipelines<S> {
    pub fn create(
        &mut self,
        cache: &mut RenderPipelineCache, // TODO: rename to pipelinelayoutcache or move into RenderPipelines?
        shader_cache: &mut ProcessedShaderCache,
        shaders: &RenderAssets<Shader>,
        specialize_pipeline: &S,
        key: S::Key,
    ) -> Option<TmpPipelineId> {
        let device = &mut self.render_device;
        let pipelines = &mut self.pipelines;
        match self.cache.entry(key.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => Some(*entry.into_mut()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let specialized = specialize_pipeline.specialize_pipeline(cache, key);
                let vertex_buffer_layouts = specialized
                    .vertex
                    .buffers
                    .iter()
                    .map(|layout| VertexBufferLayout {
                        array_stride: layout.array_stride,
                        attributes: &layout.attributes,
                        step_mode: layout.step_mode,
                    })
                    .collect::<Vec<_>>();
                // TODO: proper error handling here
                let vertex_module = shader_cache.process_shader(
                    shaders,
                    &specialized.vertex.shader,
                    specialized.vertex.shader_defs,
                )?;
                let fragment_data = if let Some(state) = specialized.fragment {
                    let fragment_module =
                        shader_cache.process_shader(shaders, &state.shader, state.shader_defs)?;
                    Some((fragment_module, state.entry_point, state.targets))
                } else {
                    None
                };
                let descriptor = RenderPipelineDescriptor {
                    depth_stencil: specialized.depth_stencil,
                    label: specialized.label,
                    layout: specialized.layout,
                    multisample: specialized.multisample,
                    primitive: specialized.primitive,
                    vertex: VertexState {
                        buffers: &vertex_buffer_layouts,
                        entry_point: specialized.vertex.entry_point,
                        module: &vertex_module,
                    },
                    fragment: fragment_data
                        .as_ref()
                        .map(|(module, entry_point, targets)| FragmentState {
                            entry_point,
                            module: &module,
                            targets,
                        }),
                };

                println!("{:?}", descriptor);
                let id = TmpPipelineId(pipelines.len());
                let pipeline = device.create_render_pipeline(&descriptor);
                pipelines.push(pipeline);
                entry.insert(id);
                Some(id)
            }
        }
    }

    #[inline]
    pub fn get(&self, id: TmpPipelineId) -> Option<&RenderPipeline> {
        self.pipelines.get(id.0)
    }
}

pub trait SpecializePipeline {
    type Key: Clone + Hash + PartialEq + Eq;
    // type Query: Fetch;
    // get_key(item: QueryItem<Self::Query>) -> Self::Key {}
    fn specialize_pipeline<'a>(
        &'a self,
        cache: &'a mut RenderPipelineCache,
        key: Self::Key,
    ) -> SpecializedRenderPipeline<'a>;
    // fn get_draw(key: Self::Key) -> Box<dyn DrawEntity>;
}

pub trait DrawEntity {
    fn draw_entity<'w>(
        &mut self,
        world: &'w World,
        pass: &mut TrackedRenderPass<'w>,
        view: Entity,
        entity: Entity,
    );
}

pub trait EntityPhaseItem: PhaseItem {
    fn phase_item_entity(&self) -> Entity;
}

impl<P: EntityPhaseItem, D: DrawEntity + Send + Sync + 'static> Draw<P> for D {
    fn draw<'w>(
        &mut self,
        world: &'w World,
        pass: &mut TrackedRenderPass<'w>,
        view: Entity,
        item: &P,
    ) {
        self.draw_entity(world, pass, view, item.phase_item_entity())
    }
}

/// Describes a render (graphics) pipeline.
#[derive(Clone, Debug)]
pub struct SpecializedRenderPipeline<'a> {
    /// Debug label of the pipeline. This will show up in graphics debuggers for easy identification.
    pub label: Label<'a>,
    /// The layout of bind groups for this pipeline.
    pub layout: Option<&'a PipelineLayout>,
    /// The compiled vertex stage, its entry point, and the input buffers layout.
    pub vertex: SpecializedVertexState<'a>,
    /// The properties of the pipeline at the primitive assembly and rasterization level.
    pub primitive: PrimitiveState,
    /// The effect of draw calls on the depth and stencil aspects of the output target, if any.
    pub depth_stencil: Option<DepthStencilState>,
    /// The multi-sampling properties of the pipeline.
    pub multisample: MultisampleState,
    /// The compiled fragment stage, its entry point, and the color targets.
    pub fragment: Option<SpecializedFragmentState<'a>>,
}

#[derive(Clone, Debug)]
pub struct SpecializedVertexState<'a> {
    /// The compiled shader module for this stage.
    pub shader: Handle<Shader>,
    pub shader_defs: Vec<String>,
    /// The name of the entry point in the compiled shader. There must be a function that returns
    /// void with this name in the shader.
    pub entry_point: &'a str,
    /// The format of any vertex buffers used with this pipeline.
    pub buffers: Vec<SpecializedVertexBufferLayout>,
}

/// Describes how the vertex buffer is interpreted.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SpecializedVertexBufferLayout {
    /// The stride, in bytes, between elements of this buffer.
    pub array_stride: BufferAddress,
    /// How often this vertex buffer is "stepped" forward.
    pub step_mode: InputStepMode,
    /// The list of attributes which comprise a single vertex.
    pub attributes: Vec<VertexAttribute>,
}

/// Describes the fragment process in a render pipeline.
#[derive(Clone, Debug)]
pub struct SpecializedFragmentState<'a> {
    /// The compiled shader module for this stage.
    pub shader: Handle<Shader>,
    pub shader_defs: Vec<String>,
    /// The name of the entry point in the compiled shader. There must be a function that returns
    /// void with this name in the shader.
    pub entry_point: &'a str,
    /// The color state of the render targets.
    pub targets: Vec<ColorTargetState>,
}
