use uuid::Uuid;
pub use bevy_derive::TypeUuid;

pub trait TypeUuid {
    const TYPE_UUID: Uuid;
}