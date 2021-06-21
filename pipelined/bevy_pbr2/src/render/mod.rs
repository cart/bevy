mod light;
use bevy_utils::HashMap;
pub use light::*;

use crate::{StandardMaterial, StandardMaterialUniformData};
use bevy_asset::{Assets, Handle};
use bevy_ecs::{prelude::*, system::SystemState};
use bevy_math::Mat4;
use bevy_render2::{
    core_pipeline::Transparent3dPhase,
    mesh::Mesh,
    pipeline::*,
    render_command::RenderCommandQueue,
    render_graph::{Node, NodeRunError, RenderGraphContext},
    render_phase::{Draw, DrawFunctions, Drawable, RenderPhase, TrackedRenderPass},
    render_resource::{
        BindGroupBuilder, BindGroupId, BufferId, BufferInfo, BufferUsage, DynamicUniformVec,
        RenderResourceBinding, SamplerId, TextureId, TextureViewId,
    },
    renderer::{RenderContext, RenderResources},
    shader::{Shader, ShaderStage, ShaderStages},
    texture::{
        SamplerDescriptor, Texture, TextureDescriptor, TextureFormat, TextureGpuData,
        TextureSampleType, TextureViewDescriptor,
    },
    view::{ExtractedView, ViewMeta, ViewUniform},
};
use bevy_transform::components::GlobalTransform;
use crevice::std140::AsStd140;

pub struct PbrShaders {
    pipeline: PipelineId,
    pipeline_descriptor: RenderPipelineDescriptor,
    // This dummy white texture is to be used in place of optional StandardMaterial textures
    dummy_white_texture: TextureId,
    dummy_white_texture_view: TextureViewId,
    dummy_white_sampler: SamplerId,
}

