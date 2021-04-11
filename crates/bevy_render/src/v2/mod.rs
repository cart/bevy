pub mod render_graph;
pub mod features;

use bevy_ecs::{prelude::{IntoSystem, System, World}, schedule::SystemDescriptor};
use self::render_graph::RenderGraph;


pub struct RenderApp {
    extract_systems: Vec<Box<dyn System<In = (), Out = ()>>>,
    world: World,
    graph: RenderGraph,
}

impl RenderApp {
    pub fn add_extract_system<Params, I: IntoSystem<Params, S>, S: System<In = (), Out =()>>(&mut self, system: I) -> &mut Self {
        self.extract_systems.push(Box::new(system.system()));
        self
    }
}
