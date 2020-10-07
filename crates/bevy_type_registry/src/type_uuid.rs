pub use bevy_derive::TypeUuid;
use uuid::Uuid;

pub trait TypeUuid {
    const TYPE_UUID: Uuid;
}
