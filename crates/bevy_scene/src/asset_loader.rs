use crate::{
    dynamic_scene::{
        BsnToDynamicValueError, DynamicPatch, DynamicPatchType, DynamicRelatedScenes, DynamicScene,
        DynamicSceneEntry,
    },
    NameEntityReference, Scene, SceneDependencies, ScenePatch,
};
use bevy_asset::{io::Reader, AssetLoader, AssetPath, LoadContext, LoadFromPath};
use bevy_bsn::{
    parse_stream::{OwnedParseError, ParseStream},
    span::Span,
    types::{Bsn, BsnEntry, BsnRoot, BsnScene},
};
use bevy_ecs::{
    name::Name,
    reflect::{AppTypeRegistry, ReflectFromTemplate, ReflectRelationshipTarget, ReflectTemplate},
    template::SceneEntityReference,
    world::{FromWorld, World},
};
use bevy_platform::collections::HashMap;
use bevy_reflect::{std_traits::ReflectDefault, TypePath, TypeRegistry};
use std::{any::Any, io::Error as IoError};
use std::{
    any::TypeId,
    str::Utf8Error,
    sync::{atomic::AtomicUsize, RwLock},
};
use thiserror::Error;

#[derive(TypePath)]
pub struct BsnLoader {
    type_registry: AppTypeRegistry,
    scene_ids: RwLock<HashMap<AssetPath<'static>, usize>>,
    next_scene_id: AtomicUsize,
}

impl FromWorld for BsnLoader {
    fn from_world(world: &mut World) -> Self {
        BsnLoader {
            type_registry: world.resource::<AppTypeRegistry>().clone(),
            next_scene_id: AtomicUsize::default(),
            scene_ids: Default::default(),
        }
    }
}

impl BsnLoader {
    fn get_scene_id(&self, asset_path: AssetPath<'static>) -> usize {
        *self
            .scene_ids
            .write()
            .unwrap()
            .entry(asset_path)
            .or_insert_with(|| {
                self.next_scene_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            })
    }
}

#[derive(Error, Debug)]
pub enum BsnLoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] IoError),
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] Utf8Error),
    #[error(transparent)]
    ParseError(#[from] OwnedParseError),
    #[error(transparent)]
    BsnToSceneError(#[from] BsnToSceneError),
}

impl AssetLoader for BsnLoader {
    type Asset = ScenePatch;

    type Settings = ();

    type Error = BsnLoaderError;

    fn extensions(&self) -> &[&str] {
        &["bsn"]
    }

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut buffer = vec![];
        reader.read_to_end(&mut buffer).await?;
        let input = str::from_utf8(&buffer)?;
        let mut parse_stream = ParseStream::from(Span::from(input));
        let root = parse_stream.parse::<BsnRoot>().map_err(|e| {
            let owned: OwnedParseError = e.into();
            owned
        })?;

        let scene_id = self.get_scene_id(load_context.path().clone());
        let type_registry = self.type_registry.read();
        let mut context = Context::new(scene_id, &type_registry);
        let scene = context.bsn_to_dynamic_scene(&root.0)?;
        let mut scene_dependencies = SceneDependencies::default();
        scene.register_dependencies(&mut scene_dependencies);
        let mut dependencies = Vec::with_capacity(scene_dependencies.len());
        for dependency in scene_dependencies.iter() {
            let handle =
                load_context.load_from_path_erased(dependency.type_id, dependency.path.clone());
            dependencies.push(handle);
        }

        Ok(ScenePatch {
            scene: Some(Box::new(scene)),
            dependencies,
            resolved: None,
        })
    }
}

#[derive(Error, Debug)]
pub enum BsnToSceneError {
    #[error("Missing type registration for {0}")]
    MissingTypeRegistration(String),
    #[error("Missing ReflectDefault for {0}")]
    MissingReflectDefault(String),
    #[error("Missing ReflectFromTemplate for {0}")]
    MissingReflectFromTemplate(String),
    #[error("Missing ReflectTemplate for {0}")]
    MissingReflectTemplate(String),
    #[error("Missing ReflectRelationshipTarget for {0}")]
    MissingReflectRelationshipTarget(String),
    #[error("Missing type registration for {0:?}")]
    MissingTemplateTypeRegistration(TypeId),
    #[error(transparent)]
    BsnToPatchValueError(#[from] BsnToDynamicValueError),
}

struct Context<'a> {
    name_to_index: HashMap<String, usize>,
    scene_id: usize,
    next_index: usize,
    type_registry: &'a TypeRegistry,
}