// TODO: this pattern for initializing the shaders / pipeline isn't ideal. this should be handled by the asset system
impl FromWorld for PbrShaders {
    fn from_world(world: &mut World) -> Self {
        let render_resources = world.get_resource::<RenderResources>().unwrap();
        let vertex_shader = Shader::from_glsl(ShaderStage::Vertex, include_str!("pbr.vert"))
            .get_spirv_shader(None)
            .unwrap();
        let fragment_shader = Shader::from_glsl(ShaderStage::Fragment, include_str!("pbr.frag"))
            .get_spirv_shader(None)
            .unwrap();

        let vertex_layout = vertex_shader.reflect_layout(true).unwrap();
        let fragment_layout = fragment_shader.reflect_layout(true).unwrap();

        let mut pipeline_layout =
            PipelineLayout::from_shader_layouts(&mut [vertex_layout, fragment_layout]);

        let vertex = render_resources.create_shader_module(&vertex_shader);
        let fragment = render_resources.create_shader_module(&fragment_shader);

        pipeline_layout.vertex_buffer_descriptors = vec![VertexBufferLayout {
            stride: 32,
            name: "Vertex".into(),
            step_mode: InputStepMode::Vertex,
            attributes: vec![
                // GOTCHA! Vertex_Position isn't first in the buffer due to how Mesh sorts attributes (alphabetically)
                VertexAttribute {
                    name: "Vertex_Position".into(),
                    format: VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 0,
                },
                VertexAttribute {
                    name: "Vertex_Normals".into(),
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 1,
                },
                VertexAttribute {
                    name: "Vertex_Uv".into(),
                    format: VertexFormat::Float32x2,
                    offset: 24,
                    shader_location: 2,
                },
            ],
        }];

        pipeline_layout.bind_group_mut(0).bindings[0].set_dynamic(true);
        pipeline_layout.bind_group_mut(0).bindings[1].set_dynamic(true);
        if let BindType::Texture { sample_type, .. } =
            &mut pipeline_layout.bind_group_mut(0).bindings[2].bind_type
        {
            *sample_type = TextureSampleType::Depth;
        }
        if let BindType::Sampler { comparison, .. } =
            &mut pipeline_layout.bind_group_mut(0).bindings[3].bind_type
        {
            *comparison = true;
        }
        pipeline_layout.bind_group_mut(1).bindings[0].set_dynamic(true);

        pipeline_layout.update_bind_group_ids();

        let pipeline_descriptor = RenderPipelineDescriptor {
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil: StencilState {
                    front: StencilFaceState::IGNORE,
                    back: StencilFaceState::IGNORE,
                    read_mask: 0,
                    write_mask: 0,
                },
                bias: DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            }),
            color_target_states: vec![ColorTargetState {
                format: TextureFormat::default(),
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }),
                write_mask: ColorWrite::ALL,
            }],
            ..RenderPipelineDescriptor::new(
                ShaderStages {
                    vertex,
                    fragment: Some(fragment),
                },
                pipeline_layout,
            )
        };

        let pipeline = render_resources.create_render_pipeline(&pipeline_descriptor);

        // A 1x1x1 'all 1.0' texture to use as a dummy texture to use in place of optional StandardMaterial textures
        let (dummy_white_texture, dummy_white_texture_view, dummy_white_sampler) = {
            let texture_descriptor = TextureDescriptor::default();
            let texture_id = render_resources.create_texture(texture_descriptor);
            let sampler_id = render_resources.create_sampler(&SamplerDescriptor::default());

            let width = texture_descriptor.size.width as usize;
            let aligned_width = render_resources.get_aligned_texture_size(width);
            let format_size = texture_descriptor.format.pixel_size();
            let aligned_data = vec![
                255;
                format_size
                    * aligned_width
                    * texture_descriptor.size.height as usize
                    * texture_descriptor.size.depth_or_array_layers as usize
            ];
            let staging_buffer_id = render_resources.create_buffer_with_data(
                BufferInfo {
                    buffer_usage: BufferUsage::COPY_SRC,
                    ..Default::default()
                },
                &aligned_data,
            );

            let texture_view_id =
                render_resources.create_texture_view(texture_id, TextureViewDescriptor::default());

            let mut render_command_queue = world.get_resource_mut::<RenderCommandQueue>().unwrap();
            render_command_queue.copy_buffer_to_texture(
                staging_buffer_id,
                0,
                (format_size * aligned_width) as u32,
                texture_id,
                [0, 0, 0],
                0,
                texture_descriptor.size,
            );
            render_command_queue.free_buffer(staging_buffer_id);

            (texture_id, texture_view_id, sampler_id)
        };
        PbrShaders {
            pipeline,
            pipeline_descriptor,
            dummy_white_texture,
            dummy_white_texture_view,
            dummy_white_sampler,
        }
    }
}

struct ExtractedStandardMaterialTextures {
    base_color_texture: Option<TextureGpuData>,
    emissive_texture: Option<TextureGpuData>,
    metallic_roughness_texture: Option<TextureGpuData>,
    occlusion_texture: Option<TextureGpuData>,
}

struct ExtractedMesh {
    transform: Mat4,
    vertex_buffer: BufferId,
    index_info: Option<IndexInfo>,
    transform_binding_offset: u32,
    material_buffer: BufferId,
    material_textures: ExtractedStandardMaterialTextures,
}

struct IndexInfo {
    buffer: BufferId,
    count: u32,
}

pub struct ExtractedMeshes {
    meshes: Vec<ExtractedMesh>,
}

