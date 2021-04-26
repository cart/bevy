use crate::Sprite;
use bevy_app::{App, Plugin};
use bevy_core::{AsBytes, Byteable};
use bevy_ecs::prelude::*;
use bevy_math::{Mat4, Vec2};
use bevy_render::{
    camera::{ActiveCameras, Camera},
    color::Color,
    mesh::{shape::Quad, Indices, Mesh},
    pass::{
        LoadOp, Operations, PassDescriptor, RenderPass, RenderPassColorAttachmentDescriptor,
        RenderPassDepthStencilAttachmentDescriptor, TextureAttachment,
    },
    pipeline::{BindGroupDescriptorId, PipelineDescriptorV2, PipelineId},
    pipeline::{
        BindType, BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrite,
        CompareFunction, CullMode, DepthBiasState, DepthStencilState, FrontFace, IndexFormat,
        InputStepMode, PipelineDescriptor, PipelineLayout, PolygonMode, PrimitiveState,
        PrimitiveTopology, StencilFaceState, StencilState, VertexAttribute, VertexBufferLayout,
        VertexFormat,
    },
    renderer::{
        BindGroup, BindGroupBuilder, BindGroupId, BufferId, BufferInfo, BufferMapMode, BufferUsage,
        RenderContext, RenderResourceContext, RenderResourceType,
    },
    shader::{Shader, ShaderId, ShaderStage, ShaderStagesV2},
    texture::TextureFormat,
    v2::{
        render_graph::{Node, RenderGraph, ResourceSlotInfo, ResourceSlots, WindowSwapChainNode},
        RenderStage,
    },
};
use bevy_transform::components::GlobalTransform;
use bevy_window::WindowId;
use std::borrow::Cow;

#[derive(Default)]
pub struct PipelinedSpritePlugin;

impl Plugin for PipelinedSpritePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Sprite>();
        app.sub_app_mut(0)
            .add_system_to_stage(RenderStage::Extract, extract_cameras.system())
            .add_system_to_stage(RenderStage::Extract, extract_sprites.system())
            .add_system_to_stage(RenderStage::Prepare, prepare_cameras.system())
            .add_system_to_stage(RenderStage::Prepare, prepare_sprites.system())
            .init_resource::<SpriteShaders>()
            .init_resource::<SpriteBuffers>()
            .init_resource::<QuadMesh>();
        let render_world = app.sub_app_mut(0).world.cell();
        let mut graph = render_world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_node("camera", CameraNode);
        graph.add_node("sprite", SpriteNode);
        graph.add_node_edge("camera", "sprite").unwrap();
        graph.add_node(
            "primary_swap_chain",
            WindowSwapChainNode::new(WindowId::primary()),
        );
        graph
            .add_slot_edge(
                "primary_swap_chain",
                WindowSwapChainNode::OUT_TEXTURE,
                "sprite",
                SpriteNode::IN_COLOR_ATTACHMENT,
            )
            .unwrap();
    }
}

pub struct QuadMesh {
    vertex_buffer: BufferId,
    index_buffer: BufferId,
    mesh: Mesh,
}

impl FromWorld for QuadMesh {
    fn from_world(world: &mut World) -> Self {
        let mut mesh = Mesh::from(Quad::new(Vec2::new(1.0, 1.0)));
        // TODO: support arbitrary attributes
        mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL).unwrap();
        mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0).unwrap();
        let render_resource_context = world
            .get_resource::<Box<dyn RenderResourceContext>>()
            .unwrap();
        let vertex_bytes = mesh.get_vertex_buffer_data();
        let vertex_buffer = render_resource_context.create_buffer_with_data(
            BufferInfo {
                buffer_usage: BufferUsage::VERTEX,
                ..Default::default()
            },
            &vertex_bytes,
        );

        let index_bytes = mesh.get_index_buffer_bytes().unwrap();
        let index_buffer = render_resource_context.create_buffer_with_data(
            BufferInfo {
                buffer_usage: BufferUsage::INDEX,
                ..Default::default()
            },
            &index_bytes,
        );

        QuadMesh {
            vertex_buffer,
            index_buffer,
            mesh,
        }
    }
}

pub struct SpriteShaders {
    vertex: ShaderId,
    fragment: ShaderId,
    pipeline: PipelineId,
    pipeline_descriptor: PipelineDescriptorV2,
}

