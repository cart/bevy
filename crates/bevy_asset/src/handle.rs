use crate::{
    meta::MetaTransform, reflect::ReflectHandle, Asset, AssetId, AssetPath, AssetReference,
    AssetServer,
};
use alloc::sync::Arc;
use bevy_ecs::{
    entity::{ContainsEntity, Entity, EntityHandle},
    template::{EntityTemplate, FromTemplate, SpecializeFromTemplate, Template, TemplateContext},
};
use bevy_platform::{collections::Equivalent, sync::Mutex};
use bevy_reflect::{Reflect, TypePath};
use core::{
    any::TypeId,
    fmt::Debug,
    hash::{Hash, Hasher},
    marker::PhantomData,
};
use disqualified::ShortName;
use thiserror::Error;
use uuid::Uuid;

#[derive(Reflect)]
#[reflect(Debug, Hash, PartialEq, Clone, Handle)]
pub struct Handle<T: Asset> {
    pub(crate) entity_handle: EntityHandle<AssetData>,
    #[reflect(ignore, clone)]
    pub(crate) _marker: PhantomData<T>,
}

#[derive(TypePath, Default)]
pub struct AssetData {
    pub path: Option<AssetPath<'static>>,
    pub uuid: Option<Uuid>,
    pub is_default: bool,
    /// The [`Asset`] [`TypeId`] hint provided when requesting this asset's handle. This will only be
    /// set if the type was provided as part of the initial asset load.
    pub type_id_hint: Option<TypeId>,
    pub meta_transform: Option<MetaTransform>,
}

impl AssetData {
    pub fn new<A: Asset>() -> Self {
        Self {
            type_id_hint: Some(TypeId::of::<A>()),
            ..Default::default()
        }
    }
}

impl Debug for AssetData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AssetData")
            .field("path", &self.path)
            .field("uuid", &self.uuid)
            .field("is_default", &self.is_default)
            .field("type_id_hint", &self.type_id_hint)
            .finish()
    }
}

impl<A: Asset> Handle<A> {
    /// Returns the [`AssetId`] of this [`Asset`].
    #[inline]
    pub fn id(&self) -> AssetId<A> {
        AssetId {
            entity: self.entity_handle.id(),
            marker: PhantomData,
        }
    }

    pub fn entity(&self) -> Entity {
        self.entity_handle.id()
    }

    pub fn is_default(&self) -> bool {
        self.entity_handle.is_default
    }

    /// Returns the path if this is (1) a strong handle and (2) the asset has a path
    #[inline]
    pub fn path(&self) -> Option<&AssetPath<'static>> {
        self.entity_handle.path.as_ref()
    }

    #[inline]
    pub fn uuid(&self) -> Option<Uuid> {
        self.entity_handle.uuid
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        self.entity_handle.strong_count()
    }

    /// Converts this [`Handle`] to an "untyped" / "generic-less" [`UntypedHandle`], which stores the [`Asset`] type information
    /// _inside_ [`UntypedHandle`]. This will return [`UntypedHandle::Strong`] for [`Handle::Strong`] and [`UntypedHandle::Uuid`] for
    /// [`Handle::Uuid`].
    #[inline]
    pub fn untyped(self) -> UntypedHandle {
        self.into()
    }
}

// This enables FromTemplate specialization for `Handle<T>` using the
// ["auto trait specialization" trick](https://github.com/coolcatcoder/rust_techniques/issues/1)
// This enables Handle to implement Default _and_ implement FromTemplate, without conflicting with the
// blanket impl of FromTemplate for T: Default + Clone.
impl<T: Asset> Unpin for Handle<T> where for<'a> [()]: SpecializeFromTemplate {}

impl<T: Asset> FromTemplate for Handle<T> {
    type Template = HandleTemplate<T>;
}

