mod light;

use std::ops::Deref;

use bevy_render2::render_component::RenderComponent;
pub use light::*;

use crate::{StandardMaterial, StandardMaterialUniformData};
use bevy_asset::Handle;
use bevy_core_pipeline::Transparent3d;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{lifetimeless::*, SystemParamItem};
use bevy_math::Mat4;
use bevy_render2::render_phase::DrawCommand;
use bevy_render2::{
    mesh::Mesh,
    render_asset::RenderAssets,
    render_component::{ComponentUniforms, DynamicUniformIndex},
    render_phase::{DrawFunctions, RenderPhase, TrackedRenderPass},
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    shader::Shader,
    texture::{BevyDefault, GpuImage, Image, TextureFormatPixelInfo},
    view::{ExtractedView, ViewMeta, ViewUniformOffset},
};
use bevy_transform::components::GlobalTransform;
use bevy_utils::slab::{FrameSlabMap, FrameSlabMapKey};
use crevice::std140::AsStd140;
use wgpu::{
    Extent3d, ImageCopyTexture, ImageDataLayout, Origin3d, TextureDimension, TextureFormat,
    TextureViewDescriptor,
};

#[derive(AsStd140, Clone)]
pub struct MeshTransform {
    transform: Mat4,
}

impl Deref for MeshTransform {
    type Target = Mat4;

    fn deref(&self) -> &Self::Target {
        &self.transform
    }
}

impl RenderComponent for MeshTransform {
    type SourceComponent = GlobalTransform;

    #[inline]
    fn extract_component(source: &Self::SourceComponent) -> Self {
        MeshTransform {
            transform: source.compute_matrix(),
        }
    }
}

pub struct PbrPipeline {
    pub pipeline: RenderPipeline,
    pub shader_module: ShaderModule,
    pub view_layout: BindGroupLayout,
    pub material_layout: BindGroupLayout,
    pub mesh_layout: BindGroupLayout,
    // This dummy white texture is to be used in place of optional StandardMaterial textures
    dummy_white_gpu_image: GpuImage,
}