pub fn extract_meshes(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    textures: Res<Assets<Texture>>,
    query: Query<(&GlobalTransform, &Handle<Mesh>, &Handle<StandardMaterial>)>,
) {
    let mut extracted_meshes = Vec::new();
    for (transform, mesh_handle, material_handle) in query.iter() {
        if let Some(mesh) = meshes.get(mesh_handle) {
            if let Some(mesh_gpu_data) = &mesh.gpu_data() {
                if let Some(material) = materials.get(material_handle) {
                    if let Some(material_gpu_data) = &material.gpu_data() {
                        extracted_meshes.push(ExtractedMesh {
                            transform: transform.compute_matrix(),
                            vertex_buffer: mesh_gpu_data.vertex_buffer,
                            index_info: mesh_gpu_data.index_buffer.map(|i| IndexInfo {
                                buffer: i,
                                count: mesh.indices().unwrap().len() as u32,
                            }),
                            transform_binding_offset: 0,
                            material_buffer: material_gpu_data.buffer,
                            material_textures: ExtractedStandardMaterialTextures {
                                base_color_texture: material.base_color_texture.as_ref().map_or(
                                    None,
                                    |texture_handle| {
                                        textures
                                            .get(texture_handle)
                                            .map_or(None, |texture| texture.gpu_data.clone())
                                    },
                                ),
                                emissive_texture: material.emissive_texture.as_ref().map_or(
                                    None,
                                    |texture_handle| {
                                        textures
                                            .get(texture_handle)
                                            .map_or(None, |texture| texture.gpu_data.clone())
                                    },
                                ),
                                metallic_roughness_texture: material
                                    .metallic_roughness_texture
                                    .as_ref()
                                    .map_or(None, |texture_handle| {
                                        textures
                                            .get(texture_handle)
                                            .map_or(None, |texture| texture.gpu_data.clone())
                                    }),
                                occlusion_texture: material.occlusion_texture.as_ref().map_or(
                                    None,
                                    |texture_handle| {
                                        textures
                                            .get(texture_handle)
                                            .map_or(None, |texture| texture.gpu_data.clone())
                                    },
                                ),
                            },
                        });
                    }
                }
            }
        }
    }

    commands.insert_resource(ExtractedMeshes {
        meshes: extracted_meshes,
    });
}

#[derive(Default)]
pub struct MeshMeta {
    transform_uniforms: DynamicUniformVec<Mat4>,
}

pub fn prepare_meshes(
    render_resources: Res<RenderResources>,
    mut mesh_meta: ResMut<MeshMeta>,
    mut extracted_meshes: ResMut<ExtractedMeshes>,
) {
    mesh_meta
        .transform_uniforms
        .reserve_and_clear(extracted_meshes.meshes.len(), &render_resources);
    for extracted_mesh in extracted_meshes.meshes.iter_mut() {
        extracted_mesh.transform_binding_offset =
            mesh_meta.transform_uniforms.push(extracted_mesh.transform);
    }

    mesh_meta
        .transform_uniforms
        .write_to_staging_buffer(&render_resources);
}

// TODO: This is temporary. Once we expose BindGroupLayouts directly, we can create view bind groups without specific shader context
struct MeshViewBindGroups {
    view_bind_group: BindGroupId,
    mesh_transform_bind_group: BindGroupId,
}

#[derive(Default)]
pub struct MaterialMeta {
    material_bind_groups: Vec<BindGroupId>,
}

