use alloc::{borrow::Cow, boxed::Box, format, vec::Vec};
use core::any::{Any, TypeId};
use serde::{de::Error as _, ser::Error as _, Deserialize, Deserializer, Serialize};
use thiserror::Error;
use tracing::warn;
use uuid::Uuid;

use bevy_ecs::{entity::Entity, query::With, reflect::ReflectComponent, world::World};
use bevy_reflect::{
    serde::{ReflectDeserializerProcessor, ReflectSerializerProcessor},
    CreateTypeData, FromReflect, PartialReflect, Reflect, TypeRegistry,
};

use crate::{
    meta::Empty, Asset, AssetId, AssetPath, AssetServer, DirectAssetAccessExt, Handle, LoadContext,
    UntypedHandle,
};

/// Type data for the [`TypeRegistry`] used to operate on reflected [`Asset`]s.
///
/// This type provides similar methods to [`Assets<T>`] like [`get`](ReflectAsset::get),
/// [`add`](ReflectAsset::add) and [`remove`](ReflectAsset::remove), but can be used in situations where you don't know which asset type `T` you want
/// until runtime.
///
/// [`ReflectAsset`] can be obtained via [`TypeRegistration::data`](bevy_reflect::TypeRegistration::data) if the asset was registered using [`register_asset_reflect`](crate::AssetApp::register_asset_reflect).
#[derive(Clone)]
pub struct ReflectAsset {
    handle_type_id: TypeId,
    add: fn(&mut World, &dyn PartialReflect) -> UntypedHandle,
    len: fn(&mut World) -> usize,
    ids: fn(&mut World) -> Vec<Entity>,
}

impl ReflectAsset {
    /// The [`TypeId`] of the [`Handle<T>`] for this asset
    pub fn handle_type_id(&self) -> TypeId {
        self.handle_type_id
    }

    /// Equivalent of [`Assets::add`]
    pub fn add(&self, world: &mut World, value: &dyn PartialReflect) -> UntypedHandle {
        (self.add)(world, value)
    }
    /// Equivalent of [`Assets::len`]
    pub fn len(&self, world: &mut World) -> usize {
        (self.len)(world)
    }

    /// Equivalent of [`Assets::is_empty`]
    pub fn is_empty(&self, world: &mut World) -> bool {
        self.len(world) == 0
    }

    /// Equivalent of [`Assets::ids`]
    pub fn ids<'w>(&self, world: &'w mut World) -> Vec<Entity> {
        (self.ids)(world)
    }
}

impl<A: Asset + FromReflect> CreateTypeData<A> for ReflectAsset {
    fn create_type_data(_input: ()) -> Self {
        ReflectAsset {
            handle_type_id: TypeId::of::<Handle<A>>(),
            add: |world, value| {
                let value: A = FromReflect::from_reflect(value)
                    .expect("could not call `FromReflect::from_reflect` in `ReflectAsset::add`");
                let handle: Handle<A> = world.spawn_asset(value);
                handle.untyped()
            },
            len: |world| world.query_filtered::<(), With<A>>().iter(world).len(),
            ids: |world| {
                world
                    .query_filtered::<Entity, With<A>>()
                    .iter(world)
                    .collect()
            },
        }
    }

    fn insert_dependencies(type_registration: &mut bevy_reflect::TypeRegistration) {
        type_registration.register_type_data::<ReflectComponent, A>();
    }
}

/// Reflect type data struct relating a [`Handle<T>`] back to the `T` asset type.
///
/// Say you want to look up the asset values of a list of handles when you have access to their `&dyn Reflect` form.
/// Assets can be looked up in the world using [`ReflectAsset`], but how do you determine which [`ReflectAsset`] to use when
/// only looking at the handle? [`ReflectHandle`] is stored in the type registry on each `Handle<T>` type, so you can use [`ReflectHandle::asset_type_id`] to look up
/// the [`ReflectAsset`] type data on the corresponding `T` asset type:
///
///
/// ```no_run
/// # use bevy_reflect::{TypeRegistry, prelude::*};
/// # use bevy_ecs::prelude::*;
/// use bevy_asset::{ReflectHandle, ReflectAsset};
///
/// # let world: &World = unimplemented!();
/// # let type_registry: TypeRegistry = unimplemented!();
/// let handles: Vec<&dyn Reflect> = unimplemented!();
/// for handle in handles {
///     let reflect_handle = type_registry.get_type_data::<ReflectHandle>(handle.type_id()).unwrap();
///     let reflect_asset = type_registry.get_type_data::<ReflectAsset>(reflect_handle.asset_type_id()).unwrap();
///
///     let handle = reflect_handle.downcast_handle_untyped(handle.as_any()).unwrap();
///     let value = reflect_asset.get(world, &handle).unwrap();
///     println!("{value:?}");
/// }
/// ```
#[derive(Clone)]
pub struct ReflectHandle {
    asset_type_id: TypeId,
    downcast_handle_untyped: fn(&dyn Any) -> Option<UntypedHandle>,
    typed: fn(UntypedHandle) -> Box<dyn Reflect>,
}

