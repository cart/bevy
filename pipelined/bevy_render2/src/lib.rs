pub mod camera;
pub mod color;
pub mod mesh;
pub mod primitives;
pub mod render_asset;
pub mod render_component;
pub mod render_graph;
pub mod render_phase;
pub mod render_resource;
pub mod renderer;
pub mod texture;
pub mod view;

use bevy_core::Time;
use crossbeam_channel::{Receiver, Sender};
pub use once_cell;

use crate::{
    camera::CameraPlugin,
    mesh::MeshPlugin,
    render_graph::RenderGraph,
    render_resource::{RenderPipelineCache, Shader, ShaderLoader},
    renderer::render_system,
    texture::ImagePlugin,
    view::{ViewPlugin, WindowRenderPlugin},
};
use bevy_app::{App, AppLabel, CoreStage, Plugin, SubApp};
use bevy_asset::{AddAsset, AssetServer};
use bevy_ecs::prelude::*;
use std::ops::{Deref, DerefMut};
use wgpu::Backends;

/// Contains the default Bevy rendering backend based on wgpu.
#[derive(Default)]
pub struct RenderPlugin;

/// The labels of the default App rendering stages.
#[derive(Debug, Hash, PartialEq, Eq, Clone, StageLabel)]
pub enum RenderStage {
    /// Extract data from the "app world" and insert it into the "render world".
    /// This step should be kept as short as possible to increase the "pipelining potential" for
    /// running the next frame while rendering the current frame.
    Extract,

    /// Prepare render resources from the extracted data for the GPU.
    Prepare,

    /// Create [`BindGroups`](crate::render_resource::BindGroup) that depend on
    /// [`Prepare`](RenderStage::Prepare) data and queue up draw calls to run during the
    /// [`Render`](RenderStage::Render) stage.
    Queue,

    // TODO: This could probably be moved in favor of a system ordering abstraction in Render or Queue
    /// Sort the [`RenderPhases`](crate::render_phase::RenderPhase) here.
    PhaseSort,

    /// Actual rendering happens here.
    /// In most cases, only the render backend should insert resources here.
    Render,

    /// Cleanup render resources here.
    Cleanup,
}

/// The Render App World. This is only available as a resource during the Extract step.
#[derive(Default)]
pub struct RenderWorld(World);