/// A [`Template`] that produces a [`Handle`].
///
/// # How asset paths are resolved in templates
///
/// When a type with a [`Handle<T>`] field derives [`FromTemplate`], that field is replaced by its
/// template type, [`HandleTemplate<T>`], when created via BSN.
/// We can see that [`HandleTemplate<T>`] has the following trait impl block:
///
/// ```rust, ignore
/// impl<I: Into<AssetPath<'static>>, T: Asset> From<I> for HandleTemplate<T> {
///     fn from(value: I) -> Self {
///         Self::Path(value.into())
///     }
/// }
/// ```
///
/// [`AssetPath<'static>`] implements [`From<&'static str>`].
/// Because of that, assigning a string literal to a `Handle<T>` field automatically converts it into
/// [`HandleTemplate<T>::Path`] with that asset path when used in the `bsn!` macro.
/// Calls to `bsn!` automatically insert `.into()` conversions, and due to Rust's blanket impl that turns [`From`] trait impls into their [`Into`]
/// equivalents, the conversion from `&'static str` to `AssetPath<'static>` is handled automatically.
/// Finally, the [`HandleTemplate<T>::Path`] generated gets converted to a [`Handle<T>`] during scene initialization,
/// as the asset is loaded from the given path, and the resulting handle is assigned to the field,
/// pointing to the asset that was found at the file path in our original string.
#[derive(Reflect)]
pub enum HandleTemplate<T: Asset> {
    /// Creates a [`Handle`] by loading [`AssetReference::Default`].
    Default,
    /// Creates a [`Handle`] by calling [`AssetServer::load`] on the given [`AssetPath`].
    Path(AssetPath<'static>),
    /// Creates a [`Handle`] by calling [`AssetServer::load`] on the given [`Uuid`].
    Uuid(Uuid),
    /// Creates a [`Handle`] by cloning the given [`Handle`] value.
    Handle(Handle<T>),
    /// Creates a [`Handle`] by adding the given asset value using [`AssetServer::add`]. This will
    /// cache the resulting [`Handle`] on the template and reuse it for future template builds.
    ///
    /// This should generally be constructed using [`HandleTemplate::value`] or [`asset_value`].
    Value(ArcMutexValue<T>),
    EntityTemplate(EntityTemplate),
}

impl<T: Asset> HandleTemplate<T> {
    /// This will create a new [`HandleTemplate`] for the given `asset` value. This makes it possible
    /// to define assets "inline" in templates / scenes that produce a [`Handle`].
    ///
    /// This supports [`Into`]
    /// to automatically convert values that can become `A`.
    pub fn value(value: impl Into<T>) -> Self {
        HandleTemplate::Value(ArcMutexValue(Arc::new(Mutex::new(AssetOrHandle::Value(
            Some(value.into()),
        )))))
    }
}

/// Stores an [`Arc<Mutex<AssetOrHandle<T>>>`].
///
/// This intermediary type exists largely to enable reflect(opaque).
#[derive(Reflect)]
#[reflect(opaque)]
pub struct ArcMutexValue<T: Asset>(Arc<Mutex<AssetOrHandle<T>>>);

impl<T: Asset> Clone for ArcMutexValue<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Reflect)]
enum AssetOrHandle<T: Asset> {
    Value(Option<T>),
    Handle(Handle<T>),
}

impl<T: Asset> Default for AssetOrHandle<T> {
    fn default() -> Self {
        Self::Value(None)
    }
}

impl<T: Asset> Default for HandleTemplate<T> {
    fn default() -> Self {
        Self::Default
    }
}

impl<I: Into<AssetPath<'static>>, T: Asset> From<I> for HandleTemplate<T> {
    #[inline]
    fn from(value: I) -> Self {
        Self::Path(value.into())
    }
}

impl<T: Asset> From<Handle<T>> for HandleTemplate<T> {
    #[inline]
    fn from(value: Handle<T>) -> Self {
        Self::Handle(value)
    }
}

