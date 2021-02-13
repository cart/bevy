pub mod core;
pub mod schedule;
pub mod system;

pub mod prelude {
    pub use crate::{
        core::{Added, Changed, Entity, Mut, Mutated, QueryState, With, Without, World},
        schedule::{
            ExclusiveSystemDescriptorCoercion, ParallelSystemDescriptorCoercion, Schedule, Stage,
            SystemStage,
        },
        system::{IntoExclusiveSystem, IntoSystem, Local, Query, QuerySet, System},
    };
}