impl<'a> Context<'a> {
    fn new(scene_id: usize, type_registry: &'a TypeRegistry) -> Self {
        Self {
            name_to_index: Default::default(),
            scene_id,
            next_index: 0,
            type_registry,
        }
    }

    fn bsn_to_dynamic_scene(&mut self, bsn: &Bsn) -> Result<DynamicScene, BsnToSceneError> {
        let mut dynamic_scene = DynamicScene::default();
        for entry in bsn.entries.iter() {
            dynamic_scene.entries.push(self.bsn_entry_to_scene(&entry)?);
        }
        Ok(dynamic_scene)
    }

    fn bsn_entry_to_scene(
        &mut self,
        bsn_entry: &BsnEntry,
    ) -> Result<DynamicSceneEntry, BsnToSceneError> {
        Ok(match bsn_entry {
            BsnEntry::Name(ident) => {
                let index = self.get_name_index(&ident.0);
                DynamicSceneEntry::Name(NameEntityReference {
                    name: Name::new(ident.0.to_string()),
                    reference: SceneEntityReference::scene_asset(self.scene_id, index),
                })
            }
            BsnEntry::FromTemplatePatch(bsn_type) => {
                let path = bsn_type.path.to_type_path();
                // PERF: we can probably accelerate this with a cache from the path to the combined reflected data
                let registration = self
                    .type_registry
                    .get_with_type_path(&path)
                    .ok_or_else(|| BsnToSceneError::MissingTypeRegistration(path.clone()))?;
                let template_type_id = registration
                    .data::<ReflectFromTemplate>()
                    .ok_or_else(|| BsnToSceneError::MissingReflectFromTemplate(path.clone()))?
                    .template_type_id;
                let template_registration =
                    self.type_registry.get(template_type_id).ok_or_else(|| {
                        BsnToSceneError::MissingTemplateTypeRegistration(template_type_id)
                    })?;
                let default_fn = template_registration
                    .data::<ReflectDefault>()
                    .ok_or_else(|| BsnToSceneError::MissingReflectDefault(path.clone()))?
                    .default;
                let reflect_template = template_registration
                    .data::<ReflectTemplate>()
                    .ok_or_else(|| BsnToSceneError::MissingReflectTemplate(path))?;

                let type_info = template_registration.type_info();
                let patch_type = DynamicPatchType::from_bsn_type(bsn_type, type_info)?;

                DynamicSceneEntry::DynamicPatch(DynamicPatch {
                    type_id: registration.type_id(),
                    into_erased: reflect_template.into_erased,
                    reflect_erased: reflect_template.reflect_erased,
                    default: default_fn,
                    patch_type,
                })
            }
            BsnEntry::RelatedSceneList {
                relationship_path,
                scene_list,
            } => {
                // PERF: we should consider trying to accelerate the Children lookup
                let path = relationship_path.to_type_path();
                let registration = self
                    .type_registry
                    .get_with_type_path(&path)
                    .ok_or_else(|| BsnToSceneError::MissingTypeRegistration(path.clone()))?;
                let reflect_relationship_target = registration
                    .data::<ReflectRelationshipTarget>()
                    .ok_or_else(|| BsnToSceneError::MissingReflectRelationshipTarget(path.clone()))?
                    .clone();

                let mut scenes = Vec::new();
                for scene in &scene_list.0 .0 {
                    scenes.push(self.bsn_to_dynamic_scene(scene)?);
                }
                DynamicSceneEntry::RelatedEntities(DynamicRelatedScenes {
                    scenes,
                    reflect_relationship_target,
                    relationship_target_type_id: registration.type_id(),
                    relationship_target_name: registration.type_info().type_path(),
                })
            }
            BsnEntry::CachedScene(bsn_scene) => match bsn_scene {
                BsnScene::Asset(string_lit) => {
                    DynamicSceneEntry::SceneAsset(string_lit.0.clone().into())
                }
            },
        })
    }

    fn get_name_index(&mut self, name: &str) -> usize {
        let (_, v) = self
            .name_to_index
            .raw_entry_mut()
            .from_key(name)
            .or_insert_with(|| {
                let index = self.next_index;
                self.next_index += 1;
                (name.to_string(), index)
            });
        *v
    }
}