// TODO: this pattern for initializing the shaders / pipeline isn't ideal. this should be handled by the asset system
impl FromWorld for PbrPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.get_resource::<RenderDevice>().unwrap();
        let shader = Shader::from_wgsl(include_str!("pbr.wgsl"))
            .process(&[])
            .unwrap();
        let shader_module = render_device.create_shader_module(&shader);

        // TODO: move this into ViewMeta?
        let view_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[
                // View
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStage::VERTEX | ShaderStage::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        // TODO: change this to ViewUniform::std140_size_static once crevice fixes this!
                        // Context: https://github.com/LPGhatguy/crevice/issues/29
                        min_binding_size: BufferSize::new(80),
                    },
                    count: None,
                },
                // Lights
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        // TODO: change this to GpuLights::std140_size_static once crevice fixes this!
                        // Context: https://github.com/LPGhatguy/crevice/issues/29
                        min_binding_size: BufferSize::new(1024),
                    },
                    count: None,
                },
                // Point Shadow Texture Cube Array
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Depth,
                        view_dimension: TextureViewDimension::CubeArray,
                    },
                    count: None,
                },
                // Point Shadow Texture Array Sampler
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler {
                        comparison: true,
                        filtering: true,
                    },
                    count: None,
                },
                // Directional Shadow Texture Array
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Depth,
                        view_dimension: TextureViewDimension::D2Array,
                    },
                    count: None,
                },
                // Directional Shadow Texture Array Sampler
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler {
                        comparison: true,
                        filtering: true,
                    },
                    count: None,
                },
            ],
            label: None,
        });

        let mesh_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStage::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: BufferSize::new(Mat4::std140_size_static() as u64),
                },
                count: None,
            }],
            label: None,
        });

        let material_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(
                            StandardMaterialUniformData::std140_size_static() as u64,
                        ),
                    },
                    count: None,
                },
                // Base Color Texture
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Base Color Texture Sampler
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler {
                        comparison: false,
                        filtering: true,
                    },
                    count: None,
                },
                // Emissive Texture
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Emissive Texture Sampler
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler {
                        comparison: false,
                        filtering: true,
                    },
                    count: None,
                },
                // Metallic Roughness Texture
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Metallic Roughness Texture Sampler
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler {
                        comparison: false,
                        filtering: true,
                    },
                    count: None,
                },
                // Occlusion Texture
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Occlusion Texture Sampler
                BindGroupLayoutEntry {
                    binding: 8,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler {
                        comparison: false,
                        filtering: true,
                    },
                    count: None,
                },
            ],
            label: None,
        });

        let pipeline_layout = render_device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            push_constant_ranges: &[],
            bind_group_layouts: &[&view_layout, &mesh_layout, &material_layout],
        });

        let pipeline = render_device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            vertex: VertexState {
                buffers: &[VertexBufferLayout {
                    array_stride: 32,
                    step_mode: InputStepMode::Vertex,
                    attributes: &[
                        // Position (GOTCHA! Vertex_Position isn't first in the buffer due to how Mesh sorts attributes (alphabetically))
                        VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 12,
                            shader_location: 0,
                        },
                        // Normal
                        VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 1,
                        },
                        // Uv
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 24,
                            shader_location: 2,
                        },
                    ],
                }],
                module: &shader_module,
                entry_point: "vertex",
            },
            fragment: Some(FragmentState {
                module: &shader_module,
                entry_point: "fragment",
                targets: &[ColorTargetState {
                    format: TextureFormat::bevy_default(),
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
            }),
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
            layout: Some(&pipeline_layout),
            multisample: MultisampleState::default(),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                polygon_mode: PolygonMode::Fill,
                clamp_depth: false,
                conservative: false,
            },
        });

        // A 1x1x1 'all 1.0' texture to use as a dummy texture to use in place of optional StandardMaterial textures
        let dummy_white_gpu_image = {
            let image = Image::new_fill(
                Extent3d::default(),
                TextureDimension::D2,
                &[255u8; 4],
                TextureFormat::bevy_default(),
            );
            let texture = render_device.create_texture(&image.texture_descriptor);
            let sampler = render_device.create_sampler(&image.sampler_descriptor);

            let format_size = image.texture_descriptor.format.pixel_size();
            let render_queue = world.get_resource_mut::<RenderQueue>().unwrap();
            render_queue.write_texture(
                ImageCopyTexture {
                    texture: &texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                },
                &image.data,
                ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(
                        std::num::NonZeroU32::new(
                            image.texture_descriptor.size.width * format_size as u32,
                        )
                        .unwrap(),
                    ),
                    rows_per_image: None,
                },
                image.texture_descriptor.size,
            );

            let texture_view = texture.create_view(&TextureViewDescriptor::default());
            GpuImage {
                texture,
                texture_view,
                sampler,
            }
        };
        PbrPipeline {
            pipeline,
            shader_module,
            view_layout,
            material_layout,
            mesh_layout,
            dummy_white_gpu_image,
        }
    }
}

pub struct StandardMaterialBindGroup {
    // TODO: compare cost of doing this vs cloning the BindGroup?
    key: FrameSlabMapKey<BufferId, BindGroup>,
}

#[derive(Default)]
pub struct MeshMeta {
    pub material_bind_groups: FrameSlabMap<BufferId, BindGroup>,
    pub mesh_transform_bind_group: FrameSlabMap<BufferId, BindGroup>,
    pub mesh_transform_bind_group_key: Option<FrameSlabMapKey<BufferId, BindGroup>>,
}

pub struct PbrViewBindGroup {
    pub view: BindGroup,
}

fn image_handle_to_view_sampler<'a>(
    pbr_pipeline: &'a PbrPipeline,
    gpu_images: &'a RenderAssets<Image>,
    image_option: &Option<Handle<Image>>,
) -> (&'a TextureView, &'a Sampler) {
    image_option.as_ref().map_or(
        (
            &pbr_pipeline.dummy_white_gpu_image.texture_view,
            &pbr_pipeline.dummy_white_gpu_image.sampler,
        ),
        |image_handle| {
            let gpu_image = gpu_images
                .get(image_handle)
                .expect("only materials with valid textures should be drawn");
            (&gpu_image.texture_view, &gpu_image.sampler)
        },
    )
}

