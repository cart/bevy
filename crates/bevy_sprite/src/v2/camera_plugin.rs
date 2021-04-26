use crate::Sprite;
use bevy_app::{App, Plugin};
use bevy_core::AsBytes;
use bevy_ecs::prelude::*;
use bevy_math::Mat4;
use bevy_render::{
    camera::{ActiveCameras, Camera},
    renderer::{
        BufferId, BufferInfo, BufferMapMode, BufferUsage, RenderContext, RenderResourceContext,
    },
    v2::{
        render_graph::{Node, RenderGraph, ResourceSlots},
        RenderStage,
    },
};
use bevy_transform::components::GlobalTransform;

#[derive(Default)]
pub struct PipelinedCameraPlugin;

impl Plugin for PipelinedCameraPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Sprite>();
        app.sub_app_mut(0)
            .add_system_to_stage(RenderStage::Extract, extract_cameras.system())
            .add_system_to_stage(RenderStage::Prepare, prepare_cameras.system());
        let render_world = app.sub_app_mut(0).world.cell();
        let mut graph = render_world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_node("camera", CameraNode);
    }
}

// TODO: this should live in bevy_render
#[derive(Debug)]
pub struct ExtractedCamera {
    pub projection: Mat4,
    pub transform: Mat4,
}

// TODO: somehow make FromWorld impl work so these don't need to be options?
// This could also use the Option<ResMut<CameraBuffers>>> pattern + commands.insert_resource to avoid options
#[derive(Debug)]
pub struct CameraBuffers {
    pub view_proj: BufferId,
    pub staging: BufferId,
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

        commands.insert_resource(CameraBuffers { staging, view_proj });
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
