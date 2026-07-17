use crate::{
    ApplySceneError, ResolveSceneError, ResolvedSceneListRoot, ResolvedSceneRoot, Scene,
    SceneDependencies, SceneList,
};
use bevy_asset::{Asset, AssetServer, Handle, LoadErased, UntypedHandle};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{component::Component, system::Query, template::FromTemplate};
use bevy_reflect::TypePath;
use thiserror::Error;

/// An [`Asset`] that holds a [`Scene`], tracks its dependencies, and holds the [`ResolvedSceneRoot`] (after the [`Scene`] has been loaded and resolved).
#[derive(Asset, TypePath)]
pub struct ScenePatch {
    /// A [`Scene`].
    pub scene: Option<Box<dyn Scene>>,
    /// The dependencies of `scene` (populated using [`Scene::register_dependencies`]). These are "asset dependencies" and will affect the load state.
    #[dependency]
    pub dependencies: Vec<UntypedHandle>,
}

impl ScenePatch {
    /// Kicks off a load of the `scene`. This enumerates the scene's dependencies using [`Scene::register_dependencies`], loads
    /// them using the given [`AssetServer`], and assigns the resulting asset handles to [`ScenePatch::dependencies`].
    pub fn load<P: Scene>(mut assets: &AssetServer, scene: P) -> Self {
        Self::load_with(&mut assets, scene)
    }

    /// Same as [`Self::load`], but allows passing in any [`LoadFromPath`] impl for more general
    /// loading cases.
    pub fn load_with<P: Scene>(load_from_path: &mut impl LoadErased, scene: P) -> Self {
        let mut dependencies = SceneDependencies::default();
        scene.register_dependencies(&mut dependencies);
        let dependencies = dependencies
            .iter()
            .map(|i| load_from_path.load_erased(i.type_id, i.path.clone().into()))
            .collect::<Vec<_>>();
        ScenePatch {
            scene: Some(Box::new(scene)),
            dependencies,
        }
    }

    /// Resolves the current `scene` (using [`Scene::resolve`]). This should only be called after every dependency has loaded from the `scene`'s
    /// [`Scene::register_dependencies`]. If successful, it will store the resolved result in [`ScenePatch::resolved`].
    pub fn resolve(
        &mut self,
        assets: &AssetServer,
        resolved_scenes: &Query<&'static ResolvedSceneRoot>,
    ) -> Result<ResolvedSceneRoot, ResolveSceneError> {
        let scene = self.scene.take().ok_or(ResolveSceneError::MissingScene)?;
        Ok(ResolvedSceneRoot::resolve(scene, assets, resolved_scenes)?)
    }
}

/// An [`Error`] that occurs during scene spawning.
#[derive(Error, Debug)]
pub enum SpawnSceneError {
    /// Failed to apply a [`ResolvedScene`].
    ///
    /// [`ResolvedScene`]: crate::ResolvedScene
    #[error(transparent)]
    ApplySceneError(#[from] ApplySceneError),
    #[error(transparent)]
    /// Calling [`Scene::resolve`] failed.
    ResolveSceneError(#[from] ResolveSceneError),
    /// Attempted to spawn a scene that has not been resolved yet.
    #[error("This scene has not been resolved yet and cannot be spawned. It is likely waiting for dependencies to load")]
    UnresolvedSceneError,
}

/// A component that, when added, will queue applying the given [`ScenePatch`] after the scene and its dependencies have been loaded and resolved.
#[derive(Component, FromTemplate, Deref, DerefMut)]
pub struct ScenePatchInstance(#[template] pub Handle<ScenePatch>);

/// An [`Asset`] that holds a [`SceneList`], tracks its dependencies, and holds a [`ResolvedSceneListRoot`] (after the [`SceneList`] has been loaded and resolved)
#[derive(Asset, TypePath)]
pub struct SceneListPatch {
    /// A [`SceneList`].
    pub scene_list: Option<Box<dyn SceneList>>,

    /// The dependencies of `scene_list` (populated using [`SceneList::register_dependencies`]). These are "asset dependencies" and will affect the load state.
    #[dependency]
    pub dependencies: Vec<UntypedHandle>,
}

impl SceneListPatch {
    /// Kicks off a load of the `scene_list`. This enumerates the scene list's dependencies using [`SceneList::register_dependencies`], loads
    /// them using the given [`AssetServer`], and assigns the resulting asset handles to [`SceneListPatch::dependencies`].
    pub fn load<L: SceneList>(assets: &AssetServer, scene_list: L) -> Self {
        let mut dependencies = SceneDependencies::default();
        scene_list.register_dependencies(&mut dependencies);
        let dependencies = dependencies
            .iter()
            .map(|dep| assets.load_builder().load_erased(dep.type_id, &dep.path))
            .collect::<Vec<_>>();
        SceneListPatch {
            scene_list: Some(Box::new(scene_list)),
            dependencies,
        }
    }

    /// Resolves the current `scene` (using [`SceneList::resolve_list`]). This should only be called after every dependency has loaded from the `scene_list`'s
    /// [`SceneList::register_dependencies`].
    pub fn resolve(
        &mut self,
        assets: &AssetServer,
        resolved_scenes: &Query<&ResolvedSceneRoot>,
    ) -> Result<ResolvedSceneListRoot, ResolveSceneError> {
        let scene_list = self
            .scene_list
            .take()
            .ok_or(ResolveSceneError::MissingScene)?;
        ResolvedSceneListRoot::resolve(scene_list, assets, resolved_scenes)
    }
}