impl ReflectHandle {
    /// The [`TypeId`] of the asset
    pub fn asset_type_id(&self) -> TypeId {
        self.asset_type_id
    }

    /// A way to go from a [`Handle<T>`] in a `dyn Any` to a [`UntypedHandle`]
    pub fn downcast_handle_untyped(&self, handle: &dyn Any) -> Option<UntypedHandle> {
        (self.downcast_handle_untyped)(handle)
    }

    /// A way to go from a [`UntypedHandle`] to a [`Handle<T>`] in a `Box<dyn Reflect>`.
    /// Equivalent of [`UntypedHandle::typed`].
    pub fn typed(&self, handle: UntypedHandle) -> Box<dyn Reflect> {
        (self.typed)(handle)
    }
}

impl<A: Asset> CreateTypeData<Handle<A>> for ReflectHandle {
    fn create_type_data(_input: ()) -> Self {
        ReflectHandle {
            asset_type_id: TypeId::of::<A>(),
            downcast_handle_untyped: |handle: &dyn Any| {
                handle
                    .downcast_ref::<Handle<A>>()
                    .map(|h| h.clone().untyped())
            },
            typed: |handle: UntypedHandle| Box::new(handle.typed_unchecked::<A>()),
        }
    }
}

/// A [`ReflectSerializerProcessor`] that manually serializes [`Handle`] and [`UntypedHandle`], and
/// passes through for all other types.
///
/// [`Handle`]s cannot be serialized normally since it contains lots of ephemeral information (e.g.,
/// the [`AssetId`] of the asset being referenced). This processor serializes just the identifying
/// and stable parts of the handle. This can later be used to deserialize the handle.
///
/// Use [`HandleDeserializeProcessor`] to deserialize this data.
pub struct HandleSerializeProcessor {
    /// How ephemeral handles are dealt with.
    pub ephemeral_handle_behavior: EphemeralHandleBehavior,
}

/// Specifies the action that will be taken when attempting to serialize an ephemeral handle.
///
/// Ephemeral handles are handles to assets that were not loaded. Specifically, these are handles to
/// assets that were manually added.
#[derive(Clone, Copy, Debug)]
pub enum EphemeralHandleBehavior {
    /// Ephemeral handles are entirely ignored and are serialized as the default [`Handle`].
    Silent,
    /// A warning is logged, and the handle is serialized as the default [`Handle`].
    Warn,
    /// Serializing an ephemeral handle will cause serialization to return an error.
    Error,
}