impl FromWorld for SpriteShaders {
    fn from_world(world: &mut World) -> Self {
        let render_resource_context = world
            .get_resource::<Box<dyn RenderResourceContext>>()
            .unwrap();
        let vertex_shader = Shader::from_glsl(ShaderStage::Vertex, include_str!("sprite.vert"))
            .get_spirv_shader(None)
            .unwrap();
        let fragment_shader = Shader::from_glsl(ShaderStage::Fragment, include_str!("sprite.frag"))
            .get_spirv_shader(None)
            .unwrap();

        let vertex_layout = vertex_shader.reflect_layout(true).unwrap();
        let fragment_layout = fragment_shader.reflect_layout(true).unwrap();

        let mut pipeline_layout =
            PipelineLayout::from_shader_layouts(&mut [vertex_layout, fragment_layout]);
        if let BindType::Uniform {
            ref mut has_dynamic_offset,
            ..
        } = pipeline_layout.bind_groups[1].bindings[0].bind_type
        {
            *has_dynamic_offset = true;
        }

        let vertex = render_resource_context.create_shader_module_v2(&vertex_shader);
        let fragment = render_resource_context.create_shader_module_v2(&fragment_shader);

        pipeline_layout.vertex_buffer_descriptors = vec![VertexBufferLayout {
            stride: 12,
            name: "Vertex_Position".into(),
            step_mode: InputStepMode::Vertex,
            attributes: vec![
                VertexAttribute {
                    name: "Vertex_Position".into(),
                    format: VertexFormat::Float3,
                    offset: 0,
                    shader_location: 0,
                },
                // VertexAttribute {
                //     name: "Vertex_Position".into(),
                //     format: VertexFormat::Float3,
                //     offset: 12,
                //     shader_location: 1,
                // },
                // VertexAttribute {
                //     name: "Vertex_Position".into(),
                //     format: VertexFormat::Float2,
                //     offset: 24,
                //     shader_location: 2,
                // }
            ],
        }];

        let pipeline_descriptor = PipelineDescriptorV2 {
            depth_stencil: None,
            color_target_states: vec![ColorTargetState {
                format: TextureFormat::default(),
                color_blend: BlendState {
                    src_factor: BlendFactor::SrcAlpha,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha_blend: BlendState {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                write_mask: ColorWrite::ALL,
            }],
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: CullMode::None,
                polygon_mode: PolygonMode::Fill,
            },
            ..PipelineDescriptorV2::new(
                ShaderStagesV2 {
                    vertex,
                    fragment: Some(fragment),
                },
                pipeline_layout,
            )
        };

        let pipeline = render_resource_context.create_render_pipeline_v2(&pipeline_descriptor);

        SpriteShaders {
            vertex,
            fragment,
            pipeline,
            pipeline_descriptor,
        }
    }
}

// TODO: this should live in bevy_render
#[derive(Debug)]
pub struct ExtractedCamera {
    projection: Mat4,
    transform: Mat4,
}

// TODO: somehow make FromWorld impl work so these don't need to be options?
// This could also use the Option<ResMut<CameraBuffers>>> pattern + commands.insert_resource to avoid options
#[derive(Debug)]
pub struct CameraBuffers {
    view_proj: BufferId,
    staging: BufferId,
    bind_group: BindGroupId,
}

fn extract_cameras(
    mut commands: Commands,
    active_cameras: Res<ActiveCameras>,
    query: Query<(&Camera, &GlobalTransform)>,
) {
    // TODO: move camera name?
    if let Some(active_camera) =
        active_cameras.get(bevy_render::render_graph::base::camera::CAMERA_2D)
    {
        if let Some((camera, transform)) = active_camera.entity.and_then(|e| query.get(e).ok()) {
            commands.insert_resource(ExtractedCamera {
                projection: camera.projection_matrix,
                transform: transform.compute_matrix(),
            })
        }
    }
}

const MATRIX_SIZE: usize = std::mem::size_of::<[[f32; 4]; 4]>();
fn prepare_cameras(
    mut commands: Commands,
    render_resource_context: Res<Box<dyn RenderResourceContext>>,
    camera_buffers: Option<ResMut<CameraBuffers>>,
    // TODO: remove this. cameras shouldn't be coupled to sprites
    sprite_shaders: Res<SpriteShaders>,
    extracted_camera: Res<ExtractedCamera>,
) {
    let staging = if let Some(camera_buffers) = camera_buffers {
        render_resource_context.map_buffer(camera_buffers.staging, BufferMapMode::Write);
        camera_buffers.staging
    } else {
        let staging = render_resource_context.create_buffer(BufferInfo {
            size: MATRIX_SIZE,
            buffer_usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
            mapped_at_creation: true,
        });
        let view_proj = render_resource_context.create_buffer(BufferInfo {
            size: MATRIX_SIZE,
            buffer_usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
            mapped_at_creation: false,
        });

        let bind_group = BindGroupBuilder::default()
            .add_binding(
                0,
                bevy_render::renderer::RenderResourceBinding::Buffer {
                    buffer: view_proj,
                    range: 0..MATRIX_SIZE as u64,
                    dynamic_index: None,
                },
            )
            .finish();

        render_resource_context.create_bind_group(
            sprite_shaders.pipeline_descriptor.layout.bind_groups[0].id,
            &bind_group,
        );

        commands.insert_resource(CameraBuffers {
            staging,
            view_proj,
            bind_group: bind_group.id,
        });
        staging
    };
    let view_proj = extracted_camera.projection * extracted_camera.transform.inverse();
    render_resource_context.write_mapped_buffer(
        staging,
        0..MATRIX_SIZE as u64,
        &mut |data, _renderer| {
            data[0..MATRIX_SIZE].copy_from_slice(view_proj.to_cols_array_2d().as_bytes());
        },
    );
    render_resource_context.unmap_buffer(staging);
}

pub struct CameraNode;

impl Node for CameraNode {
    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn RenderContext,
        _input: &ResourceSlots,
        _output: &mut ResourceSlots,
    ) {
        let camera_buffers = world.get_resource::<CameraBuffers>().unwrap();
        render_context.copy_buffer_to_buffer(
            camera_buffers.staging,
            0,
            camera_buffers.view_proj,
            0,
            MATRIX_SIZE as u64,
        );
    }
}