impl Deref for RenderWorld {
    type Target = World;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RenderWorld {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// A Label for the rendering sub-app.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, AppLabel)]
pub struct RenderApp;

/// A "scratch" world used to avoid allocating new worlds every frame when
/// swapping out the [`RenderWorld`].
#[derive(Default)]
struct ScratchRenderWorld(World);

#[derive(Clone)]
pub struct RenderAppChannel {
    pub update_sender: Sender<App>,
    pub update_receiver: Receiver<App>,
    pub renderer_sender: Sender<App>,
    pub renderer_receiver: Receiver<App>,
}

impl Default for RenderAppChannel {
    fn default() -> Self {
        let (update_sender, renderer_receiver) = crossbeam_channel::bounded(1);
        let (renderer_sender, update_receiver) = crossbeam_channel::bounded(1);
        Self {
            update_sender,
            update_receiver,
            renderer_sender,
            renderer_receiver,
        }
    }
}

#[derive(Clone, Hash, Debug, PartialEq, Eq, SystemLabel)]
pub enum RenderSystem {
    Extract,
}

struct RenderTick {
    last_tick: u128,
    requested_tick: u128,
}

impl Default for RenderTick {
    fn default() -> Self {
        Self {
            last_tick: u128::MAX,
            requested_tick: u128::MAX,
        }
    }
}

impl RenderTick {
    fn request_render(&mut self, tick: u128) {
        self.requested_tick = tick;
    }

    fn render_tick(&mut self) -> bool {
        let should_tick = self.last_tick != self.requested_tick;
        self.last_tick = self.requested_tick;
        should_tick
    }
}

impl Plugin for RenderPlugin {
    /// Initializes the renderer, sets up the [`RenderStage`](RenderStage) and creates the rendering sub-app.
    fn build(&self, app: &mut App) {
        let default_backend = if cfg!(not(target_arch = "wasm32")) {
            Backends::PRIMARY
        } else {
            Backends::GL
        };
        let backends = wgpu::util::backend_bits_from_env().unwrap_or(default_backend);
        let instance = wgpu::Instance::new(backends);
        let surface = {
            let world = app.world.cell();
            let windows = world.get_resource_mut::<bevy_window::Windows>().unwrap();
            let raw_handle = windows.get_primary().map(|window| unsafe {
                let handle = window.raw_window_handle().get_handle();
                instance.create_surface(&handle)
            });
            raw_handle
        };
        let (device, queue) = futures_lite::future::block_on(renderer::initialize_renderer(
            &instance,
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface.as_ref(),
                ..Default::default()
            },
            &wgpu::DeviceDescriptor {
                features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                #[cfg(not(target_arch = "wasm32"))]
                limits: wgpu::Limits::default(),
                #[cfg(target_arch = "wasm32")]
                limits: wgpu::Limits {
                    ..wgpu::Limits::downlevel_webgl2_defaults()
                },
                ..Default::default()
            },
        ));
        let render_app_channel = RenderAppChannel::default();
        app.insert_resource(device.clone())
            .insert_resource(queue.clone())
            .add_asset::<Shader>()
            .init_asset_loader::<ShaderLoader>()
            .insert_resource(render_app_channel)
            .init_resource::<ScratchRenderWorld>()
            .add_system_to_stage(
                CoreStage::Last,
                extract_system
                    .exclusive_system()
                    .label(RenderSystem::Extract)
                    .at_end(),
            );
        let render_pipeline_cache = RenderPipelineCache::new(device.clone());
        let asset_server = app.world.get_resource::<AssetServer>().unwrap().clone();

        let mut render_app = App::empty();
        let mut extract_stage =
            SystemStage::parallel().with_system(RenderPipelineCache::extract_shaders);
        // don't apply buffers when the stage finishes running
        // extract stage runs on the app world, but the buffers are applied to the render world
        extract_stage.set_apply_buffers(false);
        render_app
            .add_stage(RenderStage::Extract, extract_stage)
            .add_stage(RenderStage::Prepare, SystemStage::parallel())
            .add_stage(RenderStage::Queue, SystemStage::parallel())
            .add_stage(RenderStage::PhaseSort, SystemStage::parallel())
            .add_stage(
                RenderStage::Render,
                SystemStage::parallel()
                    .with_system(RenderPipelineCache::process_pipeline_queue_system)
                    .with_system(render_system.exclusive_system().at_end()),
            )
            .add_stage(RenderStage::Cleanup, SystemStage::parallel())
            .insert_resource(instance)
            .insert_resource(device)
            .insert_resource(queue)
            .insert_resource(render_pipeline_cache)
            .insert_resource(asset_server)
            .init_resource::<RenderTick>()
            .init_resource::<RenderGraph>();

        app.add_sub_app(RenderApp, render_app, move |render_app| {
            {
                let mut last_render_tick =
                    render_app.world.get_resource_mut::<RenderTick>().unwrap();
                if !last_render_tick.render_tick() {
                    return;
                }
            }
            #[cfg(feature = "trace")]
            let render_span = bevy_utils::tracing::info_span!("renderer subapp");

            {
                #[cfg(feature = "trace")]
                let stage_span = bevy_utils::tracing::info_span!("stage", name = "prepare");
                #[cfg(feature = "trace")]
                let _stage_guard = stage_span.enter();

                // prepare
                let prepare = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Prepare)
                    .unwrap();
                prepare.run(&mut render_app.world);
            }

            {
                #[cfg(feature = "trace")]
                let stage_span = bevy_utils::tracing::info_span!("stage", name = "queue");
                #[cfg(feature = "trace")]
                let _stage_guard = stage_span.enter();

                // queue
                let queue = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Queue)
                    .unwrap();
                queue.run(&mut render_app.world);
            }

            {
                #[cfg(feature = "trace")]
                let stage_span = bevy_utils::tracing::info_span!("stage", name = "sort");
                #[cfg(feature = "trace")]
                let _stage_guard = stage_span.enter();

                // phase sort
                let phase_sort = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::PhaseSort)
                    .unwrap();
                phase_sort.run(&mut render_app.world);
            }

            {
                #[cfg(feature = "trace")]
                let stage_span = bevy_utils::tracing::info_span!("stage", name = "render");
                #[cfg(feature = "trace")]
                let _stage_guard = stage_span.enter();

                // render
                let render = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Render)
                    .unwrap();
                render.run(&mut render_app.world);
            }

