use crate::{
    camera::{ActiveCameras, Camera},
    renderer::{RenderContext, RenderResourceBinding, RenderResources2},
    v2::{
        render_graph::{Node, RenderGraph, ResourceSlots},
        uniform_vec::DynamicUniformVec,
        RenderStage,
    },
};
use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_math::Mat4;
use bevy_transform::components::GlobalTransform;
use bevy_utils::HashMap;

#[derive(Default)]
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(0);
        render_app
            .init_resource::<Cameras>()
            .add_system_to_stage(RenderStage::Extract, extract_cameras.system())
            .add_system_to_stage(RenderStage::Prepare, prepare_cameras.system());
        let mut graph = render_app.world.get_resource_mut::<RenderGraph>().unwrap();
        graph.add_node("camera", CameraNode);
    }
}

#[derive(Default)]
pub struct Cameras {
    pub view_proj_uniforms: DynamicUniformVec<Mat4>,
    pub entities: HashMap<String, Entity>,
}

#[derive(Debug)]
pub struct ExtractedCamera {
    pub projection: Mat4,
    pub transform: Mat4,
    pub name: Option<String>,
}

pub struct CameraUniforms {
    pub view_proj: RenderResourceBinding,
}

fn extract_cameras(
    mut commands: Commands,
    active_cameras: Res<ActiveCameras>,
    query: Query<(&Camera, &GlobalTransform)>,
) {
    for camera in active_cameras.iter() {
        if let Some((camera, transform)) = camera.entity.and_then(|e| query.get(e).ok()) {
            // TODO: remove "spawn_and_forget" hack in favor of more intelligent multiple world handling
            commands.spawn_and_forget((ExtractedCamera {
                projection: camera.projection_matrix,
                transform: transform.compute_matrix(),
                name: camera.name.clone(),
            },));
        }
    }
}

fn prepare_cameras(
    mut commands: Commands,
    render_resources: Res<RenderResources2>,
    mut cameras: ResMut<Cameras>,
    mut extracted_cameras: Query<(Entity, &ExtractedCamera)>,
) {
    cameras.entities.clear();
    cameras
        .view_proj_uniforms
        .reserve_and_clear(extracted_cameras.iter_mut().len(), &render_resources);
    for (entity, camera) in extracted_cameras.iter() {
        let camera_uniforms = CameraUniforms {
            view_proj: cameras
                .view_proj_uniforms
                .push(camera.projection * camera.transform.inverse())
                .unwrap(),
        };
        commands.entity(entity).insert(camera_uniforms);
        if let Some(name) = camera.name.as_ref() {
            cameras.entities.insert(name.to_string(), entity);
        }
    }

    cameras
        .view_proj_uniforms
        .write_to_staging_buffer(&render_resources);
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
        let camera_uniforms = world.get_resource::<Cameras>().unwrap();
        camera_uniforms
            .view_proj_uniforms
            .write_to_uniform_buffer(render_context);
    }
}