pub fn queue_transform_bind_group(
    pbr_pipeline: Res<PbrPipeline>,
    render_device: Res<RenderDevice>,
    mut mesh_meta: ResMut<MeshMeta>,
    transform_uniforms: Res<ComponentUniforms<MeshTransform>>,
) {
    if let Some(buffer) = transform_uniforms.uniforms().uniform_buffer() {
        mesh_meta.mesh_transform_bind_group.next_frame();
        mesh_meta.mesh_transform_bind_group_key =
            Some(mesh_meta.mesh_transform_bind_group.get_or_insert_with(
                buffer.id(),
                || {
                    render_device.create_bind_group(&BindGroupDescriptor {
                        entries: &[BindGroupEntry {
                            binding: 0,
                            resource: transform_uniforms.uniforms().binding(),
                        }],
                        label: None,
                        // TODO: store this layout elsewhere
                        layout: &pbr_pipeline.mesh_layout,
                    })
                },
            ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn queue_meshes(
    mut commands: Commands,
    transparent_3d_draw_functions: Res<DrawFunctions<Transparent3d>>,
    render_device: Res<RenderDevice>,
    pbr_pipeline: Res<PbrPipeline>,
    shadow_pipeline: Res<ShadowPipeline>,
    mesh_meta: ResMut<MeshMeta>,
    light_meta: Res<LightMeta>,
    view_meta: Res<ViewMeta>,
    gpu_images: Res<RenderAssets<Image>>,
    render_materials: Res<RenderAssets<StandardMaterial>>,
    standard_material_meshes: Query<
        (Entity, &Handle<StandardMaterial>, &MeshTransform),
        With<Handle<Mesh>>,
    >,
    mut views: Query<(
        Entity,
        &ExtractedView,
        &ViewLights,
        &mut RenderPhase<Transparent3d>,
    )>,
) {
    if view_meta.uniforms.is_empty() {
        return;
    }

    let mesh_meta = mesh_meta.into_inner();
    for (entity, view, view_lights, mut transparent_phase) in views.iter_mut() {
        // TODO: cache this?
        let view_bind_group = render_device.create_bind_group(&BindGroupDescriptor {
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: view_meta.uniforms.binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: light_meta.view_gpu_lights.binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(
                        &view_lights.point_light_depth_texture_view,
                    ),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&shadow_pipeline.point_light_sampler),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(
                        &view_lights.directional_light_depth_texture_view,
                    ),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::Sampler(&shadow_pipeline.directional_light_sampler),
                },
            ],
            label: None,
            layout: &pbr_pipeline.view_layout,
        });

        commands.entity(entity).insert(PbrViewBindGroup {
            view: view_bind_group,
        });

        let draw_pbr = transparent_3d_draw_functions
            .read()
            .get_id::<DrawPbr>()
            .unwrap();
        mesh_meta.material_bind_groups.next_frame();

        let view_matrix = view.transform.compute_matrix();
        let view_row_2 = view_matrix.row(2);

        if standard_material_meshes.is_empty() {
            return;
        }
        for (entity, material_handle, transform) in standard_material_meshes.iter() {
            let gpu_material = &render_materials
                .get(material_handle)
                .expect("Failed to get StandardMaterial PreparedAsset");
            let material_bind_group_key =
                mesh_meta
                    .material_bind_groups
                    .get_or_insert_with(gpu_material.buffer.id(), || {
                        let (base_color_texture_view, base_color_sampler) =
                            image_handle_to_view_sampler(
                                &pbr_pipeline,
                                &gpu_images,
                                &gpu_material.base_color_texture,
                            );

                        let (emissive_texture_view, emissive_sampler) =
                            image_handle_to_view_sampler(
                                &pbr_pipeline,
                                &gpu_images,
                                &gpu_material.emissive_texture,
                            );

                        let (metallic_roughness_texture_view, metallic_roughness_sampler) =
                            image_handle_to_view_sampler(
                                &pbr_pipeline,
                                &gpu_images,
                                &gpu_material.metallic_roughness_texture,
                            );
                        let (occlusion_texture_view, occlusion_sampler) =
                            image_handle_to_view_sampler(
                                &pbr_pipeline,
                                &gpu_images,
                                &gpu_material.occlusion_texture,
                            );
                        render_device.create_bind_group(&BindGroupDescriptor {
                            entries: &[
                                BindGroupEntry {
                                    binding: 0,
                                    resource: gpu_material.buffer.as_entire_binding(),
                                },
                                BindGroupEntry {
                                    binding: 1,
                                    resource: BindingResource::TextureView(base_color_texture_view),
                                },
                                BindGroupEntry {
                                    binding: 2,
                                    resource: BindingResource::Sampler(base_color_sampler),
                                },
                                BindGroupEntry {
                                    binding: 3,
                                    resource: BindingResource::TextureView(emissive_texture_view),
                                },
                                BindGroupEntry {
                                    binding: 4,
                                    resource: BindingResource::Sampler(emissive_sampler),
                                },
                                BindGroupEntry {
                                    binding: 5,
                                    resource: BindingResource::TextureView(
                                        metallic_roughness_texture_view,
                                    ),
                                },
                                BindGroupEntry {
                                    binding: 6,
                                    resource: BindingResource::Sampler(metallic_roughness_sampler),
                                },
                                BindGroupEntry {
                                    binding: 7,
                                    resource: BindingResource::TextureView(occlusion_texture_view),
                                },
                                BindGroupEntry {
                                    binding: 8,
                                    resource: BindingResource::Sampler(occlusion_sampler),
                                },
                            ],
                            label: None,
                            layout: &pbr_pipeline.material_layout,
                        })
                    });

            commands.entity(entity).insert(StandardMaterialBindGroup {
                key: material_bind_group_key,
            });

            // NOTE: row 2 of the view matrix dotted with column 3 of the model matrix
            //       gives the z component of translation of the mesh in view space
            let mesh_z = view_row_2.dot(transform.col(3));
            // TODO: currently there is only "transparent phase". this should pick transparent vs opaque according to the mesh material
            transparent_phase.add(Transparent3d {
                entity,
                draw_function: draw_pbr,
                distance: mesh_z,
            });
        }
    }
}

pub type DrawPbr = (
    SetPbrPipeline,
    SetMeshViewBindGroup,
    SetTransformBindGroup,
    SetStandardMaterialBindGroup,
    DrawMesh,
);

pub struct SetPbrPipeline;
impl DrawCommand<Transparent3d> for SetPbrPipeline {
    type Param = SRes<PbrPipeline>;
    #[inline]
    fn draw<'w>(
        _view: Entity,
        _item: &Transparent3d,
        pbr_pipeline: SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) {
        pass.set_render_pipeline(&pbr_pipeline.into_inner().pipeline);
    }
}