impl ReflectSerializerProcessor for HandleSerializeProcessor {
    fn try_serialize<S>(
        &self,
        value: &dyn PartialReflect,
        registry: &TypeRegistry,
        serializer: S,
    ) -> Result<Result<S::Ok, S>, S::Error>
    where
        S: serde::Serializer,
    {
        let Some(value_reflect) = value.try_as_reflect() else {
            // Anything that isn't a concrete type should be serialized by the underlying
            // serializer, since it must not be a handle!
            return Ok(Err(serializer));
        };

        #[derive(Error, Debug)]
        #[error("Attempted to serialize an ephemeral asset handle {0:?} while `EphemeralHandleBehavior::Error` is set")]
        struct SerializingEphemeralHandleError(UntypedHandle);

        fn handle_reference_from_handle(
            handle: &UntypedHandle,
            ephemeral_handle_behavior: EphemeralHandleBehavior,
        ) -> Result<HandleReference, SerializingEphemeralHandleError> {
            Ok(match handle.path() {
                None => {
                    match ephemeral_handle_behavior {
                        EphemeralHandleBehavior::Silent => {}
                        EphemeralHandleBehavior::Warn => {
                            warn!("Serializing ephemeral handle {handle:?}. Ephemeral handles cannot be deserialized. Replacing with Handle::default");
                        }
                        EphemeralHandleBehavior::Error => {
                            return Err(SerializingEphemeralHandleError(handle.clone()))
                        }
                    }
                    HandleReference::Uuid(AssetId::<Empty>::DEFAULT_UUID)
                }
                Some(path) => HandleReference::Path(path.clone_owned()),
            })
        }

        if let Some(untyped_handle) = value_reflect.downcast_ref::<UntypedHandle>() {
            let Some(asset_registration) = registry.get(untyped_handle.type_id()) else {
                return Err(S::Error::custom(format!(
                    "Missing type registration for asset type of handle {:?}. Ensure the asset implements Reflect, includes #[reflect(Asset)], and is registered",
                    untyped_handle
                )));
            };
            return Ok(Ok(TypedHandleReference {
                asset_type: asset_registration.type_info().type_path().into(),
                reference: handle_reference_from_handle(
                    untyped_handle,
                    self.ephemeral_handle_behavior,
                )
                .map_err(S::Error::custom)?,
            }
            .serialize(serializer)?));
        }

        let Some(handle_registration) = registry.get(value_reflect.type_id()) else {
            // This is a slow path. Users are unlikely to be intentionally serializing types without
            // reflection, especially in production apps, so we can afford to be slow and give
            // better diagnostics.
            if let Some(type_info) = value_reflect.get_represented_type_info()
                && type_info.type_path().starts_with("bevy_asset::Handle")
            {
                warn!(
                    "HandleSerializeProcessor attempted to serialize a handle type \"{}\" without type data. This likely means the asset type was not registered.",
                    type_info.type_path()
                );
            }
            // Otherwise, fall back to the underlying serializer. Let it handle the error.
            return Ok(Err(serializer));
        };

        let Some(reflect_handle) = handle_registration.data::<ReflectHandle>() else {
            // This isn't an `UntypedHandle` and it isn't a `Handle<A>`, so just let the regular
            // serializer serialize it.
            return Ok(Err(serializer));
        };

        let untyped_handle = reflect_handle
            .downcast_handle_untyped(value_reflect.as_any())
            .expect("type includes `ReflectHandle` type data, so it must be a handle matching that type");

        let handle_reference =
            handle_reference_from_handle(&untyped_handle, self.ephemeral_handle_behavior)
                .map_err(S::Error::custom)?;

        Ok(Ok(handle_reference.serialize(serializer)?))
    }
}

/// A trait for loading an asset.
///
/// There are several ways to load an asset. This trait allows deserializing in many contexts
/// depending on how assets can be loaded. Note all these loads are deferred, and must have a
/// concrete type.
pub trait LoadFromPath {
    /// Initiates the load for the given expected type ID, and the path.
    ///
    /// See [`LoadBuilder::load_erased`](crate::LoadBuilder::load_erased) for more.
    fn load_from_path_erased(&mut self, type_id: TypeId, path: AssetPath<'static>)
        -> UntypedHandle;
}

impl LoadFromPath for LoadContext<'_> {
    fn load_from_path_erased(
        &mut self,
        type_id: TypeId,
        path: AssetPath<'static>,
    ) -> UntypedHandle {
        self.load_builder().load_erased(type_id, path)
    }
}

impl LoadFromPath for AssetServer {
    fn load_from_path_erased(
        &mut self,
        type_id: TypeId,
        path: AssetPath<'static>,
    ) -> UntypedHandle {
        self.load_builder().load_erased(type_id, path)
    }
}

impl LoadFromPath for &AssetServer {
    fn load_from_path_erased(
        &mut self,
        type_id: TypeId,
        path: AssetPath<'static>,
    ) -> UntypedHandle {
        self.load_builder().load_erased(type_id, path)
    }
}

/// A [`ReflectDeserializerProcessor`] that manually deserializes [`Handle`] and [`UntypedHandle`],
/// and passes through for all other types.
///
/// [`Handle`]s cannot be deserialized normally since it contains lots of ephemeral information
/// (e.g., the [`AssetId`] of the asset being referenced). This processor deserializes the
/// identifying and stable parts of the handle (usually serialized by [`HandleSerializeProcessor`]),
/// and triggers the load of that handle.
///
/// Use [`HandleSerializeProcessor`] to serialize data for this processor.
pub struct HandleDeserializeProcessor<'a> {
    /// The loader to load asset paths and retrieve their handles.
    pub load_from_path: &'a mut dyn LoadFromPath,
}

