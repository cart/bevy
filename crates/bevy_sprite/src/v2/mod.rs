use crate::Sprite;
use bevy_app::{App, Plugin};
use bevy_core::{AsBytes, Byteable};
use bevy_ecs::{prelude::*, system::ParamState};
use bevy_math::{Mat4, Vec2, Vec3, Vec4Swizzles};
use bevy_render::{
    color::Color,
    mesh::{shape::Quad, Indices, Mesh, VertexAttributeValues},
    pass::{
        LoadOp, Operations, PassDescriptor, RenderPass, RenderPassColorAttachmentDescriptor,
        TextureAttachment,
    },
    pipeline::{
        BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrite, CullMode, FrontFace,
        IndexFormat, InputStepMode, PipelineLayout, PolygonMode, PrimitiveState, PrimitiveTopology,
        VertexAttribute, VertexBufferLayout, VertexFormat,
    },
    pipeline::{PipelineDescriptorV2, PipelineId},
    renderer::{
        BindGroupBuilder, BindGroupId, BufferId, BufferInfo, BufferMapMode, BufferUsage,
        RenderContext, RenderResourceContext, RenderResourceType,
    },
    shader::{Shader, ShaderId, ShaderStage, ShaderStagesV2},
    texture::TextureFormat,
    v2::{
        draw_state::TrackedRenderPass,
        features::{CameraBuffers, CameraPlugin},
        render_graph::{Node, RenderGraph, ResourceSlotInfo, ResourceSlots, WindowSwapChainNode},
        RenderStage,
    },
};
use bevy_transform::components::GlobalTransform;
use bevy_window::WindowId;
use parking_lot::Mutex;
use std::borrow::Cow;

#[derive(Default)]
pub struct SpritePlugin;

impl Plugin for SpritePlugin {
    fn build(&self, app: &mut App) {
        let draw_functions = DrawFunctions::default();
        draw_functions
            .draw_function
            .lock()
            .push(Box::new(DrawSprite::new(&mut app.sub_app_mut(0).world)));
        app.register_type::<Sprite>();
        app.add_plugin(CameraPlugin);
        app.sub_app_mut(0)
            .add_system_to_stage(RenderStage::Extract, extract_sprites.system())
            .add_system_to_stage(
                RenderStage::Prepare,
                clear_transparent_phase.exclusive_system().at_start(),
            )
            .add_system_to_stage(RenderStage::Prepare, prepare_sprites.system())
            // TODO: remove this ugly thing
            .add_system_to_stage(RenderStage::Draw, sprite_bind_group_system.system())
            .init_resource::<TransparentPhase>()
            .insert_resource(draw_functions)
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
    bind_group: Option<BindGroupId>,
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
            bind_group: None,
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
    mut transparent_phase: ResMut<TransparentPhase>,
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

        transparent_phase.drawn_things.push(Drawable {
            draw_function: 0,
            draw_key: i,
            sort_key: 0,
        });

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

// TODO: sort out the best place for this
fn sprite_bind_group_system(
    render_resource_context: Res<Box<dyn RenderResourceContext>>,
    mut sprite_buffers: ResMut<SpriteBuffers>,
    sprite_shaders: Res<SpriteShaders>,
    camera_buffers: Res<CameraBuffers>,
) {
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

    // TODO: this will only create the bind group if it isn't already created. this is a bit nasty
    render_resource_context.create_bind_group(layout.bind_groups[0].id, &bind_group);
    sprite_buffers.bind_group = Some(bind_group.id);
}

// TODO: sort out the best place for this
fn clear_transparent_phase(mut transparent_phase: ResMut<TransparentPhase>) {
    // TODO: TRANSPARENT PHASE SHOULD NOT BE CLEARED HERE!
    transparent_phase.drawn_things.clear();
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

pub trait Draw: Send + Sync {
    fn draw(&mut self, world: &World, pass: &mut TrackedRenderPass, draw_key: usize);
}

pub struct DrawSprite {
    param_state: ParamState<(Res<'static, SpriteShaders>, Res<'static, SpriteBuffers>)>,
}

impl DrawSprite {
    fn new(world: &mut World) -> Self {
        Self {
            param_state: ParamState::new(world),
        }
    }
}

impl Draw for DrawSprite {
    fn draw(&mut self, world: &World, pass: &mut TrackedRenderPass, draw_key: usize) {
        const INDICES: usize = 6;
        let (sprite_shaders, sprite_buffers) = self.param_state.get(world);
        let layout = &sprite_shaders.pipeline_descriptor.layout;
        pass.set_pipeline(sprite_shaders.pipeline);
        pass.set_vertex_buffer(0, sprite_buffers.sprite_vertex_buffer.unwrap(), 0);
        pass.set_index_buffer(
            sprite_buffers.sprite_index_buffer.unwrap(),
            0,
            IndexFormat::Uint32,
        );
        pass.set_bind_group(
            0,
            layout.bind_groups[0].id,
            sprite_buffers.bind_group.unwrap(),
            None,
        );

        pass.draw_indexed(
            (draw_key * INDICES) as u32..(draw_key * INDICES + INDICES) as u32,
            0,
            0..1,
        );
    }
}

#[derive(Default)]
pub struct DrawFunctions {
    pub draw_function: Mutex<Vec<Box<dyn Draw>>>,
}

pub struct Drawable {
    pub draw_function: usize,
    pub draw_key: usize,
    pub sort_key: usize,
}

#[derive(Default)]
pub struct TransparentPhase {
    drawn_things: Vec<Drawable>,
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
        let draw_functions = world.get_resource::<DrawFunctions>().unwrap();

        render_context.begin_pass(&pass_descriptor, &mut |render_pass: &mut dyn RenderPass| {
            let mut draw_functions = draw_functions.draw_function.lock();
            let mut tracked_pass = TrackedRenderPass::new(render_pass);
            for drawable in transparent_phase.drawn_things.iter() {
                draw_functions[drawable.draw_function].draw(
                    world,
                    &mut tracked_pass,
                    drawable.draw_key,
                );
            }
        })
    }
}
