use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_math::Mat4;
use bevy_render::{camera::{ActiveCameras, Camera}, v2::render_graph::{Node, RenderGraph}};
use bevy_transform::components::GlobalTransform;

use crate::Sprite;

#[derive(Default)]
pub struct PipelinedSpritePlugin;

impl Plugin for PipelinedSpritePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Sprite>();
        app.sub_app_mut(0).add_system(extract_cameras.system());
        let render_world = app.sub_app_mut(0).world.cell();
        let mut render_graph = render_world.get_resource_mut::<RenderGraph>().unwrap();
        render_graph.add_node("sprite_node", SpriteNode);
    }
}

#[derive(Debug)]
pub struct ExtractedCamera {
    projection: Mat4,
    transform: Mat4,
}

fn extract_cameras(
    mut commands: Commands,
    active_cameras: Res<ActiveCameras>,
    query: Query<(&Camera, &GlobalTransform)>,
) {
    println!("extract");
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

pub struct SpriteNode;

impl Node for SpriteNode {
    fn update(
        &mut self,
        world: &World,
        render_context: &mut dyn bevy_render::renderer::RenderContext,
        input: &bevy_render::v2::render_graph::ResourceSlots,
        output: &mut bevy_render::v2::render_graph::ResourceSlots,
    ) {
        println!("{:?}", world.get_resource::<ExtractedCamera>());
    }
}