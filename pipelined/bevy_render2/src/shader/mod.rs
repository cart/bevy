#[allow(clippy::module_inception)]
mod shader;
mod shader_bundle;

pub use shader::*;
pub use shader_bundle::*;

use crate::render_asset::RenderAssetPlugin;
use bevy_app::{App, Plugin};
use bevy_asset::AddAsset;
use bevy_ecs::prelude::*;

pub struct ShaderPlugin;

#[derive(Clone, Hash, Debug, Eq, PartialEq, SystemLabel)]
pub enum ShaderRenderSystem {
    PrepareShaders,
}

impl Plugin for ShaderPlugin {
    fn build(&self, app: &mut App) {
        app.add_asset::<Shader>()
            .init_asset_loader::<ShaderLoader>()
            .add_plugin(RenderAssetPlugin::<Shader>::default());

        let render_app = app.sub_app_mut(0);
        render_app.init_resource::<CompiledShaders>();
    }
}
