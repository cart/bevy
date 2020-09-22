mod loaded_scenes;
mod dynamic_scene;
mod scene_spawner;
mod scene;
mod command;
pub mod serde;

pub use command::*;
pub use loaded_scenes::*;
pub use dynamic_scene::*;
pub use scene_spawner::*;
pub use scene::*;

pub mod prelude {
    pub use crate::{DynamicScene, SceneSpawner, Scene, SpawnSceneCommands};
}

use bevy_app::prelude::*;
use bevy_asset::AddAsset;
use bevy_ecs::IntoThreadLocalSystem;

#[derive(Default)]
pub struct ScenePlugin;

pub const SCENE_STAGE: &str = "scene";

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut AppBuilder) {
        app.add_asset::<DynamicScene>()
            .add_asset::<Scene>()
            .add_asset_loader::<SceneLoader>()
            .init_resource::<SceneSpawner>()
            .add_stage_after(stage::EVENT_UPDATE, SCENE_STAGE)
            .add_system_to_stage(SCENE_STAGE, scene_spawner_system.thread_local_system());
    }
}