            {
                #[cfg(feature = "trace")]
                let stage_span = bevy_utils::tracing::info_span!("stage", name = "cleanup");
                #[cfg(feature = "trace")]
                let _stage_guard = stage_span.enter();

                // cleanup
                let cleanup = render_app
                    .schedule
                    .get_stage_mut::<SystemStage>(&RenderStage::Cleanup)
                    .unwrap();
                cleanup.run(&mut render_app.world);

                render_app.world.clear_entities();
            }
        });

        app.add_plugin(WindowRenderPlugin)
            .add_plugin(CameraPlugin)
            .add_plugin(ViewPlugin)
            .add_plugin(MeshPlugin)
            .add_plugin(ImagePlugin);
    }
}

/// Executes the [`Extract`](RenderStage::Extract) stage of the renderer.
/// This updates the render world with the extracted ECS data of the current frame.
fn extract_system(world: &mut World) {
    world.resource_scope(|world, channel: Mut<RenderAppChannel>| {
        let mut render_app = channel.update_receiver.recv().unwrap();
        {
            #[cfg(feature = "trace")]
            let stage_span = bevy_utils::tracing::info_span!("stage", name = "reserve_and_flush");
            #[cfg(feature = "trace")]
            let _stage_guard = stage_span.enter();

            // reserve all existing app entities for use in render_app
            // they can only be spawned using `get_or_spawn()`
            let meta_len = world.entities().meta.len();
            render_app
                .world
                .entities()
                .reserve_entities(meta_len as u32);

            // flushing as "invalid" ensures that app world entities aren't added as "empty archetype" entities by default
            // these entities cannot be accessed without spawning directly onto them
            // this _only_ works as expected because clear_entities() is called at the end of every frame.
            render_app.world.entities_mut().flush_as_invalid();
        }

        {
            #[cfg(feature = "trace")]
            let stage_span = bevy_utils::tracing::info_span!("stage", name = "extract");
            #[cfg(feature = "trace")]
            let _stage_guard = stage_span.enter();

            // extract
            let extract = render_app
                .schedule
                .get_stage_mut::<SystemStage>(&RenderStage::Extract)
                .unwrap();

            // temporarily add the render world to the app world as a resource
            let scratch_world = world.remove_resource::<ScratchRenderWorld>().unwrap();
            let render_world = std::mem::replace(&mut render_app.world, scratch_world.0);
            world.insert_resource(RenderWorld(render_world));

            extract.run(world);

            // add the render world back to the render app
            let render_world = world.remove_resource::<RenderWorld>().unwrap();
            let scratch_world = std::mem::replace(&mut render_app.world, render_world.0);
            world.insert_resource(ScratchRenderWorld(scratch_world));

            extract.apply_buffers(&mut render_app.world);
        }

        let time = world.get_resource::<Time>().unwrap();
        let mut render_tick = render_app.world.get_resource_mut::<RenderTick>().unwrap();
        render_tick.request_render(time.tick());
        channel.update_sender.send(render_app).unwrap();
    })
}