pub fn queue_meshes(
    mut commands: Commands,
    draw_functions: Res<DrawFunctions>,
    render_resources: Res<RenderResources>,
    pbr_shaders: Res<PbrShaders>,
    shadow_shaders: Res<ShadowShaders>,
    mesh_meta: Res<MeshMeta>,
    mut material_meta: ResMut<MaterialMeta>,
    light_meta: Res<LightMeta>,
    view_meta: Res<ViewMeta>,
    mut extracted_meshes: ResMut<ExtractedMeshes>,
    mut views: Query<(
        Entity,
        &ExtractedView,
        &ViewLights,
        &mut RenderPhase<Transparent3dPhase>,
    )>,
    mut view_light_shadow_phases: Query<&mut RenderPhase<ShadowPhase>>,
) {
    if extracted_meshes.meshes.is_empty() {
        return;
    }
    for (entity, view, view_lights, mut transparent_phase) in views.iter_mut() {
        let layout = &pbr_shaders.pipeline_descriptor.layout;
        let view_bind_group = BindGroupBuilder::default()
            .add_binding(0, view_meta.uniforms.binding())
            .add_binding(1, light_meta.view_gpu_lights.binding())
            .add_binding(2, view_lights.light_depth_texture_view)
            .add_binding(3, shadow_shaders.light_sampler)
            .finish();

        // TODO: this will only create the bind group if it isn't already created. this is a bit nasty
        render_resources.create_bind_group(layout.bind_group(0).id, &view_bind_group);

        let mesh_transform_bind_group = BindGroupBuilder::default()
            .add_binding(0, mesh_meta.transform_uniforms.binding())
            .finish();
        render_resources.create_bind_group(layout.bind_group(1).id, &mesh_transform_bind_group);

        commands.entity(entity).insert(MeshViewBindGroups {
            view_bind_group: view_bind_group.id,
            mesh_transform_bind_group: mesh_transform_bind_group.id,
        });

        // TODO: free old bind groups? clear_unused_bind_groups() currently does this for us? Moving to RAII would also do this for us?
        material_meta.material_bind_groups.clear();
        let mut material_bind_group_indices = HashMap::default();

        let draw_pbr = draw_functions.read().get_id::<DrawPbr>().unwrap();
        let view_matrix = view.transform.compute_matrix();
        let view_row_2 = view_matrix.row(2);
        for (i, mesh) in extracted_meshes.meshes.iter_mut().enumerate() {
            let material_bind_group_index = *material_bind_group_indices
                .entry(mesh.material_buffer)
                .or_insert_with(|| {
                    let index = material_meta.material_bind_groups.len();
                    let material_bind_group = {
                        let (base_color_texture_view, base_color_sampler) =
                            mesh.material_textures.base_color_texture.as_ref().map_or(
                                (
                                    pbr_shaders.dummy_white_texture_view,
                                    pbr_shaders.dummy_white_sampler,
                                ),
                                |gpu_data| (gpu_data.texture_view, gpu_data.sampler),
                            );

                        let (emissive_texture_view, emissive_sampler) =
                            mesh.material_textures.emissive_texture.as_ref().map_or(
                                (
                                    pbr_shaders.dummy_white_texture_view,
                                    pbr_shaders.dummy_white_sampler,
                                ),
                                |gpu_data| (gpu_data.texture_view, gpu_data.sampler),
                            );

                        let (metallic_roughness_texture_view, metallic_roughness_sampler) = mesh
                            .material_textures
                            .metallic_roughness_texture
                            .as_ref()
                            .map_or(
                                (
                                    pbr_shaders.dummy_white_texture_view,
                                    pbr_shaders.dummy_white_sampler,
                                ),
                                |gpu_data| (gpu_data.texture_view, gpu_data.sampler),
                            );
                        let (occlusion_texture_view, occlusion_sampler) =
                            mesh.material_textures.occlusion_texture.as_ref().map_or(
                                (
                                    pbr_shaders.dummy_white_texture_view,
                                    pbr_shaders.dummy_white_sampler,
                                ),
                                |gpu_data| (gpu_data.texture_view, gpu_data.sampler),
                            );

                        BindGroupBuilder::default()
                            .add_binding(
                                0,
                                RenderResourceBinding::Buffer {
                                    buffer: mesh.material_buffer,
                                    range: 0..StandardMaterialUniformData::std140_size_static()
                                        as u64,
                                },
                            )
                            .add_texture_view(1, base_color_texture_view)
                            .add_sampler(2, base_color_sampler)
                            .add_texture_view(3, emissive_texture_view)
                            .add_sampler(4, emissive_sampler)
                            .add_texture_view(5, metallic_roughness_texture_view)
                            .add_sampler(6, metallic_roughness_sampler)
                            .add_texture_view(7, occlusion_texture_view)
                            .add_sampler(8, occlusion_sampler)
                            .finish()
                    };
                    render_resources
                        .create_bind_group(layout.bind_group(2).id, &material_bind_group);
                    material_meta
                        .material_bind_groups
                        .push(material_bind_group.id);
                    index
                });

            // NOTE: row 2 of the view matrix dotted with column 3 of the model matrix
            //       gives the z component of translation of the mesh in view space
            let mesh_z = view_row_2.dot(mesh.transform.col(3));
            // FIXME: Switch from usize to u64 for portability and use sort key encoding
            //        similar to https://realtimecollisiondetection.net/blog/?p=86 as appropriate
            // FIXME: What is the best way to map from view space z to a number of bits of unsigned integer?
            let sort_key = (((mesh_z * 1000.0) as usize) << 10)
                | (material_bind_group_index & ((1 << 10) - 1));
            // TODO: currently there is only "transparent phase". this should pick transparent vs opaque according to the mesh material
            transparent_phase.add(Drawable {
                draw_function: draw_pbr,
                draw_key: i,
                sort_key,
            });
        }

        // ultimately lights should check meshes for relevancy (ex: light views can "see" different meshes than the main view can)
        let draw_shadow_mesh = draw_functions.read().get_id::<DrawShadowMesh>().unwrap();
        for view_light_entity in view_lights.lights.iter().copied() {
            let mut shadow_phase = view_light_shadow_phases.get_mut(view_light_entity).unwrap();
            let layout = &shadow_shaders.pipeline_descriptor.layout;
            let shadow_view_bind_group = BindGroupBuilder::default()
                .add_binding(0, view_meta.uniforms.binding())
                .finish();

            render_resources.create_bind_group(layout.bind_group(0).id, &shadow_view_bind_group);
            // TODO: this should only queue up meshes that are actually visible by each "light view"
            for i in 0..extracted_meshes.meshes.len() {
                shadow_phase.add(Drawable {
                    draw_function: draw_shadow_mesh,
                    draw_key: i,
                    sort_key: 0, // TODO: sort back-to-front
                })
            }

            commands
                .entity(view_light_entity)
                .insert(MeshViewBindGroups {
                    view_bind_group: shadow_view_bind_group.id,
                    mesh_transform_bind_group: mesh_transform_bind_group.id,
                });
        }
    }
}

