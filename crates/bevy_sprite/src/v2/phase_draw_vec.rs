use crate::{
    v2::camera_plugin::{CameraBuffers, PipelinedCameraPlugin},
    Sprite,
};
use bevy_app::{App, Plugin};
use bevy_core::{AsBytes, Byteable};
use bevy_ecs::prelude::*;
use bevy_math::{Mat4, Vec2, Vec3, Vec4Swizzles};
use bevy_render::{
    color::Color,
    mesh::{shape::Quad, Indices, Mesh, VertexAttributeValues},
    pass::{
        LoadOp, Operations, PassDescriptor, RenderPass, RenderPassColorAttachmentDescriptor,
        TextureAttachment,
    },
    pipeline::{
        BindType, BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrite,
        CompareFunction, CullMode, DepthBiasState, DepthStencilState, FrontFace, IndexFormat,
        InputStepMode, PipelineDescriptor, PipelineLayout, PolygonMode, PrimitiveState,
        PrimitiveTopology, StencilFaceState, StencilState, VertexAttribute, VertexBufferLayout,
        VertexFormat,
    },
    pipeline::{PipelineDescriptorV2, PipelineId},
    renderer::{
        BindGroup, BindGroupBuilder, BindGroupId, BufferId, BufferInfo, BufferMapMode, BufferUsage,
        RenderContext, RenderResourceContext, RenderResourceType,
    },
    shader::{Shader, ShaderId, ShaderStage, ShaderStagesV2},
    texture::TextureFormat,
    v2::{
        draw_state::{Draw, DrawState},
        render_graph::{Node, RenderGraph, ResourceSlotInfo, ResourceSlots, WindowSwapChainNode},
        RenderStage,
    },
};
use bevy_transform::components::GlobalTransform;
use bevy_window::WindowId;
use std::borrow::Cow;

#[derive(Default)]
pub struct PhaseSpritePlugin;