impl<T: Asset> From<Uuid> for HandleTemplate<T> {
    #[inline]
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl<T: Asset> From<EntityTemplate> for HandleTemplate<T> {
    #[inline]
    fn from(value: EntityTemplate) -> Self {
        Self::EntityTemplate(value)
    }
}

impl<T: Asset> From<Entity> for HandleTemplate<T> {
    #[inline]
    fn from(value: Entity) -> Self {
        Self::EntityTemplate(EntityTemplate::Entity(value))
    }
}
impl<T: Asset> Template for HandleTemplate<T> {
    type Output = Handle<T>;
    #[allow(unsafe_code, reason = "Improved performance on high traffic type")]
    fn build_template(&self, context: &mut TemplateContext) -> bevy_ecs::error::Result<Handle<T>> {
        Ok(match self {
            HandleTemplate::Default => context
                .resource::<AssetServer>()
                .load(AssetReference::Default),
            HandleTemplate::Path(asset_path) => context.resource::<AssetServer>().load(asset_path),
            HandleTemplate::Uuid(uuid) => context
                .resource::<AssetServer>()
                .load(AssetReference::Uuid(*uuid)),
            HandleTemplate::Handle(handle) => handle.clone(),
            HandleTemplate::Value(value) => {
                // This unwrap is ok. If another caller panicked while holding this mutex, then the
                // program is in an invalid state and this should panic too.
                let mut value_or_handle = value.0.lock().unwrap();
                match &mut *value_or_handle {
                    AssetOrHandle::Value(value) => {
                        // This unwrap is ok because AssetOrHandle::Value will always either contain a Some Value
                        // when it is in this state (AssetOrHandle is private).
                        let handle = context.resource::<AssetServer>().add(value.take().unwrap());
                        *value_or_handle = AssetOrHandle::Handle(handle.clone());
                        handle
                    }
                    AssetOrHandle::Handle(handle) => handle.clone(),
                }
            }
            HandleTemplate::EntityTemplate(entity_template) => {
                let entity = entity_template.build_template(context)?;
                // SAFETY: we've checked that this is a different entity
                let world = unsafe { context.entity.world_mut() };
                world
                    .get_entity_mut(entity)?
                    .handle_with_data(AssetData::new::<T>())
                    .into()
            }
        })
    }

    fn clone_template(&self) -> Self {
        match self {
            HandleTemplate::Default => HandleTemplate::Default,
            HandleTemplate::Path(asset_path) => HandleTemplate::Path(asset_path.clone()),
            HandleTemplate::Handle(handle) => HandleTemplate::Handle(handle.clone()),
            HandleTemplate::Value(value) => HandleTemplate::Value(value.clone()),
            HandleTemplate::Uuid(uuid) => HandleTemplate::Uuid(*uuid),
            HandleTemplate::EntityTemplate(entity_template) => {
                HandleTemplate::EntityTemplate(entity_template.clone())
            }
        }
    }
}

/// This will create a new [`HandleTemplate`] for the given `asset` value. This makes it possible
/// to define assets "inline" in templates / scenes that produce a [`Handle`].
///
/// This supports [`Into`]
/// to automatically convert values that can become `A`.
pub fn asset_value<I: Into<A>, A: Asset>(asset: I) -> HandleTemplate<A> {
    HandleTemplate::value(asset)
}

impl<T: Asset> PartialEq for Handle<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.entity_handle.entity() == other.entity_handle.entity()
    }
}

impl<T: Asset> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            entity_handle: self.entity_handle.clone(),
            _marker: self._marker.clone(),
        }
    }
}

impl<A: Asset> Debug for Handle<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = ShortName::of::<A>();
        write!(
            f,
            "Handle<{name}>{{ entity: {}, type_id: {:?}, path: {:?} }}",
            self.entity(),
            TypeId::of::<A>(),
            self.path()
        )
    }
}

impl<A: Asset> From<EntityHandle<AssetData>> for Handle<A> {
    fn from(entity_handle: EntityHandle<AssetData>) -> Self {
        Handle {
            entity_handle,
            _marker: PhantomData,
        }
    }
}

impl<A: Asset> Hash for Handle<A> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl<A: Asset> Into<Entity> for &Handle<A> {
    #[inline]
    fn into(self) -> Entity {
        self.entity()
    }
}

// Handle uses AssetId when hashing. This enables using AssetId instead of handle with hashsets and hashmaps.
impl<T: Asset> Equivalent<Handle<T>> for AssetId<T> {
    fn equivalent(&self, key: &Handle<T>) -> bool {
        *self == key.id()
    }
}

impl<A: Asset> PartialOrd for Handle<A> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<A: Asset> Ord for Handle<A> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.entity().cmp(&other.entity())
    }
}

impl<A: Asset> Eq for Handle<A> {}

impl<A: Asset> From<&Handle<A>> for AssetId<A> {
    #[inline]
    fn from(value: &Handle<A>) -> Self {
        value.id()
    }
}

impl<A: Asset> From<&mut Handle<A>> for AssetId<A> {
    #[inline]
    fn from(value: &mut Handle<A>) -> Self {
        value.id()
    }
}

