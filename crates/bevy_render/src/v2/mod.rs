pub mod features;
pub mod render_graph;

use crate::camera::{self, Camera, OrthographicProjection};

use self::render_graph::RenderGraph;
use bevy_app::{App, CoreStage, Plugin};
use bevy_ecs::prelude::*;

#[derive(Default)]
pub struct PipelinedRenderPlugin;

impl Plugin for PipelinedRenderPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Camera>()
            .add_system_to_stage(
                CoreStage::PostUpdate,
                camera::active_cameras_system.system(),
            )
            .add_system_to_stage(
                CoreStage::PostUpdate,
                camera::camera_system::<OrthographicProjection>.system(),
            );
        let mut render_app = App::new();
        render_app
            .insert_resource(RenderGraph::default());
        app.add_sub_app(render_app);
    }
}