#[cfg(test)]
mod tests {
    use std::{ops::Deref, path::Path};

    use crate::WorldSceneExt;
    use crate::{self as bevy_scene, ScenePlugin};
    use bevy_app::{App, TaskPoolPlugin};
    use bevy_asset::{
        io::{
            memory::{Dir, MemoryAssetReader, MemoryAssetWriter},
            AssetSourceBuilder, AssetSourceId,
        },
        AssetApp, AssetPlugin,
    };
    use bevy_ecs::hierarchy::Children;
    use bevy_ecs::{
        component::Component,
        name::Name,
        reflect::{ReflectFromTemplate, ReflectTemplate},
    };
    use bevy_log::LogPlugin;
    use bevy_reflect::{std_traits::ReflectDefault, Reflect};
    use bevy_scene_macros::bsn;

    #[derive(Default, Component, Clone, Reflect, PartialEq, Debug)]
    #[reflect(FromTemplate, Template, Default)]
    struct Test {
        a: f32,
        b: u32,
        c: f32,
        d: String,
        e: bool,
        f: u64,
        g: f64,
        nested: Nested,
    }

    #[derive(Reflect, PartialEq, Debug, Clone, Default)]
    struct Nested {
        value: isize,
    }

    #[derive(Default, Component, Clone, Reflect, PartialEq, Debug)]
    #[reflect(FromTemplate, Template, Default)]
    struct Base;

    #[derive(Default, Component, Clone, Reflect, PartialEq, Debug)]
    #[reflect(FromTemplate, Template, Default)]
    struct Foo(usize, f32);

    #[test]
    fn dynamic_bsn() {
        let (mut app, dir) = create_app();
        app.register_type::<Test>()
            .register_type::<Foo>()
            .register_type::<Base>()
            .register_type::<Children>();
        let test_bsn = r#"
:"dependency.bsn"
#Hello
bevy_scene::asset_loader::tests::Test {
    a: 1.0, b: 2, d: "hello", e: true, f: 1,
    nested: Nested {
        value: 3,
    }
}
bevy_ecs::hierarchy::Children [
    #Child0
    bevy_scene::asset_loader::tests::Foo(0, 1.0)
]
"#;

        let dependency_bsn = r#"
bevy_scene::asset_loader::tests::Base
bevy_scene::asset_loader::tests::Test {
    g: 42.0
}
"#;
        dir.insert_asset_text(Path::new("dependency.bsn"), dependency_bsn);
        dir.insert_asset_text(Path::new("test.bsn"), test_bsn);
        let world = app.world_mut();
        let entity = world
            .queue_spawn_scene(bsn! {
               :"test.bsn"
               Test { f: 20 }
            })
            .id();
        app.update();
        let world = app.world_mut();
        let name = world.get::<Name>(entity).unwrap();
        assert_eq!(name.deref(), "Hello");
        let position = world.get::<Test>(entity).unwrap();
        assert_eq!(
            position,
            &Test {
                a: 1.0,
                b: 2,
                c: 0.0,
                d: "hello".to_string(),
                e: true,
                f: 20,
                g: 42.0,
                nested: Nested { value: 3 }
            }
        );
        let children = world.get::<Children>(entity).unwrap();
        let child0_name = world.get::<Name>(children[0]).unwrap();
        assert_eq!(child0_name.deref(), "Child0");
        let child0_foo = world.get::<Foo>(children[0]).unwrap();
        assert_eq!(child0_foo, &Foo(0, 1.0));
    }

    fn create_app() -> (App, Dir) {
        let mut app = App::new();
        let dir = Dir::default();
        let dir_clone = dir.clone();
        let dir_clone2 = dir.clone();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: dir_clone.clone(),
                })
            })
            .with_writer(move |_| {
                Some(Box::new(MemoryAssetWriter {
                    root: dir_clone2.clone(),
                }))
            }),
        )
        .add_plugins((
            TaskPoolPlugin::default(),
            LogPlugin::default(),
            AssetPlugin {
                watch_for_changes_override: Some(false),
                use_asset_processor_override: Some(false),
                ..Default::default()
            },
            ScenePlugin::default(),
        ));
        (app, dir)
    }
}
