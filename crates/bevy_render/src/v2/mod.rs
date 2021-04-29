pub mod render_graph;
pub mod draw_state;

use self::render_graph::RenderGraph;
use crate::camera::{self, ActiveCameras, Camera, OrthographicProjection};
use bevy_app::{App, CoreStage, Plugin};
use bevy_ecs::prelude::*;

#[derive(Default)]
pub struct PipelinedRenderPlugin;

/// The names of the default App stages
#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
pub enum RenderStage {
    Extract,
    Prepare,
    Draw,
    Render,
}

impl Plugin for PipelinedRenderPlugin {
    fn build(&self, app: &mut App) {
        let mut active_cameras = ActiveCameras::default();
        active_cameras.add(crate::base::camera::CAMERA_2D);
        app.register_type::<Camera>()
            .insert_resource(active_cameras)
            .add_system_to_stage(
                CoreStage::PostUpdate,
                camera::active_cameras_system.system(),
            )
            .add_system_to_stage(
                CoreStage::PostUpdate,
                camera::camera_system::<OrthographicProjection>.system(),
            );
        let mut render_app = App::empty();
        let mut extract_stage = SystemStage::parallel();
        // don't apply buffers when the stage finishes running
        // extract stage runs on the app world, but the buffers are applied to the render world
        extract_stage.set_apply_buffers(false);
        render_app
            .add_stage(RenderStage::Extract, extract_stage)
            .add_stage(RenderStage::Prepare, SystemStage::parallel())
            .add_stage(RenderStage::Draw, SystemStage::parallel())
            .add_stage(RenderStage::Render, SystemStage::parallel());
        render_app.insert_resource(RenderGraph::default());
        app.add_sub_app(render_app, |app_world, render_app| {
            // extract
            extract(app_world, render_app);

            // prepare
            let prepare = render_app
                .schedule
                .get_stage_mut::<SystemStage>(&RenderStage::Prepare)
                .unwrap();
            prepare.run(&mut render_app.world);

            // prepare
            let draw = render_app
                .schedule
                .get_stage_mut::<SystemStage>(&RenderStage::Draw)
                .unwrap();
            draw.run(&mut render_app.world);

            // render
            let render = render_app
                .schedule
                .get_stage_mut::<SystemStage>(&RenderStage::Render)
                .unwrap();
            render.run(&mut render_app.world);
        });
    }
}

fn extract(app_world: &mut World, render_app: &mut App) {
    let extract = render_app
        .schedule
        .get_stage_mut::<SystemStage>(&RenderStage::Extract)
        .unwrap();
    extract.run(app_world);
    extract.apply_buffers(&mut render_app.world);
}
