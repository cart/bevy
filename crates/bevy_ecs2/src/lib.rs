mod core;
mod resource;
mod schedule;
mod system;

#[cfg(test)]
mod tests;

pub use crate::core::*;
pub use bevy_ecs_macros2::*;
pub use lazy_static;
pub use resource::*;
pub use schedule::*;
pub use system::{Query, *};

pub mod prelude {
    pub use crate::{
        resource::{ChangedRes, FromResources, Local, Res, ResMut, Resource, Resources},
        schedule::{Schedule, State, StateStage, SystemStage},
        system::{Commands, IntoSystem, Query, System},
        Added, Bundle, Changed, Component, Entity, Flags, In, IntoChainSystem, Mut, Mutated, Or,
        QuerySet, With, Without, World,
    };
}