/// An untyped variant of [`Handle`], which internally stores the [`Asset`] type information at runtime
/// as a [`TypeId`] instead of encoding it in the compile-time type. This allows handles across [`Asset`] types
/// to be stored together and compared.
///
/// See [`Handle`] for more information.
#[derive(Clone, Reflect)]
pub struct UntypedHandle(pub(crate) EntityHandle<AssetData>);

impl UntypedHandle {
    pub fn entity(&self) -> Entity {
        self.0.id()
    }

    /// Returns the path if this is (1) a strong handle and (2) the asset has a path
    #[inline]
    pub fn path(&self) -> Option<&AssetPath<'static>> {
        self.0.path.as_ref()
    }

    /// Returns whether or not this is the default asset.
    #[inline]
    pub fn is_default(&self) -> bool {
        self.0.is_default
    }

    #[inline]
    pub fn uuid(&self) -> Option<Uuid> {
        self.0.uuid
    }

    /// Converts to a typed Handle. This _will not check if the target Handle type matches_.
    #[inline]
    pub fn typed_unchecked<A: Asset>(self) -> Handle<A> {
        Handle {
            entity_handle: self.0,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn type_id_hint(&self) -> Option<TypeId> {
        self.0.type_id_hint
    }

    /// The "meta transform" for the strong handle. This will only be [`Some`] if the handle is strong and there is a meta transform
    /// associated with it.
    #[inline]
    pub fn meta_transform(&self) -> Option<&MetaTransform> {
        self.0.meta_transform.as_ref()
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        self.0.strong_count()
    }
}

impl PartialEq for UntypedHandle {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.entity() == other.entity()
    }
}

impl Eq for UntypedHandle {}

impl Hash for UntypedHandle {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.entity().hash(state);
    }
}

impl Into<Entity> for &UntypedHandle {
    #[inline]
    fn into(self) -> Entity {
        self.entity()
    }
}

impl Debug for UntypedHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Handle{{ id: {}", self.entity(),)?;

        if let Some(path) = self.path() {
            write!(f, ", path: {}", path)?;
        }
        if let Some(uuid) = self.uuid() {
            write!(f, ", uuid: {}", uuid)?;
        }
        if self.is_default() {
            write!(f, ", is_default")?;
        }
        if let Some(type_hint) = self.type_id_hint() {
            write!(f, ", type_id_hint: {:?}", type_hint)?;
        }

        write!(f, " }}")
    }
}

impl PartialOrd for UntypedHandle {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.entity().partial_cmp(&other.entity())
    }
}

// Cross Operations

impl<A: Asset> PartialEq<UntypedHandle> for Handle<A> {
    #[inline]
    fn eq(&self, other: &UntypedHandle) -> bool {
        self.entity() == other.entity()
    }
}

impl<A: Asset> PartialEq<Handle<A>> for UntypedHandle {
    #[inline]
    fn eq(&self, other: &Handle<A>) -> bool {
        other.eq(self)
    }
}

impl<A: Asset> PartialOrd<UntypedHandle> for Handle<A> {
    #[inline]
    fn partial_cmp(&self, other: &UntypedHandle) -> Option<core::cmp::Ordering> {
        self.entity().partial_cmp(&other.entity())
    }
}

impl<A: Asset> PartialOrd<Handle<A>> for UntypedHandle {
    #[inline]
    fn partial_cmp(&self, other: &Handle<A>) -> Option<core::cmp::Ordering> {
        Some(other.partial_cmp(self)?.reverse())
    }
}

impl<A: Asset> From<Handle<A>> for UntypedHandle {
    fn from(value: Handle<A>) -> Self {
        UntypedHandle(value.entity_handle)
    }
}