struct ExtractedSprite {
    transform: Mat4,
    size: Vec2,
}

struct ExtractedSprites {
    sprites: Vec<ExtractedSprite>,
}

fn extract_sprites(mut commands: Commands, query: Query<(&Sprite, &GlobalTransform)>) {
    let mut extracted_sprites = Vec::new();
    for (sprite, transform) in query.iter() {
        extracted_sprites.push(ExtractedSprite {
            transform: transform.compute_matrix(),
            size: sprite.size,
        })
    }

    commands.insert_resource(ExtractedSprites {
        sprites: extracted_sprites,
    })
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SpriteUniform {
    pub transform: [[f32; 4]; 4],
    pub size: [f32; 2],
    pub padding: [[f32; 10]; 4],
    pub padding_0: [f32; 6],
}

unsafe impl Byteable for SpriteUniform {}

#[derive(Default)]
struct SpriteBuffers {
    sprites: Option<BufferId>,
    staging: Option<BufferId>,
    sprite_uniforms: Vec<SpriteUniform>,
    sprite_bind_group: Option<BindGroupId>,
}

fn prepare_sprites(
    render_resource_context: Res<Box<dyn RenderResourceContext>>,
    mut sprite_buffers: ResMut<SpriteBuffers>,
    sprite_shaders: Res<SpriteShaders>,
    extracted_sprites: Res<ExtractedSprites>,
) {
    let sprite_uniform_size = std::mem::size_of::<SpriteUniform>();
    let sprite_uniform_array_size = sprite_uniform_size * extracted_sprites.sprites.len();

    if extracted_sprites.sprites.len() != sprite_buffers.sprite_uniforms.len() {
        if let Some(staging) = sprite_buffers.staging.take() {
            render_resource_context.remove_buffer(staging);
        }

        if let Some(sprites) = sprite_buffers.sprites.take() {
            render_resource_context.remove_buffer(sprites);
        }
    }

    // dont create buffers when there are no sprites
    if extracted_sprites.sprites.len() == 0 {
        return;
    }

    let staging_buffer = if let Some(staging_buffer) = sprite_buffers.staging {
        render_resource_context.map_buffer(staging_buffer, BufferMapMode::Write);
        staging_buffer
    } else {
        let staging_buffer = render_resource_context.create_buffer(BufferInfo {
            size: sprite_uniform_array_size,
            buffer_usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
            mapped_at_creation: true,
        });
        sprite_buffers.staging = Some(staging_buffer);

        let buffer = render_resource_context.create_buffer(BufferInfo {
            size: sprite_uniform_array_size,
            buffer_usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
            mapped_at_creation: false,
        });
        sprite_buffers.sprites = Some(buffer);

        let bind_group = BindGroupBuilder::default()
            .add_binding(
                0,
                bevy_render::renderer::RenderResourceBinding::Buffer {
                    buffer,
                    // TODO: make this less magic (derive from actual size of sprite)
                    range: 0..72 as u64,
                    dynamic_index: None,
                },
            )
            .finish();

        render_resource_context.create_bind_group(
            sprite_shaders.pipeline_descriptor.layout.bind_groups[1].id,
            &bind_group,
        );
        sprite_buffers.sprite_bind_group = Some(bind_group.id);

        staging_buffer
    };

    sprite_buffers.sprite_uniforms.clear();
    sprite_buffers
        .sprite_uniforms
        .reserve(extracted_sprites.sprites.len());
    for extracted_sprite in extracted_sprites.sprites.iter() {
        sprite_buffers.sprite_uniforms.push(SpriteUniform {
            transform: extracted_sprite.transform.to_cols_array_2d(),
            size: extracted_sprite.size.into(),
            padding: Default::default(),
            padding_0: Default::default(),
        });
    }
    render_resource_context.write_mapped_buffer(
        staging_buffer,
        0..sprite_uniform_array_size as u64,
        &mut |data, _context| {
            data[0..sprite_uniform_array_size]
                .copy_from_slice(sprite_buffers.sprite_uniforms.as_bytes());
        },
    );
    render_resource_context.unmap_buffer(staging_buffer);
}

pub struct SpriteNode;

impl SpriteNode {
    pub const IN_COLOR_ATTACHMENT: &'static str = "color_attachment";
}

impl Node for SpriteNode {
    fn input(&self) -> &[ResourceSlotInfo] {
        static INPUT: &[ResourceSlotInfo] = &[ResourceSlotInfo {
            name: Cow::Borrowed(SpriteNode::IN_COLOR_ATTACHMENT),
            resource_type: RenderResourceType::Texture,
        }];
        INPUT
    }
    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn RenderContext,
        input: &ResourceSlots,
        output: &mut ResourceSlots,
    ) {
        // TODO: consider adding shorthand like `get_texture(0)`
        let color_attachment_texture = input.get(0).unwrap().get_texture().unwrap();
        let pass_descriptor = PassDescriptor {
            color_attachments: vec![RenderPassColorAttachmentDescriptor {
                attachment: TextureAttachment::Id(color_attachment_texture),
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::rgb(1.0, 0.1, 0.1)),
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
            // depth_stencil_attachment: Some(RenderPassDepthStencilAttachmentDescriptor {
            //     attachment: TextureAttachment::Input("depth".to_string()),
            //     depth_ops: Some(Operations {
            //         load: LoadOp::Clear(1.0),
            //         store: true,
            //     }),
            //     stencil_ops: None,
            // }),
            sample_count: 1,
        };

        let sprite_shaders = world.get_resource::<SpriteShaders>().unwrap();
        let camera_buffers = world.get_resource::<CameraBuffers>().unwrap();
        let sprite_buffers = world.get_resource::<SpriteBuffers>().unwrap();
        let quad_mesh = world.get_resource::<QuadMesh>().unwrap();
        let layout = &sprite_shaders.pipeline_descriptor.layout;

        let index_range = match quad_mesh.mesh.indices() {
            Some(Indices::U32(indices)) => 0..indices.len() as u32,
            Some(Indices::U16(indices)) => 0..indices.len() as u32,
            None => panic!(),
        };

        let sprite_uniform_size = std::mem::size_of::<SpriteUniform>();
        let sprite_uniform_array_size = sprite_uniform_size * sprite_buffers.sprite_uniforms.len();
        if sprite_buffers.sprite_uniforms.len() != 0 {
            render_context.copy_buffer_to_buffer(
                sprite_buffers.staging.unwrap(),
                0,
                sprite_buffers.sprites.unwrap(),
                0,
                sprite_uniform_array_size as u64,
            );
        }

        render_context.begin_pass(&pass_descriptor, &mut |render_pass: &mut dyn RenderPass| {
            if sprite_buffers.sprite_uniforms.len() == 0 {
                return;
            }
            render_pass.set_pipeline_v2(sprite_shaders.pipeline);
            render_pass.set_vertex_buffer(0, quad_mesh.vertex_buffer, 0);
            render_pass.set_index_buffer(quad_mesh.index_buffer, 0, IndexFormat::Uint32);
            render_pass.set_bind_group(
                0,
                layout.bind_groups[0].id,
                camera_buffers.bind_group,
                None,
            );

            for i in 0..sprite_buffers.sprite_uniforms.len() {
                render_pass.set_bind_group(
                    1,
                    layout.bind_groups[1].id,
                    sprite_buffers.sprite_bind_group.unwrap(),
                    Some(&[(i * sprite_uniform_size) as u32]),
                );
                render_pass.draw_indexed(index_range.clone(), 0, 0..1);
            }
        })
    }
}
