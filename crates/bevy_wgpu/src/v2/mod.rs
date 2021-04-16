use bevy_app::{App, Plugin};
use bevy_render::renderer::RenderResourceContext;
use futures_lite::future;

use crate::{renderer::WgpuRenderResourceContext, WgpuOptions, WgpuRenderer};

#[derive(Default)]
pub struct PipelinedWgpuPlugin;

impl Plugin for PipelinedWgpuPlugin {
    fn build(&self, app: &mut App) {
        let options = app
            .world
            .get_resource::<WgpuOptions>()
            .cloned()
            .unwrap_or_else(WgpuOptions::default);
        let mut wgpu_renderer = future::block_on(WgpuRenderer::new(options));
        let resource_context = WgpuRenderResourceContext::new(wgpu_renderer.device.clone());
        app.world
            .insert_resource::<Box<dyn RenderResourceContext>>(Box::new(resource_context));
        let resource_context = WgpuRenderResourceContext::new(wgpu_renderer.device.clone());
        app.sub_app_mut(0)
            .world
            .insert_resource::<Box<dyn RenderResourceContext>>(Box::new(resource_context));
    }
}
