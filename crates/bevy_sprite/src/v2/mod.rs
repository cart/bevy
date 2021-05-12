use crate::Sprite;
use bevy_app::{App, Plugin};
use bevy_core::Byteable;
use bevy_ecs::{prelude::*, system::ParamState};
use bevy_math::{Mat4, Vec2, Vec3, Vec4Swizzles};
use bevy_render::{
    mesh::{shape::Quad, Indices, Mesh, VertexAttributeValues},
    pipeline::{
        BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrite, CullMode, FrontFace,
        IndexFormat, InputStepMode, PipelineLayout, PolygonMode, PrimitiveState, PrimitiveTopology,
        VertexAttribute, VertexBufferLayout, VertexFormat,
    },
    pipeline::{PipelineDescriptorV2, PipelineId},
    renderer::{
        BindGroupBuilder, BindGroupId, BufferUsage, RenderContext, RenderResourceBinding,
        RenderResources2,
    },
    shader::{Shader, ShaderId, ShaderStage, ShaderStagesV2},
    texture::TextureFormat,
    v2::{
        buffer_vec::BufferVec,
        draw_state::TrackedRenderPass,
        features::{
            CameraUniforms, Cameras, Draw, DrawFunctions, Drawable, MainPassPlugin, RenderPhase,
        },
        render_graph::{Node, RenderGraph, ResourceSlots},
        RenderStage,
    },
};
use bevy_transform::components::GlobalTransform;

#[derive(Default)]
pub struct SpritePlugin;

impl Plugin for SpritePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Sprite>();
        app.add_plugin(MainPassPlugin);
        let render_app = app.sub_app_mut(0);
        render_app
            .add_system_to_stage(RenderStage::Extract, extract_sprites.system())
            .add_system_to_stage(RenderStage::Prepare, prepare_sprites.system())
            .add_system_to_stage(RenderStage::Queue, queue_sprites.system())
            .init_resource::<SpriteShaders>()
            .init_resource::<SpriteBuffers>();
        let draw_sprite = DrawSprite::new(&mut render_app.world);
        render_app
            .world
            .get_resource::<DrawFunctions>()
            .unwrap()
            .add(draw_sprite);
        let render_world = app.sub_app_mut(0).world.cell();
        let mut graph = render_world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_node("sprite", SpriteNode);
        graph.add_node_edge("sprite", "main_pass").unwrap();
    }
}

pub struct SpriteShaders {
    _vertex: ShaderId,
    _fragment: ShaderId,
    pipeline: PipelineId,
    pipeline_descriptor: PipelineDescriptorV2,
}

impl FromWorld for SpriteShaders {
    fn from_world(world: &mut World) -> Self {
        let render_resources = world.get_resource::<RenderResources2>().unwrap();
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

        let vertex = render_resources.create_shader_module_v2(&vertex_shader);
        let fragment = render_resources.create_shader_module_v2(&fragment_shader);

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

        pipeline_layout.bind_groups[0].bindings[0].set_dynamic(true);

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

        let pipeline = render_resources.create_render_pipeline_v2(&pipeline_descriptor);

        SpriteShaders {
            _vertex: vertex,
            _fragment: fragment,
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
    vertices: BufferVec<SpriteVertex>,
    indices: BufferVec<u32>,
    quad: Mesh,
    bind_group: Option<BindGroupId>,
    dynamic_uniforms: Vec<u32>,
}

impl Default for SpriteBuffers {
    fn default() -> Self {
        Self {
            vertices: BufferVec::new(BufferUsage::VERTEX),
            indices: BufferVec::new(BufferUsage::INDEX),
            bind_group: None,
            dynamic_uniforms: Vec::new(),
            quad: Quad {
                size: Vec2::new(1.0, 1.0),
                ..Default::default()
            }
            .into(),
        }
    }
}

fn prepare_sprites(
    render_resources: Res<RenderResources2>,
    mut sprite_buffers: ResMut<SpriteBuffers>,
    extracted_sprites: Res<ExtractedSprites>,
) {
    // dont create buffers when there are no sprites
    if extracted_sprites.sprites.len() == 0 {
        return;
    }

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
        panic!("expected u32 indices");
    };

    sprite_buffers.vertices.reserve_and_clear(
        extracted_sprites.sprites.len() * quad_vertex_positions.len(),
        &render_resources,
    );
    sprite_buffers.indices.reserve_and_clear(
        extracted_sprites.sprites.len() * quad_indices.len(),
        &render_resources,
    );

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

    sprite_buffers
        .vertices
        .write_to_staging_buffer(&render_resources);
    sprite_buffers
        .indices
        .write_to_staging_buffer(&render_resources);
}

fn queue_sprites(
    render_resources: Res<RenderResources2>,
    mut sprite_buffers: ResMut<SpriteBuffers>,
    sprite_shaders: Res<SpriteShaders>,
    mut transparent_phase: ResMut<RenderPhase>,
    extracted_sprites: Res<ExtractedSprites>,
    extracted_cameras: Res<Cameras>,
    camera_uniforms: Query<&CameraUniforms>,
) {
    let camera_2d = if let Some(camera_2d) = extracted_cameras
        .entities
        .get(bevy_render::render_graph::base::camera::CAMERA_2D)
    {
        *camera_2d
    } else {
        return;
    };

    if let Ok(camera_uniforms) = camera_uniforms.get(camera_2d) {
        let bind_group = BindGroupBuilder::default()
            .add_binding(0, camera_uniforms.view_proj.clone())
            .finish();

        let layout = &sprite_shaders.pipeline_descriptor.layout;

        // TODO: this will only create the bind group if it isn't already created. this is a bit nasty
        render_resources.create_bind_group(layout.bind_groups[0].id, &bind_group);
        sprite_buffers.bind_group = Some(bind_group.id);
        sprite_buffers.dynamic_uniforms.clear();
        if let RenderResourceBinding::Buffer {
            dynamic_index: Some(index),
            ..
        } = camera_uniforms.view_proj
        {
            sprite_buffers.dynamic_uniforms.push(index);
        }
    }

    for i in 0..extracted_sprites.sprites.iter().len() {
        transparent_phase.add(Drawable {
            draw_function: 0,
            draw_key: i,
            sort_key: 0,
        });
    }

    // TODO: this shouldn't happen here
    transparent_phase.sort();
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
        sprite_buffers
            .vertices
            .write_to_uniform_buffer(render_context);
        sprite_buffers
            .indices
            .write_to_uniform_buffer(render_context);
    }
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
        pass.set_vertex_buffer(0, sprite_buffers.vertices.buffer().unwrap(), 0);
        pass.set_index_buffer(
            sprite_buffers.indices.buffer().unwrap(),
            0,
            IndexFormat::Uint32,
        );
        pass.set_bind_group(
            0,
            layout.bind_groups[0].id,
            sprite_buffers.bind_group.unwrap(),
            Some(&sprite_buffers.dynamic_uniforms),
        );

        pass.draw_indexed(
            (draw_key * INDICES) as u32..(draw_key * INDICES + INDICES) as u32,
            0,
            0..1,
        );
    }
}