// TODO: this logic can be moved to prepare_meshes once wgpu::Queue is exposed directly
pub struct PbrNode;

impl Node for PbrNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut dyn RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let mesh_meta = world.get_resource::<MeshMeta>().unwrap();
        let light_meta = world.get_resource::<LightMeta>().unwrap();
        mesh_meta
            .transform_uniforms
            .write_to_uniform_buffer(render_context);
        light_meta
            .view_gpu_lights
            .write_to_uniform_buffer(render_context);
        Ok(())
    }
}

type DrawPbrParams<'a> = (
    Res<'a, PbrShaders>,
    Res<'a, MaterialMeta>,
    Res<'a, ExtractedMeshes>,
    Query<'a, (&'a ViewUniform, &'a MeshViewBindGroups, &'a ViewLights)>,
);
pub struct DrawPbr {
    params: SystemState<DrawPbrParams<'static>>,
}

impl DrawPbr {
    pub fn new(world: &mut World) -> Self {
        Self {
            params: SystemState::new(world),
        }
    }
}

impl Draw for DrawPbr {
    fn draw(
        &mut self,
        world: &World,
        pass: &mut TrackedRenderPass,
        view: Entity,
        draw_key: usize,
        sort_key: usize,
    ) {
        let (pbr_shaders, material_meta, extracted_meshes, views) = self.params.get(world);
        let (view_uniforms, mesh_view_bind_groups, view_lights) = views.get(view).unwrap();
        let layout = &pbr_shaders.pipeline_descriptor.layout;
        let extracted_mesh = &extracted_meshes.meshes[draw_key];
        pass.set_pipeline(pbr_shaders.pipeline);
        pass.set_bind_group(
            0,
            layout.bind_group(0).id,
            mesh_view_bind_groups.view_bind_group,
            Some(&[
                view_uniforms.view_uniform_offset,
                view_lights.gpu_light_binding_index,
            ]),
        );
        pass.set_bind_group(
            1,
            layout.bind_group(1).id,
            mesh_view_bind_groups.mesh_transform_bind_group,
            Some(&[extracted_mesh.transform_binding_offset]),
        );
        pass.set_bind_group(
            2,
            layout.bind_group(2).id,
            material_meta.material_bind_groups[sort_key & ((1 << 10) - 1)],
            None,
        );
        pass.set_vertex_buffer(0, extracted_mesh.vertex_buffer, 0);
        if let Some(index_info) = &extracted_mesh.index_info {
            pass.set_index_buffer(index_info.buffer, 0, IndexFormat::Uint32);
            pass.draw_indexed(0..index_info.count, 0, 0..1);
        } else {
            panic!("non-indexed drawing not supported yet")
        }
    }
}