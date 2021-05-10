mod wgpu_render_graph_executor;
pub use wgpu_render_graph_executor::*;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::{Commands, IntoExclusiveSystem, IntoSystem, Res, World};
use bevy_render::{
    renderer::RenderResourceContext,
    v2::{
        render_graph::{ExtractedWindow, ExtractedWindows, RawWindowHandleWrapper},
        RenderStage,
    },
};
use bevy_window::Windows;
use bevy_winit::WinitWindows;
use futures_lite::future;
use raw_window_handle::HasRawWindowHandle;

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
        let wgpu_renderer = future::block_on(WgpuRenderer::new(options));
        let resource_context = WgpuRenderResourceContext::new(wgpu_renderer.device.clone());
        app.world
            .insert_resource::<Box<dyn RenderResourceContext>>(Box::new(resource_context));
        let resource_context = WgpuRenderResourceContext::new(wgpu_renderer.device.clone());

        let render_app = app.sub_app_mut(0);
        render_app.insert_resource::<Box<dyn RenderResourceContext>>(Box::new(resource_context));

        let render_system = get_wgpu_render_system(wgpu_renderer);
        render_app
            .add_system_to_stage(RenderStage::Extract, extract_windows.system())
            .add_system_to_stage(RenderStage::Render, render_system.exclusive_system());
    }
}

pub fn get_wgpu_render_system(mut wgpu_renderer: WgpuRenderer) -> impl FnMut(&mut World) {
    move |world| {
        wgpu_renderer.update_v2(world);
    }
}

fn extract_windows(
    mut commands: Commands,
    winit_windows: Res<WinitWindows>,
    windows: Res<Windows>,
) {
    let mut extracted_windows = ExtractedWindows::default();
    for window in windows.iter() {
        let winit_window = winit_windows.get_window(window.id()).unwrap();
        extracted_windows.insert(
            window.id(),
            ExtractedWindow {
                id: window.id(),
                handle: RawWindowHandleWrapper(winit_window.raw_window_handle()),
                physical_width: window.physical_width(),
                physical_height: window.physical_height(),
                vsync: window.vsync(),
            },
        );
    }

    commands.insert_resource(extracted_windows);
}