impl ReflectDeserializerProcessor for HandleDeserializeProcessor<'_> {
    fn try_deserialize<'de, D>(
        &mut self,
        registration: &bevy_reflect::TypeRegistration,
        registry: &TypeRegistry,
        deserializer: D,
    ) -> Result<Result<Box<dyn PartialReflect>, D>, D::Error>
    where
        D: Deserializer<'de>,
    {
        if registration.type_id() == TypeId::of::<UntypedHandle>() {
            let typed_handle_reference = TypedHandleReference::deserialize(deserializer)?;
            let Some(asset_type) = registry.get_with_type_path(&typed_handle_reference.asset_type)
            else {
                return Err(D::Error::custom(format!(
                    "Could not find asset type by name \"{}\" for UntypedHandle",
                    typed_handle_reference.asset_type
                )));
            };
            let type_id = asset_type.type_id();
            return Ok(Ok(Box::new(match typed_handle_reference.reference {
                HandleReference::Path(path) => {
                    self.load_from_path.load_from_path_erased(type_id, path)
                }
                HandleReference::Uuid(uuid) => todo!("Re-add UUID support"),
            })));
        }

        let Some(reflect_handle) = registration.data::<ReflectHandle>() else {
            // This type isn't an `UntypedHandle`, and it isn't a `Handle<A>`, so let the regular
            // serializer deal with it. Note: it's possible the handle just never got its reflect
            // data serialized, but most types will fall into this category, so we can't give a
            // warning here.
            return Ok(Err(deserializer));
        };

        let handle_reference = HandleReference::deserialize(deserializer)?;

        let type_id = reflect_handle.asset_type_id;
        Ok(Ok(reflect_handle.typed(match handle_reference {
            HandleReference::Path(path) => self.load_from_path.load_from_path_erased(type_id, path),
            HandleReference::Uuid(uuid) => todo!("Re-add UUID support"),
        })))
    }
}

/// The "stable" data of a handle that can be serialized and deserialized.
#[derive(Serialize, Deserialize)]
pub enum HandleReference {
    /// The handle references an asset path that needs to be loaded.
    Path(AssetPath<'static>),
    /// The handle references a constant [`Uuid`].
    Uuid(Uuid),
}

/// The "stable" data of a handle whose asset type information is stored internally.
#[derive(Serialize, Deserialize)]
pub struct TypedHandleReference {
    /// The type path of the asset type this handle references.
    pub asset_type: Cow<'static, str>,
    /// The reference for this handle.
    pub reference: HandleReference,
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, vec, vec::Vec};
    use core::any::TypeId;
    use ron::ser::PrettyConfig;
    use serde::de::DeserializeSeed;
    use std::path::Path;

    use crate::{
        tests::{create_app, run_app_until, CoolText, CoolTextLoader, CoolTextRon, SubText},
        Asset, AssetApp, AssetServer, DirectAssetAccessExt, EphemeralHandleBehavior, Handle,
        HandleDeserializeProcessor, HandleSerializeProcessor, ReflectAsset, UntypedHandle,
    };
    use bevy_ecs::reflect::{AppTypeRegistry, ReflectComponent};
    use bevy_reflect::{
        serde::{TypedReflectDeserializer, TypedReflectSerializer},
        FromReflect, Reflect, TypePath,
    };

    #[derive(Asset, Reflect)]
    #[reflect(Asset)]
    struct AssetType {
        field: String,
    }

    #[test]
    fn test_reflect_asset_operations() {
        let mut app = create_app().0;
        app.init_asset::<AssetType>()
            .register_asset_reflect::<AssetType>();

        let (reflect_asset, reflect_component) = {
            let type_registry = app.world().resource::<AppTypeRegistry>();
            let type_registry = type_registry.read();

            (
                type_registry
                    .get_type_data::<ReflectAsset>(TypeId::of::<AssetType>())
                    .unwrap()
                    .clone(),
                type_registry
                    .get_type_data::<ReflectComponent>(TypeId::of::<AssetType>())
                    .unwrap()
                    .clone(),
            )
        };

        let value = AssetType {
            field: "test".into(),
        };

        let handle = reflect_asset.add(app.world_mut(), &value);
        // struct is a reserved keyword, so we can't use it here
        let mut reflect = reflect_component
            .reflect_mut(app.world_mut().entity_mut(handle.entity()))
            .unwrap();
        let strukt = reflect.reflect_mut().as_struct().unwrap();
        strukt
            .field_mut("field")
            .unwrap()
            .apply(&String::from("edited"));

        assert_eq!(reflect_asset.len(app.world_mut()), 1);
        let ids: Vec<_> = reflect_asset.ids(app.world_mut());
        assert_eq!(ids.len(), 1);
        let id = ids[0];

        let asset = reflect_component.reflect(app.world().entity(id)).unwrap();
        assert_eq!(asset.downcast_ref::<AssetType>().unwrap().field, "edited");

        reflect_component.remove(&mut app.world_mut().entity_mut(id));
        assert_eq!(reflect_asset.len(app.world_mut()), 0);
    }

