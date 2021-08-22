use bevy::ecs::system::{lifetimeless::*, SystemParamItem};
use bevy::pbr2::{DrawMesh, SetMeshViewBindGroup, SetTransformBindGroup};
use bevy::prelude::AssetServer;
use bevy::render2::render_asset::PrepareAssetError;
use bevy::render2::render_phase::{AddDrawCommand, DrawCommand};
use bevy::render2::shader::{
    CompiledShaders, FragmentDescriptor, RenderPipelineBundle, VertexDescriptor,
};
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    ecs::prelude::*,
    math::{Mat4, Vec3, Vec4},
    pbr2::PbrPipeline,
    prelude::{AddAsset, App, Assets, GlobalTransform, Handle, Plugin, Transform},
    reflect::TypeUuid,
    render2::{
        camera::PerspectiveCameraBundle,
        color::Color,
        core_pipeline::Transparent3d,
        mesh::{shape, Mesh},
        render_asset::{RenderAsset, RenderAssetPlugin, RenderAssets},
        render_component::RenderComponentPlugin,
        render_phase::{DrawFunctions, RenderPhase, TrackedRenderPass},
        render_resource::*,
        renderer::RenderDevice,
        shader::Shader,
        texture::BevyDefault,
        view::ExtractedView,
        RenderStage,
    },
    PipelinedDefaultPlugins,
};
use crevice::std140::{AsStd140, Std140};

#[derive(Debug, Clone, TypeUuid)]
#[uuid = "4ee9c363-1124-4113-890e-199d81b00281"]
pub struct CustomMaterial {
    color: Color,
}

#[derive(Clone)]
pub struct GpuCustomMaterial {
    buffer: Buffer,
    bind_group: BindGroup,
    pipeline: RenderPipeline,
}

impl RenderAsset for CustomMaterial {
    type ExtractedAsset = CustomMaterial;
    type PreparedAsset = GpuCustomMaterial;
    type Param = (
        SRes<RenderDevice>,
        SRes<PbrPipeline>,
        SRes<CustomPipeline>,
        SRes<RenderAssets<Shader>>,
        SResMut<CompiledShaders>,
    );
    fn extract_asset(&self) -> Self::ExtractedAsset {
        self.clone()
    }

    fn prepare_asset(
        extracted_asset: Self::ExtractedAsset,
        (render_device, pbr_pipeline, custom_pipeline, shaders, compiled_shaders): &mut SystemParamItem<
            Self::Param,
        >,
    ) -> Result<Self::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>> {
        let compiled_pipeline_bundle = if let Some(compiled_pipeline_bundle) = custom_pipeline
            .pipeline_bundle
            .compile(compiled_shaders, shaders, render_device, Vec::new())
        {
            compiled_pipeline_bundle
        } else {
            return Err(PrepareAssetError::RetryNextUpdate(extracted_asset));
        };

        let color: Vec4 = extracted_asset.color.as_rgba_linear().into();
        let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            contents: color.as_std140().as_bytes(),
            label: None,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
        });
        let bind_group = render_device.create_bind_group(&BindGroupDescriptor {
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
            label: None,
            layout: &custom_pipeline.material_layout,
        });

        let pipeline_layout = render_device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            push_constant_ranges: &[],
            bind_group_layouts: &[
                &pbr_pipeline.view_layout,
                &pbr_pipeline.mesh_layout,
                &custom_pipeline.material_layout,
            ],
        });

        let descriptor = compiled_pipeline_bundle.get_descriptor(
            &[VertexBufferLayout {
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
            PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                polygon_mode: PolygonMode::Fill,
                clamp_depth: false,
                conservative: false,
            },
            Some(&pipeline_layout),
            MultisampleState::default(),
        );
        let pipeline = render_device.create_render_pipeline(&descriptor);
        Ok(GpuCustomMaterial {
            buffer,
            bind_group,
            pipeline,
        })
    }
}
pub struct CustomMaterialPlugin;