/// Errors preventing the conversion of to/from an [`UntypedHandle`] and a [`Handle`].
#[derive(Error, Debug, PartialEq, Clone)]
#[non_exhaustive]
pub enum UntypedAssetConversionError {
    /// Caused when trying to convert an [`UntypedHandle`] into a [`Handle`] of the wrong type.
    #[error(
        "This UntypedHandle is for {found:?} and cannot be converted into a Handle<{expected:?}>"
    )]
    TypeIdMismatch {
        /// The expected [`TypeId`] of the [`Handle`] being converted to.
        expected: TypeId,
        /// The [`TypeId`] of the [`UntypedHandle`] being converted from.
        found: TypeId,
    },
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use bevy_ecs::world::World;
    use bevy_platform::hash::FixedHasher;
    use bevy_reflect::{FromReflect, PartialReflect};
    use core::hash::BuildHasher;

    use crate::{meta::Empty, tests::create_app, AssetApp, DirectAssetAccessExt};

    use super::*;

    /// Simple utility to directly hash a value using a fixed hasher
    fn hash<T: Hash>(data: &T) -> u64 {
        FixedHasher.hash_one(data)
    }

    /// Typed and Untyped `Handles` should be equivalent to each other and themselves
    #[test]
    fn equality() {
        let mut world = World::new();
        let typed = world.spawn_asset(Empty);
        let untyped = typed.clone().untyped();

        assert_eq!(typed, untyped.clone().typed_unchecked::<Empty>());
        assert_eq!(UntypedHandle::from(typed.clone()), untyped);
        assert_eq!(typed, untyped);
    }

    /// Typed and Untyped `Handles` should be orderable amongst each other and themselves
    /// Note that orderings rely on Entity Ord, which is the opposite of what it is expected to be (higher indices == smaller)
    #[test]
    #[expect(
        clippy::cmp_owned,
        reason = "This lints on the assertion that a typed handle converted to an untyped handle maintains its ordering compared to an untyped handle. While the conversion would normally be useless, we need to ensure that converted handles maintain their ordering, making the conversion necessary here."
    )]
    fn ordering() {
        let mut world = World::new();
        let typed_1 = world.spawn_asset(Empty);
        let typed_2 = world.spawn_asset(Empty);
        let untyped_1 = typed_1.clone().untyped();
        let untyped_2 = typed_2.clone().untyped();

        assert!(typed_1 > typed_2);
        assert!(untyped_1 > untyped_2);

        assert!(UntypedHandle::from(typed_1.clone()) > untyped_2);
        assert!(untyped_1 > UntypedHandle::from(typed_2.clone()));

        assert!(untyped_1.clone().typed_unchecked::<Empty>() > typed_2);
        assert!(typed_1 > untyped_2.clone().typed_unchecked::<Empty>());

        assert!(typed_1 > untyped_2);
        assert!(untyped_1 > typed_2);
    }

    /// Typed and Untyped `Handles` should be equivalently hashable to each other and themselves
    #[test]
    fn hashing() {
        let mut world = World::new();
        let typed = world.spawn_asset(Empty);
        let untyped = typed.clone().untyped();

        assert_eq!(
            hash(&typed),
            hash(&untyped.clone().typed_unchecked::<Empty>())
        );
        assert_eq!(hash(&typed.clone().untyped()), hash(&untyped));
        assert_eq!(hash(&typed), hash(&untyped));
    }

    /// Typed and Untyped `Handles` should be interchangeable
    #[test]
    fn conversion() {
        let mut world = World::new();
        let typed = world.spawn_asset(Empty);
        let untyped = typed.clone().untyped();

        assert_eq!(typed, untyped.clone().typed_unchecked());
        assert_eq!(typed.clone().untyped(), untyped);
    }

    /// `PartialReflect::reflect_clone`/`PartialReflect::to_dynamic` should increase the strong count of a strong handle
    #[test]
    fn strong_handle_reflect_clone() {
        #[derive(Asset, Reflect)]
        struct MyAsset {
            value: u32,
        }

        let mut app = create_app().0;
        app.init_asset::<MyAsset>();

        let handle: Handle<MyAsset> = app.world_mut().spawn_asset(MyAsset { value: 1 });
        assert_eq!(
            handle.strong_count(),
            1,
            "Inserting the asset should result in a strong count of 1"
        );

        let reflected: &dyn Reflect = &handle;
        let _cloned_handle: Box<dyn Reflect> = reflected.reflect_clone().unwrap();

        assert_eq!(
            handle.strong_count(),
            2,
            "Cloning the handle with reflect should increase the strong count to 2"
        );

        let dynamic_handle: Box<dyn PartialReflect> = reflected.to_dynamic();

        assert_eq!(
            handle.strong_count(),
            3,
            "Converting the handle to a dynamic should increase the strong count to 3"
        );

        let from_reflect_handle: Handle<MyAsset> =
            FromReflect::from_reflect(&*dynamic_handle).unwrap();

        assert_eq!(
            from_reflect_handle.strong_count(),
            4,
            "Converting the reflected value back to a handle should increase the strong count to 4"
        );
    }
}
