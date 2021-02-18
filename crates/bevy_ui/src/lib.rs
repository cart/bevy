mod anchors;
pub mod entity;
mod flex;
mod focus;
mod margins;
mod node;
mod render;
pub mod update;
pub mod widget;

pub use anchors::*;
use bevy_asset::Assets;
pub use flex::*;
pub use focus::*;
pub use margins::*;
pub use node::*;
pub use render::*;

pub mod prelude {
    pub use crate::{entity::*, node::*, widget::Button, Anchors, Interaction, Margins};
}

use bevy_app::prelude::*;
use bevy_ecs::{
    schedule::{ParallelSystemDescriptorCoercion, SystemStage},
    system::IntoSystem,
};
use bevy_render::{
    camera::ActiveCameras,
    pipeline::PipelineDescriptor,
    render_graph::{base::Msaa, RenderGraph},
    shader::Shader,
};
use update::ui_z_system;

#[derive(Default)]
pub struct UiPlugin;

pub mod stage {
    pub const UI: &str = "ui";
}

pub mod system {
    pub const FLEX: &str = "flex";
}

impl Plugin for UiPlugin {
    fn build(&self, app: &mut AppBuilder) {
        app.init_resource::<FlexSurface>()
            .add_stage_before(
                bevy_app::stage::POST_UPDATE,
                stage::UI,
                SystemStage::parallel(),
            )
            .add_system_to_stage(bevy_app::stage::PRE_UPDATE, ui_focus_system.system())
            // add these stages to front because these must run before transform update systems
            .add_system_to_stage(stage::UI, widget::text_system.system().before(system::FLEX))
            .add_system_to_stage(
                stage::UI,
                widget::image_node_system.system().before(system::FLEX),
            )
            .add_system_to_stage(stage::UI, flex_node_system.system().label(system::FLEX))
            .add_system_to_stage(stage::UI, ui_z_system.system())
            .add_system_to_stage(bevy_render::stage::DRAW, widget::draw_text_system.system());

        crate::render::add_ui_graph(app.world_mut());
    }
}