impl Plugin for CustomMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_asset::<CustomMaterial>()
            .add_plugin(RenderComponentPlugin::<Handle<CustomMaterial>>::default())
            .add_plugin(RenderAssetPlugin::<CustomMaterial>::default());
        let render_app = app.sub_app_mut(0);
        render_app.add_draw_command::<Transparent3d, DrawCustom, DrawCustom>();
        render_app
            .init_resource::<CustomPipeline>()
            .add_system_to_stage(RenderStage::Queue, queue_custom.system());
    }
}

fn main() {
    App::new()
        .add_plugins(PipelinedDefaultPlugins)
        .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(LogDiagnosticsPlugin::default())
        .add_plugin(CustomMaterialPlugin)
        .add_startup_system(setup.system())
        .run();
}

/// set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CustomMaterial>>,
) {
    // cube
    commands.spawn().insert_bundle((
        meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
        Transform::from_xyz(0.0, 0.5, 0.0),
        GlobalTransform::default(),
        materials.add(CustomMaterial {
            color: Color::GREEN,
        }),
    ));

    // camera
    commands.spawn_bundle(PerspectiveCameraBundle {
        transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..Default::default()
    });
}

pub struct CustomPipeline {
    material_layout: BindGroupLayout,
    pipeline_bundle: RenderPipelineBundle,
}

// TODO: this pattern for initializing the shaders / pipeline isn't ideal. this should be handled by the asset system
impl FromWorld for CustomPipeline {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.get_resource::<AssetServer>().unwrap();
        let pipeline_bundle = RenderPipelineBundle {
            vertex: VertexDescriptor {
                entry_point: "vertex".to_string(),
                handle: asset_server.load("shaders/custom.wgsl"),
            },
            fragment: Some(FragmentDescriptor {
                entry_point: "fragment".to_string(),
                handle: asset_server.load("shaders/custom.wgsl"),
                targets: vec![ColorTargetState {
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
        };
        let render_device = world.get_resource::<RenderDevice>().unwrap();
        let material_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStage::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(Vec4::std140_size_static() as u64),
                },
                count: None,
            }],
            label: None,
        });

        CustomPipeline {
            pipeline_bundle,
            material_layout,
        }
    }
}

pub fn queue_custom(
    transparent_3d_draw_functions: Res<DrawFunctions<Transparent3d>>,
    materials: Res<RenderAssets<CustomMaterial>>,
    material_meshes: Query<(Entity, &Handle<CustomMaterial>, &Mat4), With<Handle<Mesh>>>,
    mut views: Query<(&ExtractedView, &mut RenderPhase<Transparent3d>)>,
) {
    let draw_custom = transparent_3d_draw_functions
        .read()
        .get_id::<DrawCustom>()
        .unwrap();
    for (view, mut transparent_phase) in views.iter_mut() {
        let view_matrix = view.transform.compute_matrix();
        let view_row_2 = view_matrix.row(2);
        for (entity, material_handle, transform) in material_meshes.iter() {
            if materials.contains_key(material_handle) {
                transparent_phase.add(Transparent3d {
                    entity,
                    draw_function: draw_custom,
                    distance: view_row_2.dot(transform.col(3)),
                });
            }
        }
    }
}

type DrawCustom = (
    SetCustomMaterialPipeline,
    SetMeshViewBindGroup<0>,
    SetTransformBindGroup<1>,
    DrawMesh,
);

struct SetCustomMaterialPipeline;
impl DrawCommand<Transparent3d> for SetCustomMaterialPipeline {
    type Param = (
        SRes<RenderAssets<CustomMaterial>>,
        SQuery<Read<Handle<CustomMaterial>>>,
    );
    fn draw<'w>(
        _view: Entity,
        item: &Transparent3d,
        (materials, query): SystemParamItem<'_, 'w, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) {
        let material_handle = query.get(item.entity).unwrap();
        let material = materials.into_inner().get(material_handle).unwrap();
        pass.set_render_pipeline(&material.pipeline);
        pass.set_bind_group(2, &material.bind_group, &[]);
    }
}