pub struct SetMeshViewBindGroup;
impl DrawCommand<Transparent3d> for SetMeshViewBindGroup {
    type Param = SQuery<(
        Read<ViewUniformOffset>,
        Read<ViewLights>,
        Read<PbrViewBindGroup>,
    )>;
    #[inline]
    fn draw<'w>(
        view: Entity,
        _item: &Transparent3d,
        view_query: SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) {
        let (view_uniform, view_lights, pbr_view_bind_group) = view_query.get(view).unwrap();
        pass.set_bind_group(
            0,
            &pbr_view_bind_group.view,
            &[view_uniform.offset, view_lights.gpu_light_binding_index],
        );
    }
}

pub struct SetTransformBindGroup;
impl DrawCommand<Transparent3d> for SetTransformBindGroup {
    type Param = (
        SRes<MeshMeta>,
        SQuery<Read<DynamicUniformIndex<MeshTransform>>>,
    );
    #[inline]
    fn draw<'w>(
        _view: Entity,
        item: &Transparent3d,
        (mesh_meta, mesh_query): SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) {
        let transform_index = mesh_query.get(item.entity).unwrap();
        let mesh_meta = mesh_meta.into_inner();
        pass.set_bind_group(
            1,
            mesh_meta
                .mesh_transform_bind_group
                .get_value(mesh_meta.mesh_transform_bind_group_key.unwrap())
                .unwrap(),
            &[transform_index.index()],
        );
    }
}

pub struct SetStandardMaterialBindGroup;
impl DrawCommand<Transparent3d> for SetStandardMaterialBindGroup {
    type Param = (SRes<MeshMeta>, SQuery<Read<StandardMaterialBindGroup>>);
    #[inline]
    fn draw<'w>(
        _view: Entity,
        item: &Transparent3d,
        (mesh_meta, mesh_query): SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) {
        let material = mesh_query.get(item.entity).unwrap();
        let mesh_meta = mesh_meta.into_inner();

        pass.set_bind_group(2, &mesh_meta.material_bind_groups[material.key], &[]);
    }
}

pub struct DrawMesh;
impl DrawCommand<Transparent3d> for DrawMesh {
    type Param = (SRes<RenderAssets<Mesh>>, SQuery<Read<Handle<Mesh>>>);
    #[inline]
    fn draw<'w>(
        _view: Entity,
        item: &Transparent3d,
        (meshes, mesh_query): SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) {
        let mesh_handle = mesh_query.get(item.entity).unwrap();
        let gpu_mesh = meshes.into_inner().get(mesh_handle).unwrap();
        pass.set_vertex_buffer(0, gpu_mesh.vertex_buffer.slice(..));
        if let Some(index_info) = &gpu_mesh.index_info {
            pass.set_index_buffer(index_info.buffer.slice(..), 0, IndexFormat::Uint32);
            pass.draw_indexed(0..index_info.count, 0, 0..1);
        } else {
            panic!("non-indexed drawing not supported yet")
        }
    }
}