impl Plugin for PhaseSpritePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Sprite>();
        app.add_plugin(PipelinedCameraPlugin);
        app.sub_app_mut(0)
            .add_system_to_stage(RenderStage::Extract, extract_sprites.system())
            .add_system_to_stage(RenderStage::Prepare, prepare_sprites.system())
            .add_system_to_stage(RenderStage::Draw, draw_sprites.system())
            .init_resource::<TransparentPhase>()
            .init_resource::<SpriteShaders>()
            .init_resource::<SpriteBuffers>();
        let render_world = app.sub_app_mut(0).world.cell();
        let mut graph = render_world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_node("main_pass", MainPassNode);
        graph.add_node("sprite", SpriteNode);
        graph.add_node_edge("camera", "main_pass").unwrap();
        graph.add_node_edge("sprite", "main_pass").unwrap();
        graph.add_node(
            "primary_swap_chain",
            WindowSwapChainNode::new(WindowId::primary()),
        );
        graph
            .add_slot_edge(
                "primary_swap_chain",
                WindowSwapChainNode::OUT_TEXTURE,
                "main_pass",
                MainPassNode::IN_COLOR_ATTACHMENT,
            )
            .unwrap();
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
        let vertex_shader =
            Shader::from_glsl(ShaderStage::Vertex, include_str!("vertex_sprite.vert"))
                .get_spirv_shader(None)
                .unwrap();
        let fragment_shader =
            Shader::from_glsl(ShaderStage::Fragment, include_str!("vertex_sprite.frag"))
                .get_spirv_shader(None)
                .unwrap();

        let vertex_layout = vertex_shader.reflect_layout(true).unwrap();
        let fragment_layout = fragment_shader.reflect_layout(true).unwrap();

        let mut pipeline_layout =
            PipelineLayout::from_shader_layouts(&mut [vertex_layout, fragment_layout]);

        let vertex = render_resource_context.create_shader_module_v2(&vertex_shader);
        let fragment = render_resource_context.create_shader_module_v2(&fragment_shader);

        pipeline_layout.vertex_buffer_descriptors = vec![VertexBufferLayout {
            stride: 12,
            name: "Vertex_Position".into(),
            step_mode: InputStepMode::Vertex,
            attributes: vec![VertexAttribute {
                name: "Vertex_Position".into(),
                format: VertexFormat::Float3,
                offset: 0,
                shader_location: 0,
            }],
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
struct SpriteVertex {
    pub position: [f32; 3],
}

unsafe impl Byteable for SpriteVertex {}

struct SpriteBuffers {
    sprite_vertex_buffer: Option<BufferId>,
    sprite_index_buffer: Option<BufferId>,
    staging: Option<BufferId>,
    capacity: usize,
    sprite_count: usize,
    vertices: Vec<SpriteVertex>,
    indices: Vec<u32>,
    quad: Mesh,
}

impl Default for SpriteBuffers {
    fn default() -> Self {
        Self {
            sprite_vertex_buffer: None,
            sprite_index_buffer: None,
            staging: None,
            capacity: 0,
            sprite_count: 0,
            vertices: Vec::new(),
            indices: Vec::new(),
            quad: Quad {
                size: Vec2::new(1.0, 1.0),
                ..Default::default()
            }
            .into(),
        }
    }
}

fn prepare_sprites(
    render_resource_context: Res<Box<dyn RenderResourceContext>>,
    mut sprite_buffers: ResMut<SpriteBuffers>,
    extracted_sprites: Res<ExtractedSprites>,
) {
    let quad_vertex_positions = if let VertexAttributeValues::Float3(vertex_positions) =
        sprite_buffers
            .quad
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .clone()
    {
        vertex_positions
    } else {
        panic!("expected vec3");
    };

    let quad_indices = if let Indices::U32(indices) = sprite_buffers.quad.indices().unwrap() {
        indices.clone()
    } else {
        panic!("expectd u32 indices");
    };

    let sprite_vertex_size = std::mem::size_of::<SpriteVertex>();
    let sprite_vertex_array_len = quad_vertex_positions.len() * extracted_sprites.sprites.len();
    let sprite_vertex_array_size = sprite_vertex_size * sprite_vertex_array_len;

    let sprite_index_size = std::mem::size_of::<u32>();
    let sprite_index_array_len = quad_indices.len() * extracted_sprites.sprites.len();
    let sprite_index_array_size = sprite_index_size * sprite_index_array_len;

    sprite_buffers.sprite_count = extracted_sprites.sprites.len();

    if extracted_sprites.sprites.len() > sprite_buffers.capacity {
        if let Some(staging) = sprite_buffers.staging.take() {
            render_resource_context.remove_buffer(staging);
        }

        if let Some(vertices) = sprite_buffers.sprite_vertex_buffer.take() {
            render_resource_context.remove_buffer(vertices);
        }

        if let Some(indices) = sprite_buffers.sprite_index_buffer.take() {
            render_resource_context.remove_buffer(indices);
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
            size: sprite_vertex_array_size + sprite_index_array_size,
            buffer_usage: BufferUsage::COPY_SRC | BufferUsage::MAP_WRITE,
            mapped_at_creation: true,
        });
        sprite_buffers.staging = Some(staging_buffer);

        let vertex_buffer = render_resource_context.create_buffer(BufferInfo {
            size: sprite_vertex_array_size,
            buffer_usage: BufferUsage::COPY_DST | BufferUsage::VERTEX,
            mapped_at_creation: false,
        });
        sprite_buffers.sprite_vertex_buffer = Some(vertex_buffer);

        let index_buffer = render_resource_context.create_buffer(BufferInfo {
            size: sprite_index_array_size,
            buffer_usage: BufferUsage::COPY_DST | BufferUsage::INDEX,
            mapped_at_creation: false,
        });
        sprite_buffers.sprite_index_buffer = Some(index_buffer);

        sprite_buffers.capacity = extracted_sprites.sprites.len();

        staging_buffer
    };

    sprite_buffers.vertices.clear();
    sprite_buffers.vertices.reserve(sprite_vertex_array_len);

    sprite_buffers.indices.clear();
    sprite_buffers.indices.reserve(sprite_index_array_len);

    for (i, extracted_sprite) in extracted_sprites.sprites.iter().enumerate() {
        for vertex_position in quad_vertex_positions.iter() {
            let mut final_position =
                Vec3::from(*vertex_position) * extracted_sprite.size.extend(1.0);
            final_position = (extracted_sprite.transform * final_position.extend(1.0)).xyz();
            sprite_buffers.vertices.push(SpriteVertex {
                position: final_position.into(),
            });
        }

        for index in quad_indices.iter() {
            sprite_buffers
                .indices
                .push((i * quad_vertex_positions.len()) as u32 + *index);
        }
    }
    render_resource_context.write_mapped_buffer(
        staging_buffer,
        0..sprite_vertex_array_size as u64,
        &mut |data, _context| {
            data[0..sprite_vertex_array_size].copy_from_slice(sprite_buffers.vertices.as_bytes());
        },
    );
    render_resource_context.write_mapped_buffer(
        staging_buffer,
        sprite_vertex_array_size as u64
            ..(sprite_vertex_array_size + sprite_index_array_size) as u64,
        &mut |data, _context| {
            data[0..sprite_index_array_size].copy_from_slice(sprite_buffers.indices.as_bytes());
        },
    );
    render_resource_context.unmap_buffer(staging_buffer);
}

fn draw_sprites(
    render_resource_context: Res<Box<dyn RenderResourceContext>>,
    sprite_buffers: Res<SpriteBuffers>,
    sprite_shaders: Res<SpriteShaders>,
    camera_buffers: Res<CameraBuffers>,
    mut transparent_phase: ResMut<TransparentPhase>,
) {
    let indices = 6;

    const MATRIX_SIZE: usize = std::mem::size_of::<[[f32; 4]; 4]>();
    let bind_group = BindGroupBuilder::default()
        .add_binding(
            0,
            bevy_render::renderer::RenderResourceBinding::Buffer {
                buffer: camera_buffers.view_proj,
                range: 0..MATRIX_SIZE as u64,
                dynamic_index: None,
            },
        )
        .finish();

    let layout = &sprite_shaders.pipeline_descriptor.layout;

    // TODO: TRANSPARENT PHASE SHOULD NOT BE CLEARED HERE!
    transparent_phase.drawn_things.clear();

    // TODO: this will only create the bind group if it isn't already created. this is a bit nasty
    render_resource_context.create_bind_group(layout.bind_groups[0].id, &bind_group);
    for i in 0..sprite_buffers.sprite_count {
        let mut draw = Draw::default();
        draw.set_pipeline(sprite_shaders.pipeline);
        draw.set_vertex_buffer(0, sprite_buffers.sprite_vertex_buffer.unwrap(), 0);
        draw.set_index_buffer(
            sprite_buffers.sprite_index_buffer.unwrap(),
            0,
            IndexFormat::Uint32,
        );
        draw.set_bind_group(0, layout.bind_groups[0].id, bind_group.id, None);

        draw.draw_indexed(
            (i * indices) as u32..(i * indices + indices) as u32,
            0,
            0..1,
        );
        transparent_phase.drawn_things.push(draw);
    }
}
pub struct SpriteNode;

impl Node for SpriteNode {
    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn RenderContext,
        _input: &ResourceSlots,
        _output: &mut ResourceSlots,
    ) {
        let sprite_buffers = world.get_resource::<SpriteBuffers>().unwrap();
        let sprite_vertex_size = std::mem::size_of::<SpriteVertex>();
        let sprite_index_size = std::mem::size_of::<u32>();
        let sprite_vertex_array_size = (sprite_buffers.vertices.len() * sprite_vertex_size) as u64;
        if sprite_buffers.sprite_count != 0 {
            render_context.copy_buffer_to_buffer(
                sprite_buffers.staging.unwrap(),
                0,
                sprite_buffers.sprite_vertex_buffer.unwrap(),
                0,
                sprite_vertex_array_size,
            );
            render_context.copy_buffer_to_buffer(
                sprite_buffers.staging.unwrap(),
                sprite_vertex_array_size,
                sprite_buffers.sprite_index_buffer.unwrap(),
                0,
                (sprite_buffers.indices.len() * sprite_index_size) as u64,
            );
        }
    }
}

#[derive(Default)]
pub struct TransparentPhase {
    drawn_things: Vec<Draw>,
}

pub struct MainPassNode;

impl MainPassNode {
    pub const IN_COLOR_ATTACHMENT: &'static str = "color_attachment";
}

impl Node for MainPassNode {
    fn input(&self) -> &[ResourceSlotInfo] {
        static INPUT: &[ResourceSlotInfo] = &[ResourceSlotInfo {
            name: Cow::Borrowed(MainPassNode::IN_COLOR_ATTACHMENT),
            resource_type: RenderResourceType::Texture,
        }];
        INPUT
    }
    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn RenderContext,
        input: &ResourceSlots,
        _output: &mut ResourceSlots,
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
            sample_count: 1,
        };

        let transparent_phase = world.get_resource::<TransparentPhase>().unwrap();

        render_context.begin_pass(&pass_descriptor, &mut |render_pass: &mut dyn RenderPass| {
            let mut draw_state = DrawState::default();
            for draw in transparent_phase.drawn_things.iter() {
                draw_state.draw(draw, render_pass);
            }
        })
    }
}
