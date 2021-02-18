pub mod core;
pub mod schedule;
pub mod system;

pub mod prelude {
    pub use crate::{
        core::{
            Added, Changed, Entity, Flags, Mut, Mutated, QueryState, With, WithBundle, Without,
            World,
        },
        schedule::{
            ExclusiveSystemDescriptorCoercion, ParallelSystemDescriptorCoercion, Schedule, Stage,
            State, StateStage, SystemStage,
        },
        system::{
            Commands, In, IntoChainSystem, IntoExclusiveSystem, IntoSystem, Local, NonSend,
            NonSendMut, Query, QuerySet, RemovedComponents, Res, ResMut, System,
        },
    };
}