    fn serialize_as_cool_text(text: &str) -> String {
        let cool_text_ron = CoolTextRon {
            text: text.into(),
            dependencies: vec![],
            embedded_dependencies: vec![],
            sub_texts: vec![],
        };
        ron::ser::to_string_pretty(&cool_text_ron, PrettyConfig::new().new_line("\n")).unwrap()
    }

    #[test]
    fn roundtrip_reflect_serialize_handles() {
        #[derive(Asset, TypePath)]
        struct OtherAsset;

        #[derive(Reflect)]
        struct Stuff {
            typed: Handle<CoolText>,
            untyped: UntypedHandle,
            ephemeral: Handle<OtherAsset>,
        }

        // Initial app to serialize a `Stuff` instance.
        let ron_data = {
            let (mut app, dir) = create_app();
            app.init_asset::<OtherAsset>()
                .init_asset::<CoolText>()
                .init_asset::<SubText>()
                // Normally reflection auto registration would mean we don't need this, but that
                // feature may not be set for tests, so register the types manually just in case.
                .register_asset_reflect::<CoolText>()
                .register_type::<Stuff>()
                .register_asset_loader(CoolTextLoader);

            dir.insert_asset_text(Path::new("abc.cool.ron"), &serialize_as_cool_text("hello"));
            dir.insert_asset_text(Path::new("def.cool.ron"), &serialize_as_cool_text("world"));

            let type_registry = app.world().resource::<AppTypeRegistry>().0.clone();
            let asset_server = app.world().resource::<AssetServer>().clone();

            let untyped = asset_server.load_builder().load_untyped("def.cool.ron");
            run_app_until(&mut app, |_| {
                asset_server.is_loaded(untyped.entity()).then_some(())
            });
            let ephemeral = app.world_mut().spawn_asset(OtherAsset);

            let stuff = Stuff {
                typed: asset_server.load("abc.cool.ron"),
                untyped,
                ephemeral,
            };

            let type_registry = type_registry.read();
            let processor = HandleSerializeProcessor {
                ephemeral_handle_behavior: EphemeralHandleBehavior::Silent,
            };
            let reflect_serializer =
                TypedReflectSerializer::with_processor(&stuff, &type_registry, &processor);

            ron::to_string(&reflect_serializer).unwrap()
        };

        // Create a new app to deserialize the serialized data.
        let (mut app, dir) = create_app();
        app.init_asset::<OtherAsset>()
            .init_asset::<CoolText>()
            .init_asset::<SubText>()
            // See above for why we register these manually.
            .register_asset_reflect::<CoolText>()
            .register_type::<Stuff>()
            .register_asset_loader(CoolTextLoader);

        dir.insert_asset_text(Path::new("abc.cool.ron"), &serialize_as_cool_text("hello"));
        dir.insert_asset_text(Path::new("def.cool.ron"), &serialize_as_cool_text("world"));

        let type_registry = app.world().resource::<AppTypeRegistry>().0.clone();
        let mut asset_server = app.world().resource::<AssetServer>().clone();

        let type_registry = type_registry.read();
        let mut processor = HandleDeserializeProcessor {
            load_from_path: &mut asset_server,
        };
        let reflect_deserializer = TypedReflectDeserializer::with_processor(
            type_registry.get(TypeId::of::<Stuff>()).unwrap(),
            &type_registry,
            &mut processor,
        );

        let mut ron_deserializer = ron::Deserializer::from_str(&ron_data).unwrap();
        let stuff = Stuff::from_reflect(
            reflect_deserializer
                .deserialize(&mut ron_deserializer)
                .unwrap()
                .as_ref(),
        )
        .unwrap();

        // The ephemeral handle was replaced by the default handle.
        todo!("Check this");
        // assert_eq!(stuff.ephemeral, Handle::default());

        // The deserializer should have caused the handles to start loading.
        run_app_until(&mut app, |_| {
            (asset_server.is_loaded(&stuff.typed) && asset_server.is_loaded(stuff.untyped.entity()))
                .then_some(())
        });

        // Make sure that the handles actually do end up with the correct assets.
        assert_eq!(
            app.world()
                .get::<CoolText>(stuff.typed.entity())
                .unwrap()
                .text,
            "hello"
        );
        assert_eq!(
            app.world()
                .get::<CoolText>(stuff.untyped.entity())
                .unwrap()
                .text,
            "world"
        );
    }
}
